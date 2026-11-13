# Horizon server plugins (ABI v1)

Runtime plugins are native shared libraries loaded from `plugins/` in the dedicated server's working directory. The plugin host loads them once at startup, in filename order; restart the server after changing a plugin. Each plugin is compiled independently and communicates through the versioned C ABI in `api/`. The loader isolates the unsafe dynamic-library and FFI boundary from Horizon's game crate.

The current Horizon tree does not contain a `horizon-mod-sdk` crate or a `ServerMod` runtime interface. Its documented trait mods implement Horizon's Rust `Hooks` and link into a generated game executable, so they still require rebuilding that executable when their logic changes. ABI v1 is the separate runtime API for dedicated-server plugins.

Build the Portuguese Aura Loja plugin from the repository root:

```sh
cargo build --release -p horizon-plugin-aura-loja
mkdir -p plugins
cp target/release/libhorizon_plugin_aura_loja.so plugins/
```

On Linux, the plugin and Horizon executable must use the same architecture and compatible Rust/C runtime. Native plugins execute in the server process and must be trusted. A plugin panic, invalid memory access, or incompatible binary can terminate the server. ABI version mismatches are rejected at startup.

The generic ABI exposes command registration, player messages, chest menus, menu clicks, and versioned player-join/player-quit/block-break/menu-closed events. Event payload pointers are valid only during the callback. Block break can be cancelled; the player lifecycle and menu-close events are notifications. `window_id` is meaningful only for menu-close; `position` and `data_id` are for block-break events.

`Aura Loja` has `/loja`, the eight AuraEssentials categories and 66 products from its Java `shop.yml`, with category, product and item-action chest screens in Portuguese. The buy/sell controls are visual only in this first slice: the server API does not yet expose an external plugin economy or inventory transaction API, so the plugin reports that no balance or items changed. That API is the next step before enabling purchases.

The server release workflow now builds Horizon without the old SMP mixin and places `Aura Loja` in `plugins/` as a separate `.so`. The old `native/auraessentials` mixin sources are not loaded by the runtime plugin host or included in that release.
