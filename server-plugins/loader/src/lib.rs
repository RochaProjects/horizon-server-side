//! Runtime plugin loader. Unsafe ABI handling is isolated in this crate.

use horizon_server_plugin_api::{
    ABI_VERSION, ByteSlice, HostApiV1, MenuItemV1, PluginEntryV1, PluginV1, ServerEventV1,
};
use libloading::Library;
use std::{
    ffi::c_void,
    fs,
    path::Path,
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
}

struct HostBridge<'a> {
    host: &'a mut dyn Host,
}

struct LoadedPlugin {
    descriptor: &'static PluginV1,
    state: *mut c_void,
    _library: Library,
}

// Calls are serialized on the server thread while this global lock is held.
unsafe impl Send for LoadedPlugin {}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        if let Some(destroy) = self.descriptor.destroy {
            // SAFETY: state was created by this still-loaded plugin.
            unsafe { destroy(self.state) };
        }
    }
}

#[derive(Default)]
struct Loader {
    loaded: bool,
    plugins: Vec<LoadedPlugin>,
}
static LOADER: OnceLock<Mutex<Loader>> = OnceLock::new();
fn loader() -> &'static Mutex<Loader> {
    LOADER.get_or_init(|| Mutex::new(Loader::default()))
}
fn uuid_parts(uuid: u128) -> (u64, u64) {
    ((uuid >> 64) as u64, uuid as u64)
}

unsafe extern "C" fn send_message(
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

unsafe extern "C" fn register_command(context: *mut c_void, bytes: ByteSlice, len: usize) -> i32 {
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

unsafe extern "C" fn open_menu(
    context: *mut c_void,
    high: u64,
    low: u64,
    title: ByteSlice,
    title_len: usize,
    rows: u8,
    items: *const MenuItemV1,
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

fn host_api(host: &mut dyn Host) -> (HostApiV1, HostBridge<'_>) {
    (
        HostApiV1 {
            abi_version: ABI_VERSION,
            context: std::ptr::null_mut(),
            register_command: Some(register_command),
            send_message: Some(send_message),
            open_menu: Some(open_menu),
        },
        HostBridge { host },
    )
}
fn bind_api(api: &mut HostApiV1, bridge: &mut HostBridge<'_>) {
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
    let entry = unsafe { library.get::<PluginEntryV1>(b"horizon_server_plugin_v1\0") }
        .map_err(|e| e.to_string())?;
    let ptr = unsafe { entry() };
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
    let state = unsafe { descriptor.create.expect("validado")() };
    if state.is_null() {
        return Err("plugin não conseguiu iniciar".into());
    }
    let descriptor = unsafe { &*ptr };
    Ok(LoadedPlugin {
        descriptor,
        state,
        _library: library,
    })
}

fn plugin_name(plugin: &PluginV1) -> String {
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
    for plugin in &mut state.plugins {
        let (mut api, mut bridge) = host_api(host);
        bind_api(&mut api, &mut bridge);
        // SAFETY: plugin state, descriptor and call-scoped API are valid.
        unsafe { plugin.descriptor.register_commands.expect("validado")(plugin.state, &api) };
    }
}

pub fn perform_command(root: &Path, host: &mut dyn Host, player: u128, command: &str) -> bool {
    let lock = load_once(root);
    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
    let (high, low) = uuid_parts(player);
    for plugin in &mut state.plugins {
        let (mut api, mut bridge) = host_api(host);
        bind_api(&mut api, &mut bridge);
        // SAFETY: pointers refer to this synchronous call.
        let handled = unsafe {
            plugin.descriptor.command.expect("validado")(
                plugin.state,
                &api,
                high,
                low,
                command.as_ptr(),
                command.len(),
            ) != 0
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
    for plugin in &mut state.plugins {
        let (mut api, mut bridge) = host_api(host);
        bind_api(&mut api, &mut bridge);
        // SAFETY: plugin state and call-scoped host API are valid.
        unsafe {
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
        };
    }
}

pub fn dispatch_event(root: &Path, host: &mut dyn Host, event: &Event) -> bool {
    let lock = load_once(root);
    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut canceled = false;
    let (mut api, mut bridge) = host_api(host);
    bind_api(&mut api, &mut bridge);
    let (player_high, player_low) = uuid_parts(event.player);
    let c_event = ServerEventV1 {
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
    for plugin in &mut state.plugins {
        if let Some(callback) = plugin.descriptor.event {
            // SAFETY: event strings and host bridge outlive each synchronous callback.
            canceled |= unsafe { callback(plugin.state, &api, &c_event) != 0 };
        }
    }
    canceled
}
