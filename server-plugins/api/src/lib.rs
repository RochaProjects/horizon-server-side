//! Versioned C ABI shared by the dedicated-server plugin loader and plugins.
//! This surface deliberately contains no Horizon or Rust-owned game types.

use std::ffi::c_void;

pub const ABI_VERSION: u32 = 2;
pub const ABI_VERSION_V3: u32 = 3;
pub type ByteSlice = *const u8;

/// Stable numeric identifiers for generic server events in ABI v2.
pub mod event {
    pub const PLAYER_JOIN: u32 = 1;
    pub const PLAYER_QUIT: u32 = 2;
    pub const BLOCK_BREAK: u32 = 3;
    pub const MENU_CLOSED: u32 = 4;
    pub const PLAYER_CHANGED_DIMENSION: u32 = 5;
    pub const PLAYER_DEATH: u32 = 6;
    pub const BLOCK_PLACE: u32 = 7;
    pub const CHAT: u32 = 8;
    pub const RESOURCE_PACK_STATUS: u32 = 9;
    pub const PLAYER_TELEPORT: u32 = 10;
}

/// Stable operation names accepted by `HostApiV3::call_json`.
pub mod operation {
    pub const COMMAND_REGISTER: &str = "command.register";
    pub const CHAT_SEND: &str = "chat.send";
    pub const MENU_OPEN: &str = "menu.open";
    pub const PLAYER_GET: &str = "player.get";
    pub const PLAYER_TELEPORT: &str = "player.teleport";
    pub const PLAYER_GAME_MODE_SET: &str = "player.game_mode.set";
    pub const RESOURCE_PACK_SEND: &str = "resource_pack.send";
    pub const RESOURCE_PACK_SEND_BATCH: &str = "resource_pack.send_batch";
    pub const RESOURCE_PACK_REMOVE: &str = "resource_pack.remove";
    pub const WORLD_DIMENSIONS_LIST: &str = "world.dimensions.list";
    pub const WORLD_GAMERULE_GET: &str = "world.gamerule.get";
    pub const WORLD_GAMERULE_SET: &str = "world.gamerule.set";
    pub const WORLD_CUSTOM_GAMERULE_GET: &str = "world.custom_gamerule.get";
    pub const WORLD_CUSTOM_GAMERULE_SET: &str = "world.custom_gamerule.set";
    pub const WORLD_CUSTOM_GAMERULE_CLEAR: &str = "world.custom_gamerule.clear";
    pub const WORLD_DIMENSION_UNLOAD: &str = "world.dimension.unload";
    pub const SCOREBOARD_SIDEBAR_SET: &str = "scoreboard.sidebar.set";
    pub const SCOREBOARD_SIDEBAR_CLEAR: &str = "scoreboard.sidebar.clear";
    pub const PLUGIN_STORAGE_GET: &str = "plugin.storage.get";
    pub const PLUGIN_STORAGE_SET: &str = "plugin.storage.set";
    pub const PLUGIN_STORAGE_DELETE: &str = "plugin.storage.delete";
    pub const PLUGIN_STORAGE_LIST: &str = "plugin.storage.list";
    pub const ENTITY_TEXT_DISPLAY_CREATE: &str = "entity.text_display.create";
    pub const ENTITY_MOVE: &str = "entity.move";
    pub const ENTITY_REMOVE: &str = "entity.remove";
}

/// Return values from `HostApiV2::get_balance` and `HostApiV2::transact`.
pub mod result {
    pub const OK: i32 = 1;
    pub const INSUFFICIENT_BALANCE: i32 = 2;
    pub const ITEMS_REJECTED: i32 = 3;
    pub const STORAGE_ERROR: i32 = 4;
    pub const INVALID_REQUEST: i32 = 5;
    pub const BUFFER_TOO_SMALL: i32 = 6;
}

#[repr(C)]
pub struct ServerEventV2 {
    pub kind: u32,
    pub player_high: u64,
    pub player_low: u64,
    pub name: ByteSlice,
    pub name_len: usize,
    pub dimension: ByteSlice,
    pub dimension_len: usize,
    /// Meaningful only for `event::MENU_CLOSED`.
    pub window_id: i32,
    /// Meaningful only for `event::BLOCK_BREAK`; zero for join/quit.
    pub position: [i32; 3],
    /// The block-state registry id on `BLOCK_BREAK`; zero otherwise.
    pub data_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MenuItemV2 {
    pub slot: u32,
    pub item: ByteSlice,
    pub item_len: usize,
    /// Display name; Minecraft legacy `§` formatting codes are accepted.
    pub name: ByteSlice,
    pub name_len: usize,
    /// Minecraft text color name, e.g. "gold" or "white"; empty keeps the component default.
    pub color: ByteSlice,
    pub color_len: usize,
    /// Newline-separated lines; each accepts Minecraft legacy `§` formatting codes.
    pub lore: ByteSlice,
    pub lore_len: usize,
    pub count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ItemDeltaV2 {
    pub item: ByteSlice,
    pub item_len: usize,
    /// Positive adds plain items; negative removes them. Zero is invalid.
    pub delta: i32,
}

#[repr(C)]
pub struct HostApiV2 {
    pub abi_version: u32,
    pub context: *mut c_void,
    pub register_command: Option<unsafe extern "C-unwind" fn(*mut c_void, ByteSlice, usize) -> i32>,
    pub send_message:
        Option<unsafe extern "C-unwind" fn(*mut c_void, u64, u64, ByteSlice, usize) -> i32>,
    pub open_menu: Option<
        unsafe extern "C-unwind" fn(
            *mut c_void,
            u64,
            u64,
            ByteSlice,
            usize,
            u8,
            *const MenuItemV2,
            usize,
        ) -> i32,
    >,
    pub get_balance: Option<
        unsafe extern "C-unwind" fn(*mut c_void, u64, u64, ByteSlice, usize, *mut i64) -> i32,
    >,
    /// Apply a balance delta and item deltas as one server-thread transaction.
    /// Pass an empty currency ID and a zero balance delta for an item-only
    /// transaction; the inventory update still runs atomically on the server thread.
    /// Returns `result::OK`, `INSUFFICIENT_BALANCE`, `ITEMS_REJECTED`,
    /// `STORAGE_ERROR`, or `INVALID_REQUEST`.
    pub transact: Option<
        unsafe extern "C-unwind" fn(
            *mut c_void,
            u64,
            u64,
            ByteSlice,
            usize,
            i64,
            *const ItemDeltaV2,
            usize,
        ) -> i32,
    >,
}

#[repr(C)]
pub struct PluginV2 {
    pub abi_version: u32,
    pub name: ByteSlice,
    pub name_len: usize,
    pub create: Option<unsafe extern "C-unwind" fn() -> *mut c_void>,
    pub destroy: Option<unsafe extern "C-unwind" fn(*mut c_void)>,
    pub register_commands: Option<unsafe extern "C-unwind" fn(*mut c_void, *const HostApiV2)>,
    /// Return nonzero when the plugin handled this command.
    pub command: Option<
        unsafe extern "C-unwind" fn(
            *mut c_void,
            *const HostApiV2,
            u64,
            u64,
            ByteSlice,
            usize,
        ) -> u8,
    >,
    pub menu_click: Option<
        unsafe extern "C-unwind" fn(*mut c_void, *const HostApiV2, u64, u64, i32, i32, i32, i32),
    >,
    /// Return nonzero to cancel a cancellable event such as `BLOCK_BREAK`.
    pub event: Option<
        unsafe extern "C-unwind" fn(*mut c_void, *const HostApiV2, *const ServerEventV2) -> u8,
    >,
}

// Plugin descriptors are immutable static tables; pointer fields reference
// static plugin-owned metadata and are read only while its library is loaded.
unsafe impl Sync for PluginV2 {}

/// Every dynamic plugin exports this symbol and returns a pointer to a static [`PluginV2`].
pub type PluginEntryV2 = unsafe extern "C-unwind" fn() -> *const PluginV2;

/// ABI v3 keeps the function table fixed. New generic server operations are
/// added as named JSON requests through `HostApiV3::call_json`, so plugin
/// features do not require another ABI layout change.
#[repr(C)]
pub struct HostApiV3 {
    pub abi_version: u32,
    pub context: *mut c_void,
    /// ABI v2's stable generic command/message/menu/economy functions remain
    /// available to v3 plugins.
    pub common: HostApiV2,
    /// Request and response are UTF-8 JSON. `response_len` is always written
    /// when non-null; if the response does not fit, it receives the required
    /// size and the function returns `result::BUFFER_TOO_SMALL`.
    pub call_json: Option<
        unsafe extern "C-unwind" fn(
            *mut c_void,
            u64,
            u64,
            ByteSlice,
            usize,
            *mut u8,
            usize,
            *mut usize,
        ) -> i32,
    >,
}

/// ABI v3 event and command callbacks receive JSON values, leaving event
/// payloads extensible without changing the C layout.
#[repr(C)]
pub struct PluginV3 {
    pub abi_version: u32,
    pub name: ByteSlice,
    pub name_len: usize,
    pub create: Option<unsafe extern "C-unwind" fn() -> *mut c_void>,
    pub destroy: Option<unsafe extern "C-unwind" fn(*mut c_void)>,
    pub register_commands: Option<unsafe extern "C-unwind" fn(*mut c_void, *const HostApiV3)>,
    /// `command` is the raw UTF-8 command line without its leading slash.
    /// Return nonzero when handled.
    pub command: Option<
        unsafe extern "C-unwind" fn(
            *mut c_void,
            *const HostApiV3,
            u64,
            u64,
            ByteSlice,
            usize,
        ) -> u8,
    >,
    pub menu_click: Option<
        unsafe extern "C-unwind" fn(*mut c_void, *const HostApiV3, u64, u64, i32, i32, i32, i32),
    >,
    /// The event payload is a JSON object. Return nonzero to cancel a
    /// cancellable event.
    pub event: Option<
        unsafe extern "C-unwind" fn(*mut c_void, *const HostApiV3, u32, ByteSlice, usize) -> u8,
    >,
    /// Return -1 to deny, 0 to defer to the operator-level default, or 1 to
    /// grant the permission node.
    pub permission:
        Option<unsafe extern "C-unwind" fn(*mut c_void, u64, u64, ByteSlice, usize, u8) -> i8>,
}

unsafe impl Sync for PluginV3 {}

pub type PluginEntryV3 = unsafe extern "C-unwind" fn() -> *const PluginV3;
