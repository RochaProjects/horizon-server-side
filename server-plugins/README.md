# Plugins de servidor Horizon (ABI v2 e v3)

Plugins Linux são bibliotecas Rust `.so` carregadas no início do servidor a partir de `plugins/`. O host valida a ABI, chama os callbacks no thread do servidor e os mantém carregados até o processo terminar. Para atualizar um plugin, substitua a biblioteca e reinicie o servidor.

O contrato público está em `api/`; o carregador e a fronteira FFI ficam em `loader/`. Não há crate `horizon-mod-sdk` nem trait `ServerMod` neste checkout. Os trait mods documentados em `docs/native-mods.md` são compilados dentro de um executável; os plugins deste SDK são compilados separadamente. ABI v2 continua carregando sem alteração. Plugins novos devem usar ABI v3: ela preserva as funções comuns de v2 e acrescenta uma chamada JSON extensível, eventos com payload JSON e permissões nomeadas. Novas operações genéricas podem ser acrescentadas sem alterar o layout da ABI.

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

The descriptor and its name must remain static while the library is loaded. Callback strings, menu item arrays, event payloads and the API pointer are borrowed only for the callback; copy data a plugin needs to keep. Return nonzero from `command` only when it handled the command. The v2 command helper remains public to all players; v3 command registration accepts a permission node and an operator-level fallback.

## 3. Build and install

From the repository root:

```sh
cargo build --release -p meu-plugin
mkdir -p plugins
cp target/release/libmeu_plugin.so plugins/
```

The server and plugin must target the same OS, architecture and compatible native runtime. Linux is the packaged server target today. The loader also recognizes `.dylib` and `.dll` on their respective platforms.

## APIs v2 and v3

`HostApiV2` provides generic command registration, player messages, vanilla chest menus, menu clicks, player lifecycle and block-break events, balance reads, and transactions. Menu titles, names and lore accept vanilla `§` formatting codes; a color code explicitly clears inherited decorations such as italic. An empty item color leaves the component's color unset. ABI v3 menus also accept arbitrary item `components`, including Java resource-pack components such as `minecraft:item_model` and `minecraft:custom_model_data`.

ABI v3's `call_json` accepts an UTF-8 JSON object `{ "operation": "...", "data": { ... } }` and returns JSON. Requests and responses are capped at 64 KiB. UUIDs are the two `u64` callback arguments (high then low); world and dimension IDs use namespaced strings such as `minecraft:overworld` and `example:sky`.

| Operation | Purpose |
|---|---|
| `command.register` | Register usage, operator-level fallback, and optional named permission node during plugin startup. |
| `chat.send` | Send a structured Minecraft text component to the callback player. |
| `menu.open` | Open a virtual chest with structured title, item names and lore; clicks go to `menu_click`. |
| `player.get` | Read online player's UUID, name, dimension, coordinates, rotation, mode, permission level and health. |
| `player.teleport` | Teleport to coordinates in a built-in or valid custom dimension; pending cross-level moves are queued by the server. |
| `player.game_mode.set` | Change the player's vanilla game mode. |
| `resource_pack.send`, `resource_pack.send_batch`, `resource_pack.remove` | Send/remove Java Edition server resource packs; batch sends 1–8 pack pushes in order during one plugin API call, and pack status is delivered as an event. This does not turn separate packs into a single archive or make the current Play-stage send happen before spawn. |
| `world.dimensions.list` | List built-in dimensions and custom dimensions that exist in the save. |
| `world.gamerule.get/set` | Read/change a vanilla gamerule for a loaded built-in level. Vanilla rules remain shared according to Horizon's current vanilla level model. |
| `world.custom_gamerule.get/set/clear` | Read, override or clear a gamerule override for an existing custom dimension. |
| `world.dimension.unload` | Save and unload an idle custom level when no player or ticket uses it. |
| `scoreboard.sidebar.set/clear` | Show a title and up to 15 styled lines (optional right-side value) to one player. |
| `entity.text_display.create` | Create a vanilla text display in a loaded custom or built-in level, with alignment, billboard, background, shadow and width settings; useful for holograms. |
| `entity.move/remove` | Move or remove an entity Horizon keeps as a non-simulated display/marker, by dimension and entity ID. |
| `plugin.storage.get/set/delete/list` | Persist JSON values under `plugins/<PluginId>/storage.json`, atomically, for homes, warps, requests, listings, and other plugin state. Values are capped at 8 KiB each and 4 MiB per plugin file. Existing `plugins/data/<PluginId>.json` data is moved on first access. |

Example request for a sidebar:

```json
{"operation":"scoreboard.sidebar.set","data":{"title":{"text":"Servidor","color":"gold"},"lines":[{"left":{"text":"Saldo: ","color":"gray"},"right":{"text":"120","color":"green"}}]}}
```

Storage requests include the plugin's lowercase ID as `plugin` and a `key`; `set` also takes any JSON `value`. `list` may omit `key` or use it as a prefix, and supports `limit` (1–256) plus the previous response's `next` value as `after`. Keep each plugin on its own ID. Plugins are trusted native code, so this namespace is organization and collision avoidance, not a security boundary.

Events currently include player join/quit, block break/place, menu close, dimension change, teleport, death, chat and Java resource-pack status. ABI v3 callbacks receive `{player,name,dimension,window_id,position,data_id,data}`. Current place events and chat events are notifications after the action; block-break events can be canceled. For plugin features that need cancellable place, chat formatting/interception, movement/interaction protection, or entity damage events, the engine still needs a hook before the action is committed.

Dimension APIs expose the engine's existing model: the three built-in dimensions, custom dimensions defined by the world's loaded data packs, and fallback custom dimensions opened with the Overworld generator. They do not yet create/rename/reset dimensions or select a new Nether/End/flat generator at runtime. Those need a separate world-registry/generation capability before a plugin can promise those operations. Text displays provide a vanilla primitive for holograms; actual player-model NPCs with skins need a fake-player/profile and entity-tracking API that this ABI does not expose yet. Java resource packs are sent by Horizon; Bedrock packs and Geyser extensions belong to the separate Velocity/Geyser proxy and are outside this backend API.

Transactions may name a shared currency ID (for example `aura:coins`), a signed balance delta, and signed plain-item deltas. To transact items without changing a balance, pass an empty currency ID and a zero balance delta. The host checks the balance and inventory, commits the balance file, then applies inventory changes on the server thread; it rolls the balance back if the inventory update fails. A transaction needs the player online and outside a player's active command turn, as menu clicks are. This is atomic during normal server processing, but the balance file and Minecraft's world save are separate files, so a process or machine crash at the precise moment of a purchase is not a cross-file atomic commit. Balance values are signed 64-bit integer units; the plugin that owns a currency defines its precision. `Aura Loja` uses hundredths, so `100` means `1,00` moeda or Crédito.

Balances are saved atomically in `plugins/AuraEssentialsHorizon/balances.json`, keyed by currency ID and player UUID. Existing `plugins/balances.json` files are moved there at startup. To seed a test account, stop the server and edit the file, using the player's 32-digit UUID without hyphens:

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

`aura-loja/` is the first separate plugin and identifies itself as `AuraEssentialsHorizon`. On first startup it creates `plugins/AuraEssentialsHorizon/config.json` and `shop.json` without overwriting files already present. `shop.json` contains the eight Portuguese categories and their products; edit a category array to add/remove products, set `icon`, `lore`, `itemModel` or `components`, and products are paginated automatically to fit the chest. `config.json` controls the menu title and the normal/VIP currency IDs. An operator can run `/loja recarregar` to validate and apply both files without restarting. Buying and selling use the generic transaction API. `aura:coins` and `aura:credits` start at zero; an economy/admin plugin or an operator-managed `balances.json` entry must credit a player before purchases succeed. Selling still requires the corresponding plain items in the online player's inventory.

Menu item models are Java-side item components. They can point at models/textures supplied by a Java resource pack; the plugin can request several Java packs in one API batch, but clients still receive and report each pack separately. Bedrock item textures are controlled by Geyser/Floodgate's proxy resource-pack setup, not by the Horizon backend plugin API.

The client supports Minecraft dialogs, and Horizon's protocol layer can encode `show_dialog` during both configuration and play. The public server-plugin API does not yet expose dialogs or a pre-placement login callback. Its resource-pack operation currently sends a Play-stage packet to an already placed/online player. Sending packs or dialogs before spawn requires a new configuration-stage hook and a way to hold the connection through the client's configuration exchange; the current login hook belongs to the native mod API and only allows accepting/refusing the join. So a plugin cannot yet guarantee a pre-spawn pack prompt or a dialog while holding a player in pre-login.

The old `native/auraessentials` mixin sources are legacy. The GitHub release workflow builds only the vanilla engine with the public plugin host, plus `horizon-server` and `horizon-prepare`; it does not build or package `aura-loja`. Build the plugin locally when needed and place its `.so` in the server's `plugins/` folder. The loader preserves that local plugin when installing a release that has no bundled plugin.
