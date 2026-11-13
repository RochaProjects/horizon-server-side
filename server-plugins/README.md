# Criar plugins de servidor Horizon (ABI v2)

Plugins Linux são bibliotecas Rust `.so` carregadas no início do servidor a partir de `plugins/`. O host valida a ABI, chama os callbacks no thread do servidor e os mantém carregados até o processo terminar. Para atualizar um plugin, substitua a biblioteca e reinicie o servidor.

O contrato público está em `api/`; o carregador e a fronteira FFI ficam em `loader/`. Não há crate `horizon-mod-sdk` nem trait `ServerMod` neste checkout. Os trait mods documentados em `docs/native-mods.md` são compilados dentro de um executável; os plugins deste SDK são compilados separadamente.

## 1. Crie o crate

Dentro de `server-plugins/`, crie `meu-plugin/Cargo.toml`:

```toml
[package]
name = "meu-plugin"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-server-plugin-api = { path = "../api" }
```

Adicione `"server-plugins/meu-plugin"` aos membros do workspace em `Cargo.toml`. Para um projeto fora deste repositório, publique ou espelhe o crate `horizon-server-plugin-api` e troque a dependência de caminho pela versão correspondente.

## 2. Implemente a entrada e os callbacks

Este exemplo registra `/ola` e envia uma mensagem. Os callbacks usam `C-unwind` para que o host possa capturar panics Rust e desativar o plugin que falhou.

```rust
use horizon_server_plugin_api::{ABI_VERSION, HostApiV2, PluginV2};
use std::ffi::c_void;

static NAME: &[u8] = b"Meu plugin";

unsafe extern "C-unwind" fn create() -> *mut c_void {
    Box::into_raw(Box::new(())) as *mut c_void
}

unsafe extern "C-unwind" fn destroy(state: *mut c_void) {
    if !state.is_null() {
        unsafe { drop(Box::from_raw(state.cast::<()>())) };
    }
}

unsafe extern "C-unwind" fn register_commands(_: *mut c_void, host: *const HostApiV2) {
    let Some(host) = (unsafe { host.as_ref() }) else { return };
    if let Some(register) = host.register_command {
        let command = b"ola";
        unsafe { register(host.context, command.as_ptr(), command.len()) };
    }
}

unsafe extern "C-unwind" fn command(
    _: *mut c_void,
    host: *const HostApiV2,
    player_high: u64,
    player_low: u64,
    _: *const u8,
    _: usize,
) -> u8 {
    let Some(host) = (unsafe { host.as_ref() }) else { return 0 };
    if let Some(send) = host.send_message {
        let text = "Olá! Plugin carregado.";
        unsafe {
            send(
                host.context,
                player_high,
                player_low,
                text.as_ptr(),
                text.len(),
            );
        }
    }
    1
}

unsafe extern "C-unwind" fn menu_click(
    _: *mut c_void, _: *const HostApiV2, _: u64, _: u64, _: i32, _: i32, _: i32, _: i32,
) {}

static PLUGIN: PluginV2 = PluginV2 {
    abi_version: ABI_VERSION,
    name: NAME.as_ptr(),
    name_len: NAME.len(),
    create: Some(create),
    destroy: Some(destroy),
    register_commands: Some(register_commands),
    command: Some(command),
    menu_click: Some(menu_click),
    event: None,
};

#[unsafe(no_mangle)]
pub extern "C-unwind" fn horizon_server_plugin_v2() -> *const PluginV2 {
    &PLUGIN
}
```

The descriptor and its name must remain static while the library is loaded. Callback strings, menu item arrays, event payloads and the `HostApiV2` pointer are borrowed only for the callback; copy data a plugin needs to keep. Return nonzero from `command` only when it handled the command. The current command registration helper exposes commands to all players; do not use it for privileged actions.

## 3. Build and install

From the repository root:

```sh
cargo build --release -p meu-plugin
mkdir -p plugins
cp target/release/libmeu_plugin.so plugins/
```

The server and plugin must target the same OS, architecture and compatible native runtime. Linux is the packaged server target today. The loader also recognizes `.dylib` and `.dll` on their respective platforms.

## API v2

`HostApiV2` provides generic command registration, player messages, vanilla chest menus, menu clicks, player lifecycle and block-break events, balance reads, and item-plus-balance transactions. It has no Aura-specific methods.

Transactions name a shared currency ID (for example `aura:coins`), a signed balance delta, and signed plain-item deltas. The host checks the balance and inventory, commits the balance file, then applies inventory changes on the server thread; it rolls the balance back if the inventory update fails. A transaction needs the player online and outside a player's active command turn, as menu clicks are. This is atomic during normal server processing, but the balance file and Minecraft's world save are separate files, so a process or machine crash at the precise moment of a purchase is not a cross-file atomic commit. Balance values are signed 64-bit integer units; the plugin that owns a currency defines its precision. `Aura Loja` uses hundredths, so `100` means `1,00` moeda or Crédito.

Balances are saved atomically in `plugins/balances.json`, keyed by currency ID and player UUID. To seed a test account, stop the server and edit the file, using the player's 32-digit UUID without hyphens:

```json
{
  "version": 1,
  "accounts": [
    { "currency": "aura:coins", "player": "00000000000000000000000000000000", "units": 100000 },
    { "currency": "aura:credits", "player": "00000000000000000000000000000000", "units": 10000 }
  ]
}
```

Replace the example UUID with the real player UUID. The IDs are global so a separate economy plugin can share a currency intentionally. Native plugins are trusted code: any loaded plugin can call the generic transaction API for any currency ID. Treat plugin files as executable server code and restrict who can install them.

The host catches Rust panics at plugin callbacks and disables a plugin that panics. This does not contain memory corruption, segmentation faults, explicit aborts, or process-wide resource exhaustion; native plugins are not sandboxed. `C-unwind` is required for panic capture across the ABI.

## Aura Loja

`aura-loja/` is the first separate plugin. `/loja` reproduces the eight AuraEssentials categories, their 66 products, and the category, product and detail screens in Portuguese. Buying and selling use the generic transaction API. `aura:coins` and `aura:credits` start at zero; an economy/admin plugin or an operator-managed `balances.json` entry must credit a player before purchases succeed. Selling still requires the corresponding plain items in the online player's inventory.

The old `native/auraessentials` mixin sources are legacy and are not part of the server release build. The release workflow packages the vanilla engine and this plugin as separate files.
