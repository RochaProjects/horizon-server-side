//! Loja em português baseada na lista de itens e no fluxo de telas do AuraEssentials Java.

use horizon_server_plugin_api::{
    ABI_VERSION_V3, HostApiV3, ItemDeltaV2, PluginV3, event, operation,
    result as transaction_result,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::c_void,
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

const NAME: &[u8] = b"AuraEssentialsHorizon";
const DEFAULT_ITEMS_JSON: &str = include_str!("../shop.json");
const DEFAULT_CONFIG_JSON: &str = r#"{
  "title": "Loja",
  "currency": {
    "normal": "aura:coins",
    "vip": "aura:credits"
  }
}"#;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShopItem {
    display_name: String,
    material: String,
    icon: Option<String>,
    item_model: Option<String>,
    components: Option<HashMap<String, serde_json::Value>>,
    lore: Option<Vec<String>>,
    #[serde(default = "one")]
    amount: u32,
    buy: f64,
    sell: f64,
}

#[derive(Clone, Deserialize, Serialize)]
struct ShopConfig {
    #[serde(default = "default_title")]
    title: String,
    #[serde(default)]
    currency: CurrencyConfig,
}

#[derive(Clone, Deserialize, Serialize)]
struct CurrencyConfig {
    #[serde(default = "default_coins")]
    normal: String,
    #[serde(default = "default_credits")]
    vip: String,
}

impl Default for CurrencyConfig {
    fn default() -> Self {
        Self {
            normal: default_coins(),
            vip: default_credits(),
        }
    }
}

fn default_title() -> String {
    "Loja".into()
}
fn default_coins() -> String {
    "aura:coins".into()
}
fn default_credits() -> String {
    "aura:credits".into()
}

fn one() -> u32 {
    1
}

#[derive(Clone)]
struct Category {
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    items: Vec<ShopItem>,
}

struct State {
    loaded: bool,
    config: ShopConfig,
    categories: Vec<Category>,
    views: HashMap<(u128, i32), View>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            loaded: false,
            config: ShopConfig {
                title: default_title(),
                currency: CurrencyConfig::default(),
            },
            categories: Vec::new(),
            views: HashMap::new(),
        }
    }
}

#[derive(Clone)]
enum View {
    Categories(HashMap<i32, usize>),
    Items {
        category: usize,
        page: usize,
        rows: u8,
        slots: HashMap<i32, usize>,
    },
    Detail {
        category: usize,
        page: usize,
        item: usize,
        quantity: u32,
    },
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn plugin_dir() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|root| root.join("plugins").join("AuraEssentialsHorizon"))
}

fn ensure_default_files() -> Result<PathBuf, String> {
    let dir =
        plugin_dir().ok_or_else(|| "não foi possível localizar a pasta do servidor".to_owned())?;
    fs::create_dir_all(&dir)
        .map_err(|error| format!("não foi possível criar {}: {error}", dir.display()))?;
    let shop = dir.join("shop.json");
    if !shop.exists() {
        fs::write(
            &shop,
            format!(
                "{}\n",
                serde_json::to_string_pretty(
                    &serde_json::from_str::<serde_json::Value>(DEFAULT_ITEMS_JSON)
                        .map_err(|error| error.to_string())?
                )
                .map_err(|error| error.to_string())?
            ),
        )
        .map_err(|error| format!("não foi possível criar shop.json: {error}"))?;
    }
    let config = dir.join("config.json");
    if !config.exists() {
        fs::write(&config, DEFAULT_CONFIG_JSON)
            .map_err(|error| format!("não foi possível criar config.json: {error}"))?;
    }
    Ok(dir)
}

fn load_shop() -> Result<Vec<Category>, String> {
    let dir = ensure_default_files()?;
    let bytes = fs::read(dir.join("shop.json"))
        .map_err(|error| format!("não foi possível ler shop.json: {error}"))?;
    let data: HashMap<String, Vec<ShopItem>> =
        serde_json::from_slice(&bytes).map_err(|error| format!("shop.json inválido: {error}"))?;
    let categories = category_meta()
        .into_iter()
        .map(|(id, title, icon)| Category {
            id,
            title,
            icon,
            items: data.get(id).cloned().unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    if categories.iter().all(|category| category.items.is_empty()) {
        return Err("shop.json não tem itens em nenhuma categoria".into());
    }
    Ok(categories)
}

fn load_config() -> Result<ShopConfig, String> {
    let dir = ensure_default_files()?;
    let bytes = fs::read(dir.join("config.json"))
        .map_err(|error| format!("não foi possível ler config.json: {error}"))?;
    let config: ShopConfig =
        serde_json::from_slice(&bytes).map_err(|error| format!("config.json inválido: {error}"))?;
    for currency in [&config.currency.normal, &config.currency.vip] {
        if currency.is_empty()
            || currency.len() > 128
            || !currency.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_:.-".contains(&byte)
            })
        {
            return Err("config.json contém um identificador de moeda inválido".into());
        }
    }
    Ok(config)
}

fn ensure_loaded(state: &mut State) {
    if state.loaded {
        return;
    }
    let loaded =
        load_shop().and_then(|categories| load_config().map(|config| (categories, config)));
    match loaded {
        Ok((categories, config)) => {
            state.categories = categories;
            state.config = config;
        }
        Err(error) => {
            eprintln!("[AuraEssentialsHorizon] {error}; usando a lista interna inicial");
            state.categories = categories_from_json(DEFAULT_ITEMS_JSON).unwrap_or_default();
        }
    }
    state.loaded = true;
}

fn categories_from_json(json: &str) -> Result<Vec<Category>, String> {
    let data: HashMap<String, Vec<ShopItem>> =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    Ok(category_meta()
        .into_iter()
        .map(|(id, title, icon)| Category {
            id,
            title,
            icon,
            items: data.get(id).cloned().unwrap_or_default(),
        })
        .collect())
}

fn category_meta() -> [(&'static str, &'static str, &'static str); 8] {
    [
        ("blocos", "Blocos", "bricks"),
        ("minerios", "Minérios", "diamond"),
        ("drops_mobs", "Drops de Mobs", "blaze_rod"),
        ("comida", "Comida", "cooked_beef"),
        ("plantacao", "Plantação", "wheat"),
        ("livros_encantados", "Livros Encantados", "enchanted_book"),
        ("madeiras", "Madeiras", "oak_log"),
        ("loja_vip", "Loja VIP", "nether_star"),
    ]
}

fn bytes(s: &str) -> (*const u8, usize) {
    (s.as_ptr(), s.len())
}

struct ItemSpec {
    slot: usize,
    item: String,
    name: String,
    color: String,
    lore: Vec<String>,
    components: HashMap<String, serde_json::Value>,
    count: u32,
}

fn spec(slot: usize, item: &str, name: &str, color: &str, count: u32) -> ItemSpec {
    ItemSpec {
        slot,
        item: item.into(),
        name: name.into(),
        color: color.into(),
        lore: Vec::new(),
        components: HashMap::new(),
        count,
    }
}

fn spec_lore(mut item: ItemSpec, lore: Vec<String>) -> ItemSpec {
    item.lore = lore;
    item
}

fn show_menu(
    host: &HostApiV3,
    player: u128,
    title: &str,
    rows: u8,
    items: Vec<ItemSpec>,
) -> Option<i32> {
    let menu_items: Vec<_> = items
        .iter()
        .map(|item| {
            serde_json::json!({
                "slot": item.slot,
                "item": item.item,
                "count": item.count,
                "name": {"text": item.name, "color": item.color, "italic": false},
                "lore": item.lore,
                "components": item.components,
            })
        })
        .collect();
    let result = call_json(
        host,
        player,
        serde_json::json!({
            "operation": operation::MENU_OPEN,
            "data": {"title": title, "rows": rows, "items": menu_items},
        }),
    )?;
    result
        .get("window_id")?
        .as_i64()
        .and_then(|id| i32::try_from(id).ok())
}

fn call_json(
    host: &HostApiV3,
    player: u128,
    request: serde_json::Value,
) -> Option<serde_json::Value> {
    let callback = host.call_json?;
    let input = request.to_string();
    let (high, low) = ((player >> 64) as u64, player as u64);
    let mut output = vec![0_u8; 65_536];
    let mut output_len = 0_usize;
    // SAFETY: input/output buffers remain valid for the synchronous callback.
    let status = unsafe {
        callback(
            host.context,
            high,
            low,
            input.as_ptr(),
            input.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut output_len,
        )
    };
    (status == transaction_result::OK && output_len <= output.len())
        .then(|| serde_json::from_slice(&output[..output_len]).ok())
        .flatten()
}

fn message(host: &HostApiV3, player: u128, text: &str) {
    if let Some(send) = host.common.send_message {
        let (high, low) = ((player >> 64) as u64, player as u64);
        // SAFETY: text remains valid for this synchronous call.
        unsafe { send(host.common.context, high, low, text.as_ptr(), text.len()) };
    }
}

fn transact_shop_item(
    host: &HostApiV3,
    player: u128,
    item: &ShopItem,
    currency: &str,
    quantity: u32,
    buying: bool,
) -> i32 {
    let Some(transact) = host.common.transact else {
        return transaction_result::STORAGE_ERROR;
    };
    let units = if buying { item.buy } else { item.sell };
    if !units.is_finite() || units < 0.0 {
        return transaction_result::INVALID_REQUEST;
    }
    let balance_delta = (units * f64::from(quantity) * 100.0).round();
    if balance_delta > i64::MAX as f64 {
        return transaction_result::INVALID_REQUEST;
    }
    let amount = item.amount.checked_mul(quantity);
    let Some(amount) = amount.and_then(|n| i32::try_from(n).ok()) else {
        return transaction_result::INVALID_REQUEST;
    };
    if amount == 0 {
        return transaction_result::INVALID_REQUEST;
    }
    let material = item.material.to_ascii_lowercase();
    let item_id = format!("minecraft:{material}");
    let (item_ptr, item_len) = bytes(&item_id);
    let (currency_ptr, currency_len) = bytes(currency);
    let delta = if buying {
        -(balance_delta as i64)
    } else {
        balance_delta as i64
    };
    let item_delta = ItemDeltaV2 {
        item: item_ptr,
        item_len,
        delta: if buying { amount } else { -amount },
    };
    let (high, low) = ((player >> 64) as u64, player as u64);
    // SAFETY: the API pointers and item id remain alive for this synchronous host call.
    unsafe {
        transact(
            host.common.context,
            high,
            low,
            currency_ptr,
            currency_len,
            delta,
            &item_delta,
            1,
        )
    }
}

fn trade_message(host: &HostApiV3, player: u128, result: i32, buying: bool) {
    let text = match result {
        transaction_result::OK if buying => "Compra concluída.",
        transaction_result::OK => "Venda concluída.",
        transaction_result::INSUFFICIENT_BALANCE => "Saldo insuficiente.",
        transaction_result::ITEMS_REJECTED if buying => {
            "Não há espaço suficiente no inventário para receber os itens."
        }
        transaction_result::ITEMS_REJECTED => "Você não tem os itens necessários no inventário.",
        transaction_result::STORAGE_ERROR => {
            "Falha ao salvar a transação. Confira os logs do servidor."
        }
        _ => "Não foi possível concluir a transação.",
    };
    message(host, player, text);
}

fn balance_text(host: &HostApiV3, player: u128, currency: &str) -> Option<String> {
    let get = host.common.get_balance?;
    let (currency_ptr, currency_len) = bytes(currency);
    let (high, low) = ((player >> 64) as u64, player as u64);
    let mut balance = 0_i64;
    // SAFETY: output and currency bytes live through this synchronous host call.
    let result = unsafe {
        get(
            host.common.context,
            high,
            low,
            currency_ptr,
            currency_len,
            &mut balance,
        )
    };
    (result == transaction_result::OK).then(|| price_units(balance))
}

fn price_units(units: i64) -> String {
    let sign = if units < 0 { "-" } else { "" };
    let absolute = units.unsigned_abs();
    let whole = absolute / 100;
    let fraction = absolute % 100;
    if fraction == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole},{fraction:02}")
            .trim_end_matches('0')
            .to_owned()
    }
}

fn open_categories(state: &mut State, host: &HostApiV3, player: u128) {
    ensure_loaded(state);
    let mut mapping = HashMap::new();
    let items = state
        .categories
        .iter()
        .enumerate()
        .map(|(index, category)| {
            let slot = match category.id {
                "blocos" => 10,
                "minerios" => 11,
                "drops_mobs" => 12,
                "comida" => 13,
                "plantacao" => 14,
                "livros_encantados" => 15,
                "madeiras" => 16,
                _ => 22,
            };
            mapping.insert(slot as i32, index);
            spec(
                slot,
                &format!("minecraft:{}", category.icon),
                category.title,
                "white",
                1,
            )
        })
        .collect();
    state.views.retain(|(uuid, _), _| *uuid != player);
    if let Some(window) = show_menu(host, player, &state.config.title, 3, items) {
        state
            .views
            .insert((player, window), View::Categories(mapping));
    } else {
        message(host, player, "Não foi possível abrir a loja.");
    }
}

fn open_items(
    state: &mut State,
    host: &HostApiV3,
    player: u128,
    category: usize,
    requested_page: usize,
) {
    let Some(cat) = state.categories.get(category) else {
        return;
    };
    let item_count = cat.items.len();
    let item_rows = item_count.saturating_sub(4).max(1).div_ceil(9).max(1);
    let rows = (item_rows + 1).clamp(3, 6) as u8;
    let size = usize::from(rows) * 9;
    let nav_row = size - 9;
    let slots: Vec<usize> = (0..nav_row)
        .chain([nav_row, nav_row + 1, nav_row + 7, nav_row + 8])
        .collect();
    let page_size = slots.len();
    let pages = item_count.div_ceil(page_size).max(1);
    let page = requested_page.min(pages - 1);
    let start = page * page_size;
    let visible = item_count.saturating_sub(start).min(page_size);
    let mut mapping = HashMap::new();
    let mut items = Vec::new();
    for (i, slot) in slots.iter().take(visible).enumerate() {
        let item_index = start + i;
        let item = &cat.items[item_index];
        mapping.insert(*slot as i32, item_index);
        let item_color = "yellow";
        let lore = if cat.id == "loja_vip" {
            vec![
                "§7Produto VIP".into(),
                "§8----------------".into(),
                format!("§7Comprar: §6{} Créditos", price(item.buy)),
                "§aClique para comprar".into(),
            ]
        } else {
            vec![
                "§7Quantidade: §f1x".into(),
                format!("§7Comprar: §6{} moedas", price(item.buy)),
                format!("§7Vender: §6{} moedas", price(item.sell)),
                "§8----------------".into(),
                "§aClique esquerdo: §7comprar 1x".into(),
                "§bClique direito: §7vender 1x".into(),
            ]
        };
        let icon = item
            .icon
            .as_deref()
            .unwrap_or(&item.material)
            .to_ascii_lowercase();
        let product = spec(
            *slot,
            &format!("minecraft:{icon}"),
            &item.display_name,
            item_color,
            item.amount.clamp(1, 64),
        );
        let mut product = product;
        if let Some(model) = &item.item_model {
            product.components.insert(
                "minecraft:item_model".into(),
                serde_json::Value::String(model.clone()),
            );
        }
        if let Some(components) = &item.components {
            product.components.extend(components.clone());
        }
        let lore = item.lore.clone().unwrap_or(lore);
        items.push(spec_lore(product, lore));
    }
    items.push(spec(
        nav_row + 2,
        "minecraft:gray_stained_glass_pane",
        " ",
        "gray",
        1,
    ));
    items.push(spec(
        nav_row + 3,
        "minecraft:stone_button",
        &format!("Página anterior ({}/{})", page + 1, pages),
        "white",
        1,
    ));
    items.push(spec(nav_row + 4, "minecraft:arrow", "Voltar", "white", 1));
    items.push(spec(
        nav_row + 5,
        "minecraft:oak_button",
        &format!("Próxima página ({}/{})", page + 1, pages),
        "white",
        1,
    ));
    items.push(spec(
        nav_row + 6,
        "minecraft:gray_stained_glass_pane",
        " ",
        "gray",
        1,
    ));
    let title = format!("Loja: {}", cat.title);
    state.views.retain(|(uuid, _), _| *uuid != player);
    if let Some(window) = show_menu(host, player, &title, rows, items) {
        state.views.insert(
            (player, window),
            View::Items {
                category,
                page,
                rows,
                slots: mapping,
            },
        );
    }
}

fn open_detail(
    state: &mut State,
    host: &HostApiV3,
    player: u128,
    category: usize,
    page: usize,
    item_index: usize,
    quantity: u32,
) {
    let Some(item) = state
        .categories
        .get(category)
        .and_then(|c| c.items.get(item_index))
    else {
        return;
    };
    let quantity = quantity.clamp(1, 9999);
    let material = item.material.to_ascii_lowercase();
    let currency = if state.categories[category].id == "loja_vip" {
        "Créditos"
    } else {
        "moedas"
    };
    let currency_id = if state.categories[category].id == "loja_vip" {
        state.config.currency.vip.as_str()
    } else {
        state.config.currency.normal.as_str()
    };
    let mut quantity_lore = vec![
        format!(
            "§7Comprar: §6{} {currency}",
            price(item.buy * f64::from(quantity))
        ),
        format!(
            "§7Vender: §6{} {currency}",
            price(item.sell * f64::from(quantity))
        ),
        format!("§7Itens por pacote: §f{}", item.amount),
    ];
    if let Some(balance) = balance_text(host, player, currency_id) {
        quantity_lore.push(format!("§7Saldo: §6{balance} {currency}"));
    }
    quantity_lore.extend([
        "§8----------------".into(),
        "§7Ajuste a quantidade nos botões".into(),
        format!("§aClique para comprar {quantity}x"),
        format!("§cClique para vender {quantity}x"),
    ]);
    let mut product_icon = spec(
        4,
        &format!("minecraft:{material}"),
        &item.display_name,
        "yellow",
        item.amount.clamp(1, 64),
    );
    if let Some(model) = &item.item_model {
        product_icon.components.insert(
            "minecraft:item_model".into(),
            serde_json::Value::String(model.clone()),
        );
    }
    if let Some(components) = &item.components {
        product_icon.components.extend(components.clone());
    }
    let mut items = vec![
        spec(0, "minecraft:gray_stained_glass_pane", " ", "dark_gray", 1),
        product_icon,
        spec(10, "minecraft:red_stained_glass_pane", "−64", "red", 1),
        spec(11, "minecraft:orange_stained_glass_pane", "−16", "gold", 1),
        spec(
            12,
            "minecraft:pink_stained_glass_pane",
            "−1",
            "light_purple",
            1,
        ),
        spec_lore(
            spec(
                13,
                "minecraft:paper",
                &format!("Quantidade: {quantity}"),
                "yellow",
                1,
            ),
            quantity_lore,
        ),
        spec(
            14,
            "minecraft:light_blue_stained_glass_pane",
            "+1",
            "aqua",
            1,
        ),
        spec(15, "minecraft:cyan_stained_glass_pane", "+16", "aqua", 1),
        spec(16, "minecraft:lime_stained_glass_pane", "+64", "green", 1),
        spec_lore(
            spec(20, "minecraft:emerald", "Comprar", "green", 1),
            vec!["§7Clique para confirmar a compra".into()],
        ),
        spec(22, "minecraft:arrow", "Voltar", "white", 1),
        spec_lore(
            spec(24, "minecraft:redstone", "Vender", "red", 1),
            vec!["§7Clique para vender do inventário".into()],
        ),
    ];
    for slot in [1, 2, 3, 5, 6, 7, 8, 9, 17, 18, 19, 21, 23, 25, 26] {
        items.push(spec(
            slot,
            "minecraft:gray_stained_glass_pane",
            " ",
            "dark_gray",
            1,
        ));
    }
    let title = format!("Loja: {}", item.display_name);
    state.views.retain(|(uuid, _), _| *uuid != player);
    if let Some(window) = show_menu(host, player, &title, 3, items) {
        state.views.insert(
            (player, window),
            View::Detail {
                category,
                page,
                item: item_index,
                quantity,
            },
        );
    }
}

fn price(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .replace('.', ",")
    }
}

unsafe extern "C-unwind" fn create() -> *mut c_void {
    let mut plugin_state = state().lock().unwrap_or_else(|error| error.into_inner());
    ensure_loaded(&mut plugin_state);
    Box::into_raw(Box::new(())) as *mut c_void
}
unsafe extern "C-unwind" fn destroy(state: *mut c_void) {
    if !state.is_null() {
        unsafe { drop(Box::from_raw(state.cast::<()>())) };
    }
}

unsafe extern "C-unwind" fn register_commands(_: *mut c_void, host: *const HostApiV3) {
    if host.is_null() {
        return;
    }
    let host = unsafe { &*host };
    let _ = call_json(
        host,
        0,
        serde_json::json!({
            "operation": operation::COMMAND_REGISTER,
            "data": {"usage": "loja", "default_level": 0},
        }),
    );
    let _ = call_json(
        host,
        0,
        serde_json::json!({
            "operation": operation::COMMAND_REGISTER,
            "data": {
                "usage": "loja recarregar",
                "default_level": 2,
                "permission": "auraessentials.loja.admin",
            },
        }),
    );
}

unsafe extern "C-unwind" fn command(
    _: *mut c_void,
    host: *const HostApiV3,
    high: u64,
    low: u64,
    line: *const u8,
    len: usize,
) -> u8 {
    if host.is_null() || line.is_null() || len > 512 {
        return 0;
    }
    let line = unsafe { std::slice::from_raw_parts(line, len) };
    let Ok(line) = std::str::from_utf8(line) else {
        return 0;
    };
    let line = line.trim().trim_start_matches('/');
    if !matches!(line, "loja" | "loja recarregar") {
        return 0;
    }
    let host = unsafe { &*host };
    let player = (u128::from(high) << 64) | u128::from(low);
    let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
    if line == "loja recarregar" {
        // Permission is enforced by Horizon's registered operator-level command.
        match (load_shop(), load_config()) {
            (Ok(categories), Ok(config)) => {
                state.categories = categories;
                state.config = config;
                state.loaded = true;
                state.views.clear();
                message(host, player, "Configuração e itens da loja recarregados.");
            }
            (Err(error), _) | (_, Err(error)) => {
                message(
                    host,
                    player,
                    &format!("Não foi possível recarregar a loja: {error}"),
                );
            }
        }
    } else {
        ensure_loaded(&mut state);
        open_categories(&mut state, host, player);
    }
    1
}

unsafe extern "C-unwind" fn menu_click(
    _: *mut c_void,
    host: *const HostApiV3,
    high: u64,
    low: u64,
    window: i32,
    slot: i32,
    _button: i32,
    mode: i32,
) {
    if host.is_null() || !matches!(mode, 0 | 1) {
        return;
    }
    let host = unsafe { &*host };
    let player = (u128::from(high) << 64) | u128::from(low);
    let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some(view) = state.views.remove(&(player, window)) else {
        return;
    };
    match view {
        View::Categories(mapping) => {
            if let Some(category) = mapping.get(&slot).copied() {
                open_items(&mut state, host, player, category, 0);
            }
        }
        View::Items {
            category,
            page,
            rows,
            slots,
        } => {
            let nav = usize::from(rows) * 9 - 9;
            if slot == (nav + 4) as i32 {
                open_categories(&mut state, host, player);
            } else if slot == (nav + 3) as i32 && page > 0 {
                open_items(&mut state, host, player, category, page - 1);
            } else if slot == (nav + 5) as i32 {
                open_items(&mut state, host, player, category, page + 1);
            } else if let Some(item) = slots.get(&slot).copied() {
                if state.categories[category].id == "loja_vip" {
                    let product = state.categories[category].items[item].clone();
                    let currency = state.config.currency.vip.clone();
                    let result = transact_shop_item(host, player, &product, &currency, 1, true);
                    trade_message(host, player, result, true);
                    open_items(&mut state, host, player, category, page);
                } else {
                    open_detail(&mut state, host, player, category, page, item, 1);
                }
            }
        }
        View::Detail {
            category,
            page,
            item,
            quantity,
        } => match slot {
            22 => open_items(&mut state, host, player, category, page),
            10 | 11 | 12 | 14 | 15 | 16 => {
                let delta = match slot {
                    10 => -64,
                    11 => -16,
                    12 => -1,
                    14 => 1,
                    15 => 16,
                    _ => 64,
                };
                let next = (quantity as i64 + delta).clamp(1, 9999) as u32;
                open_detail(&mut state, host, player, category, page, item, next);
            }
            20 => {
                let product = state.categories[category].items[item].clone();
                let currency = if state.categories[category].id == "loja_vip" {
                    state.config.currency.vip.clone()
                } else {
                    state.config.currency.normal.clone()
                };
                let result = transact_shop_item(host, player, &product, &currency, quantity, true);
                trade_message(host, player, result, true);
                state.views.insert(
                    (player, window),
                    View::Detail {
                        category,
                        page,
                        item,
                        quantity,
                    },
                );
            }
            24 => {
                let product = state.categories[category].items[item].clone();
                let currency = if state.categories[category].id == "loja_vip" {
                    state.config.currency.vip.clone()
                } else {
                    state.config.currency.normal.clone()
                };
                let result = transact_shop_item(host, player, &product, &currency, quantity, false);
                trade_message(host, player, result, false);
                state.views.insert(
                    (player, window),
                    View::Detail {
                        category,
                        page,
                        item,
                        quantity,
                    },
                );
            }
            _ => {
                state.views.insert(
                    (player, window),
                    View::Detail {
                        category,
                        page,
                        item,
                        quantity,
                    },
                );
            }
        },
    }
}

unsafe extern "C-unwind" fn event(
    _: *mut c_void,
    _: *const HostApiV3,
    kind: u32,
    payload: *const u8,
    payload_len: usize,
) -> u8 {
    if payload.is_null() || payload_len > 65_536 {
        return 0;
    }
    let payload = unsafe { std::slice::from_raw_parts(payload, payload_len) };
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return 0;
    };
    let player = payload
        .get("player")
        .and_then(serde_json::Value::as_str)
        .and_then(|uuid| u128::from_str_radix(uuid, 16).ok())
        .unwrap_or(0);
    let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
    match kind {
        event::PLAYER_QUIT => {
            state.views.retain(|(uuid, _), _| *uuid != player);
        }
        event::MENU_CLOSED => {
            if let Some(window) = payload
                .get("window_id")
                .and_then(serde_json::Value::as_i64)
                .and_then(|window| i32::try_from(window).ok())
            {
                state.views.remove(&(player, window));
            }
        }
        _ => {}
    }
    0
}

static DESCRIPTOR: PluginV3 = PluginV3 {
    abi_version: ABI_VERSION_V3,
    name: NAME.as_ptr(),
    name_len: NAME.len(),
    create: Some(create),
    destroy: Some(destroy),
    register_commands: Some(register_commands),
    command: Some(command),
    menu_click: Some(menu_click),
    event: Some(event),
    permission: None,
};

#[unsafe(no_mangle)]
pub extern "C-unwind" fn horizon_server_plugin_v3() -> *const PluginV3 {
    &DESCRIPTOR
}
