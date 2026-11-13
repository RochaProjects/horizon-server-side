//! Loja em português baseada na lista de itens e no fluxo de telas do AuraEssentials Java.

use horizon_server_plugin_api::{ABI_VERSION, HostApiV1, MenuItemV1, PluginV1, ServerEventV1};
use serde::Deserialize;
use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Mutex, OnceLock},
};

const NAME: &[u8] = b"Aura Loja";
const ITEMS_JSON: &str = include_str!("../shop.json");

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShopItem {
    display_name: String,
    material: String,
    icon: Option<String>,
    buy: f64,
    sell: f64,
}

#[derive(Clone)]
struct Category {
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    items: Vec<ShopItem>,
}

#[derive(Default)]
struct State {
    categories: Vec<Category>,
    views: HashMap<(u128, i32), View>,
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
    STATE.get_or_init(|| {
        Mutex::new(State {
            categories: categories(),
            views: HashMap::new(),
        })
    })
}

fn categories() -> Vec<Category> {
    let data: HashMap<String, Vec<ShopItem>> = serde_json::from_str(ITEMS_JSON).unwrap_or_default();
    let meta = [
        ("blocos", "Blocos", "bricks"),
        ("minerios", "Minérios", "diamond"),
        ("drops_mobs", "Drops de Mobs", "blaze_rod"),
        ("comida", "Comida", "cooked_beef"),
        ("plantacao", "Plantação", "wheat"),
        ("livros_encantados", "Livros Encantados", "enchanted_book"),
        ("madeiras", "Madeiras", "oak_log"),
        ("loja_vip", "Loja VIP", "nether_star"),
    ];
    meta.into_iter()
        .map(|(id, title, icon)| {
            let items = data.get(id).cloned().unwrap_or_default();
            Category {
                id,
                title,
                icon,
                items,
            }
        })
        .collect()
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
    count: u32,
}

fn spec(slot: usize, item: &str, name: &str, color: &str, count: u32) -> ItemSpec {
    ItemSpec {
        slot,
        item: item.into(),
        name: name.into(),
        color: color.into(),
        lore: Vec::new(),
        count,
    }
}

fn spec_lore(mut item: ItemSpec, lore: Vec<String>) -> ItemSpec {
    item.lore = lore;
    item
}

fn show_menu(
    host: &HostApiV1,
    player: u128,
    title: &str,
    rows: u8,
    items: Vec<ItemSpec>,
) -> Option<i32> {
    let open = host.open_menu?;
    let (high, low) = ((player >> 64) as u64, player as u64);
    let lore_text: Vec<_> = items.iter().map(|it| it.lore.join("\n")).collect();
    let descriptors: Vec<_> = items
        .iter()
        .zip(&lore_text)
        .map(|(it, lore_text)| {
            let (item, item_len) = bytes(&it.item);
            let (name, name_len) = bytes(&it.name);
            let (color, color_len) = bytes(&it.color);
            let (lore, lore_len) = bytes(lore_text);
            MenuItemV1 {
                slot: it.slot as u32,
                item,
                item_len,
                name,
                name_len,
                color,
                color_len,
                lore,
                lore_len,
                count: it.count,
            }
        })
        .collect();
    let (title_ptr, title_len) = bytes(title);
    // SAFETY: all slices and descriptor pointers stay alive for this synchronous host call.
    let window = unsafe {
        open(
            host.context,
            high,
            low,
            title_ptr,
            title_len,
            rows,
            descriptors.as_ptr(),
            descriptors.len(),
        )
    };
    (window >= 0).then_some(window)
}

fn message(host: &HostApiV1, player: u128, text: &str) {
    if let Some(send) = host.send_message {
        let (high, low) = ((player >> 64) as u64, player as u64);
        // SAFETY: text remains valid for this synchronous call.
        unsafe { send(host.context, high, low, text.as_ptr(), text.len()) };
    }
}

fn open_categories(state: &mut State, host: &HostApiV1, player: u128) {
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
    if let Some(window) = show_menu(host, player, "Loja", 3, items) {
        state
            .views
            .insert((player, window), View::Categories(mapping));
    } else {
        message(host, player, "Não foi possível abrir a loja.");
    }
}

fn open_items(
    state: &mut State,
    host: &HostApiV1,
    player: u128,
    category: usize,
    requested_page: usize,
) {
    let Some(cat) = state.categories.get(category) else {
        return;
    };
    let item_count = cat.items.len();
    let item_rows = ((item_count.saturating_sub(4).max(1) + 8) / 9).max(1);
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
    let mut add = |slot, id: &str, name: &str, color: &str, count| {
        items.push(spec(slot, id, name, color, count))
    };
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
            1,
        );
        items.push(spec_lore(product, lore));
    }
    add(
        nav_row + 2,
        "minecraft:gray_stained_glass_pane",
        " ",
        "gray",
        1,
    );
    add(
        nav_row + 3,
        "minecraft:stone_button",
        &format!("Página anterior ({}/{})", page + 1, pages),
        "white",
        1,
    );
    add(nav_row + 4, "minecraft:arrow", "Voltar", "white", 1);
    add(
        nav_row + 5,
        "minecraft:oak_button",
        &format!("Próxima página ({}/{})", page + 1, pages),
        "white",
        1,
    );
    add(
        nav_row + 6,
        "minecraft:gray_stained_glass_pane",
        " ",
        "gray",
        1,
    );
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
    host: &HostApiV1,
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
    let mut items = vec![
        spec(0, "minecraft:gray_stained_glass_pane", " ", "dark_gray", 1),
        spec(
            4,
            &format!("minecraft:{material}"),
            &item.display_name,
            "yellow",
            1,
        ),
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
            vec![
                format!(
                    "§7Comprar: §6{} {currency}",
                    price(item.buy * f64::from(quantity))
                ),
                format!(
                    "§7Vender: §6{} {currency}",
                    price(item.sell * f64::from(quantity))
                ),
                "§8----------------".into(),
                "§7Ajuste a quantidade nos botões".into(),
                format!("§aClique para comprar {quantity}x"),
                format!("§cClique para vender {quantity}x"),
            ],
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
            .into()
    }
}

unsafe extern "C" fn create() -> *mut c_void {
    Box::into_raw(Box::new(())) as *mut c_void
}
unsafe extern "C" fn destroy(state: *mut c_void) {
    if !state.is_null() {
        unsafe { drop(Box::from_raw(state.cast::<()>())) };
    }
}

unsafe extern "C" fn register_commands(_: *mut c_void, host: *const HostApiV1) {
    if host.is_null() {
        return;
    }
    let host = unsafe { &*host };
    if let Some(register) = host.register_command {
        let usage = b"loja";
        unsafe { register(host.context, usage.as_ptr(), usage.len()) };
    }
}

unsafe extern "C" fn command(
    _: *mut c_void,
    host: *const HostApiV1,
    high: u64,
    low: u64,
    line: *const u8,
    len: usize,
) -> u8 {
    if host.is_null() || line.is_null() || len > 512 {
        return 0;
    }
    let line = unsafe { std::slice::from_raw_parts(line, len) };
    if std::str::from_utf8(line)
        .ok()
        .is_none_or(|s| !matches!(s.trim(), "loja" | "/loja"))
    {
        return 0;
    }
    let host = unsafe { &*host };
    let player = (u128::from(high) << 64) | u128::from(low);
    let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
    open_categories(&mut state, host, player);
    1
}

unsafe extern "C" fn menu_click(
    _: *mut c_void,
    host: *const HostApiV1,
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
                    message(
                        host,
                        player,
                        "A compra VIP ainda não está conectada ao saldo de Créditos. Nenhum saldo foi alterado.",
                    );
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
                message(
                    host,
                    player,
                    "A compra ainda não está conectada ao saldo. Nenhum item ou saldo foi alterado.",
                );
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
                message(
                    host,
                    player,
                    "A venda ainda não está conectada ao saldo. Nenhum item ou saldo foi alterado.",
                );
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

unsafe extern "C" fn event(_: *mut c_void, _: *const HostApiV1, event: *const ServerEventV1) -> u8 {
    if event.is_null() {
        return 0;
    }
    let event = unsafe { &*event };
    let player = (u128::from(event.player_high) << 64) | u128::from(event.player_low);
    let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
    match event.kind {
        horizon_server_plugin_api::event::PLAYER_QUIT => {
            state.views.retain(|(uuid, _), _| *uuid != player);
        }
        horizon_server_plugin_api::event::MENU_CLOSED => {
            state.views.remove(&(player, event.window_id));
        }
        _ => {}
    }
    0
}

static DESCRIPTOR: PluginV1 = PluginV1 {
    abi_version: ABI_VERSION,
    name: NAME.as_ptr(),
    name_len: NAME.len(),
    create: Some(create),
    destroy: Some(destroy),
    register_commands: Some(register_commands),
    command: Some(command),
    menu_click: Some(menu_click),
    event: Some(event),
};

#[unsafe(no_mangle)]
pub extern "C" fn horizon_server_plugin_v1() -> *const PluginV1 {
    &DESCRIPTOR
}
