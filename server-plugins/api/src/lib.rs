//! Versioned C ABI shared by the dedicated-server plugin loader and plugins.
//! This surface deliberately contains no Horizon or Rust-owned game types.

use std::ffi::c_void;

pub const ABI_VERSION: u32 = 1;
pub type ByteSlice = *const u8;

/// Stable numeric identifiers for generic server events in ABI v1.
pub mod event {
    pub const PLAYER_JOIN: u32 = 1;
    pub const PLAYER_QUIT: u32 = 2;
    pub const BLOCK_BREAK: u32 = 3;
    pub const MENU_CLOSED: u32 = 4;
}

#[repr(C)]
pub struct ServerEventV1 {
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
pub struct MenuItemV1 {
    pub slot: u32,
    pub item: ByteSlice,
    pub item_len: usize,
    pub name: ByteSlice,
    pub name_len: usize,
    /// Minecraft text color name, e.g. "gold" or "white"; empty keeps default.
    pub color: ByteSlice,
    pub color_len: usize,
    /// Newline-separated legacy-formatted lines for the vanilla `minecraft:lore` component.
    pub lore: ByteSlice,
    pub lore_len: usize,
    pub count: u32,
}

#[repr(C)]
pub struct HostApiV1 {
    pub abi_version: u32,
    pub context: *mut c_void,
    pub register_command: Option<unsafe extern "C" fn(*mut c_void, ByteSlice, usize) -> i32>,
    pub send_message: Option<unsafe extern "C" fn(*mut c_void, u64, u64, ByteSlice, usize) -> i32>,
    pub open_menu: Option<
        unsafe extern "C" fn(
            *mut c_void,
            u64,
            u64,
            ByteSlice,
            usize,
            u8,
            *const MenuItemV1,
            usize,
        ) -> i32,
    >,
}

#[repr(C)]
pub struct PluginV1 {
    pub abi_version: u32,
    pub name: ByteSlice,
    pub name_len: usize,
    pub create: Option<unsafe extern "C" fn() -> *mut c_void>,
    pub destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    pub register_commands: Option<unsafe extern "C" fn(*mut c_void, *const HostApiV1)>,
    /// Return nonzero when the plugin handled this command.
    pub command: Option<
        unsafe extern "C" fn(*mut c_void, *const HostApiV1, u64, u64, ByteSlice, usize) -> u8,
    >,
    pub menu_click:
        Option<unsafe extern "C" fn(*mut c_void, *const HostApiV1, u64, u64, i32, i32, i32, i32)>,
    /// Return nonzero to cancel a cancellable event such as `BLOCK_BREAK`.
    pub event:
        Option<unsafe extern "C" fn(*mut c_void, *const HostApiV1, *const ServerEventV1) -> u8>,
}

// Plugin descriptors are immutable static tables; pointer fields reference
// static plugin-owned metadata and are read only while its library is loaded.
unsafe impl Sync for PluginV1 {}

/// Every dynamic plugin exports this symbol and returns a pointer to a static [`PluginV1`].
pub type PluginEntryV1 = unsafe extern "C" fn() -> *const PluginV1;
