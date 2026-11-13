//! Runtime plugin loader. Unsafe ABI handling is isolated in this crate.

use horizon_server_plugin_api::{
    ABI_VERSION, ByteSlice, HostApiV2, ItemDeltaV2, MenuItemV2, PluginEntryV2, PluginV2,
    ServerEventV2,
};
use libloading::Library;
use serde::{Deserialize, Serialize};
use std::{
    ffi::c_void,
    fs::{self, File, OpenOptions},
    io::Write,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub slot: usize,
    pub item: String,
    pub name: String,
    pub color: String,
    pub lore: Vec<String>,
    pub count: u32,
}

#[derive(Clone, Debug)]
pub struct ItemDelta {
    pub item: String,
    pub delta: i32,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub kind: u32,
    pub player: u128,
    pub name: String,
    pub dimension: String,
    pub window_id: i32,
    pub position: [i32; 3],
    pub data_id: u32,
}

pub trait Host {
    fn register_command(&mut self, usage: &str);
    fn send_message(&mut self, player: u128, message: &str) -> bool;
    fn open_menu(
        &mut self,
        player: u128,
        title: &str,
        rows: u8,
        items: Vec<MenuItem>,
    ) -> Option<i32>;
    /// Applies these item changes on the server thread. `finish(true)` must
    /// durably commit the balance; `finish(false)` rolls it back if applying
    /// the inventory fails. Implementations must validate before committing.
    fn transact_items(
        &mut self,
        player: u128,
        items: &[ItemDelta],
        finish: &mut dyn FnMut(bool) -> bool,
    ) -> Result<(), ()>;
}

struct HostBridge<'a> {
    host: &'a mut dyn Host,
    balances: &'a mut BalanceStore,
}

struct LoadedPlugin {
    descriptor: &'static PluginV2,
    state: *mut c_void,
    disabled: bool,
    _library: Library,
}

// Calls are serialized on the server thread while this global lock is held.
unsafe impl Send for LoadedPlugin {}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        if let Some(destroy) = self.descriptor.destroy {
            // SAFETY: the v2 ABI permits unwinding and state belongs to this library.
            let _ = catch_unwind(AssertUnwindSafe(|| unsafe { destroy(self.state) }));
        }
    }
}

#[derive(Default)]
struct Loader {
    loaded: bool,
    plugins: Vec<LoadedPlugin>,
    balances: BalanceStore,
}

#[derive(Default)]
struct BalanceStore {
    path: Option<PathBuf>,
    accounts: std::collections::HashMap<(String, u128), i64>,
    available: bool,
}

#[derive(Serialize, Deserialize)]
struct BalanceFile {
    version: u32,
    accounts: Vec<BalanceAccount>,
}

#[derive(Serialize, Deserialize)]
struct BalanceAccount {
    currency: String,
    player: String,
    units: i64,
}

impl BalanceStore {
    fn load(&mut self, plugins_dir: &Path) {
        if self.path.is_some() {
            return;
        }
        let path = plugins_dir.join("balances.json");
        self.path = Some(path.clone());
        self.available = true;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                self.available = false;
                log::error!("Não foi possível ler {}: {error}", path.display());
                return;
            }
        };
        let parsed = serde_json::from_slice::<BalanceFile>(&bytes);
        let Ok(file) = parsed else {
            self.available = false;
            log::error!(
                "{} está inválido; transações de saldo desativadas",
                path.display()
            );
            return;
        };
        if file.version != 1 {
            self.available = false;
            log::error!(
                "Versão desconhecida em {}; transações de saldo desativadas",
                path.display()
            );
            return;
        }
        for account in file.accounts {
            let Some(player) = u128::from_str_radix(&account.player, 16).ok() else {
                self.available = false;
                log::error!(
                    "UUID inválido em {}; transações de saldo desativadas",
                    path.display()
                );
                return;
            };
            if account.units < 0 || !valid_currency(&account.currency) {
                self.available = false;
                log::error!(
                    "Conta inválida em {}; transações de saldo desativadas",
                    path.display()
                );
                return;
            }
            self.accounts
                .insert((account.currency, player), account.units);
        }
    }

    fn balance(&self, currency: &str, player: u128) -> Option<i64> {
        self.available.then(|| {
            self.accounts
                .get(&(currency.to_owned(), player))
                .copied()
                .unwrap_or(0)
        })
    }

    fn replace(&mut self, accounts: std::collections::HashMap<(String, u128), i64>) -> bool {
        if !self.available {
            return false;
        }
        let Some(path) = self.path.as_ref() else {
            return false;
        };
        let mut serialized: Vec<_> = accounts
            .iter()
            .map(|((currency, player), units)| BalanceAccount {
                currency: currency.clone(),
                player: format!("{player:032x}"),
                units: *units,
            })
            .collect();
        serialized.sort_by(|a, b| (&a.currency, &a.player).cmp(&(&b.currency, &b.player)));
        let bytes = match serde_json::to_vec(&BalanceFile {
            version: 1,
            accounts: serialized,
        }) {
            Ok(bytes) => bytes,
            Err(error) => {
                log::error!("Falha ao serializar saldos: {error}");
                return false;
            }
        };
        let Some(parent) = path.parent() else {
            return false;
        };
        if let Err(error) = fs::create_dir_all(parent) {
            log::error!("Falha ao criar {}: {error}", parent.display());
            return false;
        }
        let temp = path.with_extension("json.tmp");
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, path)?;
            Ok::<_, std::io::Error>(())
        })();
        if let Err(error) = write_result {
            log::error!("Falha ao salvar {}: {error}", path.display());
            let _ = fs::remove_file(temp);
            return false;
        }
        self.accounts = accounts;
        if let Err(error) = File::open(parent).and_then(|directory| directory.sync_all()) {
            log::warn!(
                "{} foi atualizado, mas não foi possível sincronizar a pasta: {error}",
                path.display()
            );
        }
        true
    }
}

fn valid_currency(currency: &str) -> bool {
    !currency.is_empty()
        && currency.len() <= 128
        && currency
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:.-".contains(&b))
}
static LOADER: OnceLock<Mutex<Loader>> = OnceLock::new();
fn loader() -> &'static Mutex<Loader> {
    LOADER.get_or_init(|| Mutex::new(Loader::default()))
}
fn uuid_parts(uuid: u128) -> (u64, u64) {
    ((uuid >> 64) as u64, uuid as u64)
}

unsafe extern "C-unwind" fn send_message(
    context: *mut c_void,
    high: u64,
    low: u64,
    bytes: ByteSlice,
    len: usize,
) -> i32 {
    if context.is_null() || bytes.is_null() || len > 32_768 {
        return 0;
    }
    // SAFETY: host functions are available only during this synchronous callback.
    let bridge = unsafe { &mut *(context.cast::<HostBridge<'_>>()) };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    let Ok(message) = std::str::from_utf8(bytes) else {
        return 0;
    };
    i32::from(
        bridge
            .host
            .send_message((u128::from(high) << 64) | u128::from(low), message),
    )
}

unsafe extern "C-unwind" fn register_command(
    context: *mut c_void,
    bytes: ByteSlice,
    len: usize,
) -> i32 {
    if context.is_null() || bytes.is_null() || len == 0 || len > 512 {
        return 0;
    }
    // SAFETY: context and command bytes are valid during the synchronous callback.
    let bridge = unsafe { &mut *(context.cast::<HostBridge<'_>>()) };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    let Ok(usage) = std::str::from_utf8(bytes) else {
        return 0;
    };
    bridge.host.register_command(usage);
    1
}

unsafe extern "C-unwind" fn open_menu(
    context: *mut c_void,
    high: u64,
    low: u64,
    title: ByteSlice,
    title_len: usize,
    rows: u8,
    items: *const MenuItemV2,
    item_count: usize,
) -> i32 {
    if context.is_null()
        || title.is_null()
        || title_len > 256
        || item_count > 54
        || (item_count > 0 && items.is_null())
        || !(1..=6).contains(&rows)
    {
        return -1;
    }
    // SAFETY: pointer validity is bounded to the synchronous ABI call.
    let bridge = unsafe { &mut *(context.cast::<HostBridge<'_>>()) };
    let title_bytes = unsafe { std::slice::from_raw_parts(title, title_len) };
    let Ok(title) = std::str::from_utf8(title_bytes) else {
        return -1;
    };
    let raw_items = if item_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(items, item_count) }
    };
    let mut converted = Vec::with_capacity(raw_items.len());
    for item in raw_items {
        if item.item.is_null()
            || item.name.is_null()
            || item.color.is_null()
            || item.item_len > 256
            || item.name_len > 512
            || item.color_len > 32
            || item.lore.is_null()
            || item.lore_len > 4096
            || item.slot >= u32::from(rows) * 9
        {
            return -1;
        }
        let read = |ptr: ByteSlice, len: usize| -> Option<String> {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            Some(std::str::from_utf8(bytes).ok()?.to_owned())
        };
        let (Some(id), Some(name), Some(color), Some(lore)) = (
            read(item.item, item.item_len),
            read(item.name, item.name_len),
            read(item.color, item.color_len),
            read(item.lore, item.lore_len),
        ) else {
            return -1;
        };
        converted.push(MenuItem {
            slot: item.slot as usize,
            item: id,
            name,
            color,
            lore: lore
                .split('\n')
                .map(str::to_owned)
                .filter(|line| !line.is_empty())
                .collect(),
            count: item.count,
        });
    }
    bridge
        .host
        .open_menu(
            (u128::from(high) << 64) | u128::from(low),
            title,
            rows,
            converted,
        )
        .unwrap_or(-1)
}

unsafe extern "C-unwind" fn get_balance(
    context: *mut c_void,
    high: u64,
    low: u64,
    currency: ByteSlice,
    currency_len: usize,
    output: *mut i64,
) -> i32 {
    if context.is_null() || currency.is_null() || output.is_null() || currency_len > 128 {
        return 0;
    }
    // SAFETY: callback context and input are valid for this synchronous ABI call.
    let bridge = unsafe { &mut *(context.cast::<HostBridge<'_>>()) };
    let bytes = unsafe { std::slice::from_raw_parts(currency, currency_len) };
    let Ok(currency) = std::str::from_utf8(bytes) else {
        return 0;
    };
    if !valid_currency(currency) {
        return 0;
    }
    let player = (u128::from(high) << 64) | u128::from(low);
    let Some(balance) = bridge.balances.balance(currency, player) else {
        return 4;
    };
    unsafe { *output = balance };
    1
}

unsafe extern "C-unwind" fn transact(
    context: *mut c_void,
    high: u64,
    low: u64,
    currency: ByteSlice,
    currency_len: usize,
    balance_delta: i64,
    raw_items: *const ItemDeltaV2,
    item_count: usize,
) -> i32 {
    if context.is_null()
        || currency.is_null()
        || currency_len == 0
        || currency_len > 128
        || item_count > 16
        || (item_count > 0 && raw_items.is_null())
    {
        return 5;
    }
    // SAFETY: callback context and all input pointers are valid for this synchronous ABI call.
    let bridge = unsafe { &mut *(context.cast::<HostBridge<'_>>()) };
    let currency_bytes = unsafe { std::slice::from_raw_parts(currency, currency_len) };
    let Ok(currency) = std::str::from_utf8(currency_bytes) else {
        return 5;
    };
    if !valid_currency(currency) || !bridge.balances.available {
        return if bridge.balances.available { 5 } else { 4 };
    }
    let raw_items = if item_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(raw_items, item_count) }
    };
    let mut normalized = std::collections::BTreeMap::<String, i64>::new();
    for item in raw_items {
        if item.item.is_null() || item.item_len == 0 || item.item_len > 256 || item.delta == 0 {
            return 5;
        }
        let bytes = unsafe { std::slice::from_raw_parts(item.item, item.item_len) };
        let Ok(id) = std::str::from_utf8(bytes) else {
            return 5;
        };
        if !valid_item_id(id) {
            return 5;
        }
        *normalized.entry(id.to_owned()).or_default() += i64::from(item.delta);
    }
    let mut items = Vec::with_capacity(normalized.len());
    for (item, delta) in normalized {
        if delta == 0 {
            continue;
        }
        let Ok(delta) = i32::try_from(delta) else {
            return 5;
        };
        items.push(ItemDelta { item, delta });
    }
    let player = (u128::from(high) << 64) | u128::from(low);
    let Some(old_balance) = bridge.balances.balance(currency, player) else {
        return 4;
    };
    let Some(new_balance) = old_balance.checked_add(balance_delta) else {
        return if balance_delta < 0 { 2 } else { 5 };
    };
    if new_balance < 0 {
        return 2;
    }
    let old_accounts = bridge.balances.accounts.clone();
    let mut new_accounts = old_accounts.clone();
    let key = (currency.to_owned(), player);
    if new_balance == 0 {
        new_accounts.remove(&key);
    } else {
        new_accounts.insert(key.clone(), new_balance);
    }
    let HostBridge { host, balances } = bridge;
    let mut storage_error = false;
    let result = host.transact_items(player, &items, &mut |commit| {
        if commit {
            if balances.replace(new_accounts.clone()) {
                true
            } else {
                storage_error = true;
                false
            }
        } else if balances.replace(old_accounts.clone()) {
            true
        } else {
            storage_error = true;
            false
        }
    });
    match result {
        Ok(()) => 1,
        Err(()) if storage_error => 4,
        Err(()) => 3,
    }
}

fn valid_item_id(item: &str) -> bool {
    let Some((namespace, path)) = item.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace.len() <= 64
        && path.len() <= 128
        && item
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:/.-".contains(&b))
}

fn host_api<'a>(
    host: &'a mut dyn Host,
    balances: &'a mut BalanceStore,
) -> (HostApiV2, HostBridge<'a>) {
    (
        HostApiV2 {
            abi_version: ABI_VERSION,
            context: std::ptr::null_mut(),
            register_command: Some(register_command),
            send_message: Some(send_message),
            open_menu: Some(open_menu),
            get_balance: Some(get_balance),
            transact: Some(transact),
        },
        HostBridge { host, balances },
    )
}
fn bind_api(api: &mut HostApiV2, bridge: &mut HostBridge<'_>) {
    api.context = (bridge as *mut HostBridge<'_>).cast();
}

fn load_once(root: &Path) -> &'static Mutex<Loader> {
    let lock = loader();
    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
    if state.loaded {
        return lock;
    }
    state.loaded = true;
    let dir = root.join("plugins");
    state.balances.load(&dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        log::info!(
            "Sem pasta de plugins Horizon em {}; plugins nativos desativados",
            dir.display()
        );
        return lock;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if !is_library(&path) {
            continue;
        }
        // SAFETY: runtime plugins are trusted native code and validated before use.
        match unsafe { load_plugin(&path) } {
            Ok(plugin) => {
                log::info!(
                    "Plugin Horizon carregado: {} ({})",
                    plugin_name(plugin.descriptor),
                    path.display()
                );
                state.plugins.push(plugin);
            }
            Err(error) => log::error!(
                "Falha ao carregar plugin Horizon {}: {error}",
                path.display()
            ),
        }
    }
    lock
}

fn is_library(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        if cfg!(target_os = "windows") {
            ext == "dll"
        } else if cfg!(target_os = "macos") {
            ext == "dylib"
        } else {
            ext == "so"
        }
    })
}

unsafe fn load_plugin(path: &Path) -> Result<LoadedPlugin, String> {
    // SAFETY: the library remains resident for the complete plugin lifetime.
    let library = unsafe { Library::new(path) }.map_err(|e| e.to_string())?;
    let entry = unsafe { library.get::<PluginEntryV2>(b"horizon_server_plugin_v2\0") }
        .map_err(|e| e.to_string())?;
    let entry = *entry;
    let ptr = catch_unwind(AssertUnwindSafe(|| unsafe { entry() }))
        .map_err(|_| "panic na função de entrada do plugin".to_owned())?;
    if ptr.is_null() {
        return Err("plugin retornou descritor nulo".into());
    }
    let descriptor = unsafe { &*ptr };
    if descriptor.abi_version != ABI_VERSION {
        return Err(format!(
            "ABI {} incompatível; servidor usa ABI {ABI_VERSION}",
            descriptor.abi_version
        ));
    }
    if descriptor.name.is_null()
        || descriptor.name_len == 0
        || descriptor.name_len > 128
        || descriptor.create.is_none()
        || descriptor.destroy.is_none()
        || descriptor.register_commands.is_none()
        || descriptor.command.is_none()
        || descriptor.menu_click.is_none()
    {
        return Err("descritor incompleto".into());
    }
    let state = catch_unwind(AssertUnwindSafe(|| unsafe {
        descriptor.create.expect("validado")()
    }))
    .map_err(|_| "panic ao iniciar o plugin".to_owned())?;
    if state.is_null() {
        return Err("plugin não conseguiu iniciar".into());
    }
    let descriptor = unsafe { &*ptr };
    Ok(LoadedPlugin {
        descriptor,
        state,
        disabled: false,
        _library: library,
    })
}

fn plugin_name(plugin: &PluginV2) -> String {
    if plugin.name.is_null() || plugin.name_len > 128 {
        return "<nome inválido>".into();
    }
    // SAFETY: descriptor name is static plugin-owned data.
    let bytes = unsafe { std::slice::from_raw_parts(plugin.name, plugin.name_len) };
    std::str::from_utf8(bytes)
        .unwrap_or("<nome inválido>")
        .to_owned()
}

pub fn register_commands(root: &Path, host: &mut dyn Host) {
    let lock = load_once(root);
    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
    let Loader {
        plugins, balances, ..
    } = &mut *state;
    for plugin in plugins {
        if plugin.disabled {
            continue;
        }
        let (mut api, mut bridge) = host_api(host, balances);
        bind_api(&mut api, &mut bridge);
        // SAFETY: plugin state, descriptor and call-scoped API are valid; C-unwind lets the host catch panics.
        if catch_unwind(AssertUnwindSafe(|| unsafe {
            plugin.descriptor.register_commands.expect("validado")(plugin.state, &api)
        }))
        .is_err()
        {
            plugin.disabled = true;
            log::error!(
                "Plugin {} desativado após panic",
                plugin_name(plugin.descriptor)
            );
        }
    }
}

pub fn perform_command(root: &Path, host: &mut dyn Host, player: u128, command: &str) -> bool {
    let lock = load_once(root);
    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
    let (high, low) = uuid_parts(player);
    let Loader {
        plugins, balances, ..
    } = &mut *state;
    for plugin in plugins {
        if plugin.disabled {
            continue;
        }
        let (mut api, mut bridge) = host_api(host, balances);
        bind_api(&mut api, &mut bridge);
        // SAFETY: pointers refer to this synchronous call; C-unwind lets the host catch panics.
        let result = catch_unwind(AssertUnwindSafe(|| unsafe {
            plugin.descriptor.command.expect("validado")(
                plugin.state,
                &api,
                high,
                low,
                command.as_ptr(),
                command.len(),
            ) != 0
        }));
        let Ok(handled) = result else {
            plugin.disabled = true;
            log::error!(
                "Plugin {} desativado após panic",
                plugin_name(plugin.descriptor)
            );
            continue;
        };
        if handled {
            return true;
        }
    }
    false
}

pub fn menu_click(
    root: &Path,
    host: &mut dyn Host,
    player: u128,
    window: i32,
    slot: i32,
    button: i32,
    mode: i32,
) {
    let lock = load_once(root);
    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
    let (high, low) = uuid_parts(player);
    let Loader {
        plugins, balances, ..
    } = &mut *state;
    for plugin in plugins {
        if plugin.disabled {
            continue;
        }
        let (mut api, mut bridge) = host_api(host, balances);
        bind_api(&mut api, &mut bridge);
        // SAFETY: plugin state and call-scoped host API are valid; C-unwind lets the host catch panics.
        if catch_unwind(AssertUnwindSafe(|| unsafe {
            plugin.descriptor.menu_click.expect("validado")(
                plugin.state,
                &api,
                high,
                low,
                window,
                slot,
                button,
                mode,
            )
        }))
        .is_err()
        {
            plugin.disabled = true;
            log::error!(
                "Plugin {} desativado após panic",
                plugin_name(plugin.descriptor)
            );
        }
    }
}

pub fn dispatch_event(root: &Path, host: &mut dyn Host, event: &Event) -> bool {
    let lock = load_once(root);
    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut canceled = false;
    let Loader {
        plugins, balances, ..
    } = &mut *state;
    let (mut api, mut bridge) = host_api(host, balances);
    bind_api(&mut api, &mut bridge);
    let (player_high, player_low) = uuid_parts(event.player);
    let c_event = ServerEventV2 {
        kind: event.kind,
        player_high,
        player_low,
        name: event.name.as_ptr(),
        name_len: event.name.len(),
        dimension: event.dimension.as_ptr(),
        dimension_len: event.dimension.len(),
        window_id: event.window_id,
        position: event.position,
        data_id: event.data_id,
    };
    for plugin in plugins {
        if plugin.disabled {
            continue;
        }
        if let Some(callback) = plugin.descriptor.event {
            // SAFETY: event strings and host bridge outlive each synchronous callback.
            match catch_unwind(AssertUnwindSafe(|| unsafe {
                callback(plugin.state, &api, &c_event) != 0
            })) {
                Ok(was_canceled) => canceled |= was_canceled,
                Err(_) => {
                    plugin.disabled = true;
                    log::error!(
                        "Plugin {} desativado após panic",
                        plugin_name(plugin.descriptor)
                    );
                }
            }
        }
    }
    canceled
}
