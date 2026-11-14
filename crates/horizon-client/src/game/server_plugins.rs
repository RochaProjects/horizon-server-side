//! Runtime server plugins are a separate layer: only the narrow C ABI loader
//! is linked here, while plugin logic stays in independently built libraries.

use super::{
    Game,
    native_server::{NativeCommand, NativeServerPlayer, NativeSidebar},
};
#[cfg(feature = "server-plugins")]
use horizon_client::{
    items::ItemStack, lan::data::component_nbt, mod_api::GameRuleValue, text::Text,
};
#[cfg(feature = "server-plugins")]
use horizon_world::nbt::{Compound, Tag};
#[cfg(feature = "server-plugins")]
use horizon_server_plugin_loader::{Event, Host, ItemDelta, MenuItem, TransactionError};

#[cfg(feature = "server-plugins")]
struct GameHost<'g, 'a, 'c> {
    game: &'g mut Game<'a>,
    commands: Option<&'c mut Vec<NativeCommand>>,
}

#[cfg(feature = "server-plugins")]
impl Host for GameHost<'_, '_, '_> {
    fn register_command(&mut self, usage: &str) {
        if let Some(commands) = self.commands.as_deref_mut() {
            commands.push(NativeCommand::new(usage));
        }
    }

    fn send_message(&mut self, player: u128, message: &str) -> bool {
        self.game.native_tell(player, Text::literal(message))
    }

    fn open_menu(
        &mut self,
        player: u128,
        title: &str,
        rows: u8,
        items: Vec<MenuItem>,
    ) -> Option<i32> {
        let size = usize::from(rows.clamp(1, 6)) * 9;
        let mut stacks = vec![None; size];
        for item in items {
            if item.slot >= size {
                continue;
            }
            let id = if item.item.contains(':') {
                item.item
            } else {
                format!("minecraft:{}", item.item)
            };
            let mut stack = ItemStack::new(&id, item.count.clamp(1, 64));
            if !item.name.is_empty() {
                let color = match item.color.as_str() {
                    "" => "",
                    "black" => "§0",
                    "dark_blue" => "§1",
                    "dark_green" => "§2",
                    "dark_aqua" => "§3",
                    "dark_red" => "§4",
                    "dark_purple" => "§5",
                    "gold" => "§6",
                    "gray" => "§7",
                    "dark_gray" => "§8",
                    "blue" => "§9",
                    "green" => "§a",
                    "aqua" => "§b",
                    "red" => "§c",
                    "light_purple" => "§d",
                    "yellow" => "§e",
                    "white" => "§f",
                    _ => "§f",
                };
                // Parse legacy codes as Minecraft's component serializer does:
                // a color code also explicitly clears italic/bold decorations.
                // This matters because custom item names inherit italic in tooltips.
                let name = Text::literal(&format!("§r{color}{}", item.name)).resolve_legacy();
                stack.data_mut().custom_name = Some(component_nbt(&name.component()));
            }
            if !item.lore.is_empty() {
                let lore = serde_json::Value::Array(
                    item.lore
                        .iter()
                        .map(|line| Text::literal(line).resolve_legacy().component())
                        .collect(),
                );
                stack
                    .data_mut()
                    .other
                    .push(("minecraft:lore".into(), component_nbt(&lore)));
            }
            stacks[item.slot] = Some(stack);
        }
        self.game.native_open_menu(
            player,
            rows,
            Text::literal(title).resolve_legacy(),
            stacks,
        )
    }

    fn transact_items(
        &mut self,
        player: u128,
        items: &[ItemDelta],
        finish: &mut dyn FnMut(bool) -> bool,
    ) -> Result<(), TransactionError> {
        let changes: Vec<_> = items
            .iter()
            .map(|item| (item.item.clone(), item.delta))
            .collect();
        self.game
            .native_plugin_item_transaction(player, &changes, finish)
            .map_err(|()| TransactionError)
    }

    fn call_json(
        &mut self,
        player: u128,
        request: serde_json::Value,
    ) -> serde_json::Value {
        use horizon_server_plugin_api::operation as op;

        let Some(operation) = request.get("operation").and_then(|v| v.as_str()) else {
            return serde_json::json!({"ok": false, "error": "missing_operation"});
        };
        let data = request.get("data").cloned().unwrap_or_default();
        let fail = |reason: &str| serde_json::json!({"ok": false, "error": reason});
        match operation {
            op::COMMAND_REGISTER => {
                let Some(usage) = data.get("usage").and_then(|v| v.as_str()) else {
                    return fail("invalid_usage");
                };
                let level = data
                    .get("default_level")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                    .min(4) as u8;
                let mut command = NativeCommand::op(usage, level);
                if let Some(permission) = data.get("permission").and_then(|v| v.as_str()) {
                    if permission.is_empty() || permission.len() > 128 {
                        return fail("invalid_permission");
                    }
                    command = command.permission(permission);
                }
                if let Some(commands) = self.commands.as_deref_mut() {
                    commands.push(command);
                    serde_json::json!({"ok": true})
                } else {
                    fail("commands_can_only_be_registered_during_startup")
                }
            }
            op::CHAT_SEND => {
                let Some(component) = data.get("component") else {
                    return fail("missing_component");
                };
                let text = horizon_client::packs::component(component);
                serde_json::json!({"ok": self.game.native_tell(player, text)})
            }
            op::MENU_OPEN => {
                let rows = data
                    .get("rows")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3)
                    .clamp(1, 6) as u8;
                let Some(items_json) = data.get("items").and_then(|v| v.as_array()) else {
                    return fail("invalid_items");
                };
                if items_json.len() > 54 {
                    return fail("too_many_items");
                }
                let title = data
                    .get("title")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({"text": ""}));
                let title = if let Some(literal) = title.as_str() {
                    Text::literal(literal).resolve_legacy()
                } else {
                    horizon_client::packs::component(&title)
                };
                let size = usize::from(rows) * 9;
                let mut stacks = vec![None; size];
                for entry in items_json {
                    let Some(slot) = entry.get("slot").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    if slot >= size as u64 {
                        continue;
                    }
                    let Some(id) = entry.get("item").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if id.len() > 256 {
                        continue;
                    }
                    let id = if id.contains(':') {
                        id.to_owned()
                    } else {
                        format!("minecraft:{id}")
                    };
                    let count = entry
                        .get("count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1)
                        .clamp(1, 64) as u32;
                    let mut stack = ItemStack::new(&id, count);
                    if let Some(name) = entry.get("name") {
                        let name = if let Some(literal) = name.as_str() {
                            Text::literal(literal).resolve_legacy().component()
                        } else {
                            name.clone()
                        };
                        stack.data_mut().custom_name = Some(component_nbt(&name));
                    }
                    if let Some(lore) = entry.get("lore").and_then(|v| v.as_array()) {
                        let lore: Vec<_> = lore
                            .iter()
                            .map(|line| {
                                if let Some(literal) = line.as_str() {
                                    Text::literal(literal).resolve_legacy().component()
                                } else {
                                    line.clone()
                                }
                            })
                            .collect();
                        stack.data_mut().other.push((
                            "minecraft:lore".into(),
                            component_nbt(&serde_json::Value::Array(lore)),
                        ));
                    }
                    if let Some(components) = entry.get("components").and_then(|v| v.as_object()) {
                        for (component, value) in components {
                            if component.len() > 128
                                || !component.contains(':')
                                || !component
                                    .bytes()
                                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_:/.-".contains(&byte))
                            {
                                continue;
                            }
                            stack.data_mut().set_loot_component(component, value);
                        }
                    }
                    stacks[slot as usize] = Some(stack);
                }
                let window = self.game.native_open_menu(player, rows, title, stacks);
                serde_json::json!({"ok": window.is_some(), "window_id": window})
            }
            op::RESOURCE_PACK_SEND | op::RESOURCE_PACK_SEND_BATCH => {
                let packs: Vec<&serde_json::Value> = if operation == op::RESOURCE_PACK_SEND {
                    vec![&data]
                } else {
                    let Some(packs) = data.get("packs").and_then(|value| value.as_array()) else {
                        return fail("invalid_packs");
                    };
                    if packs.is_empty() || packs.len() > 8 {
                        return fail("invalid_pack_count");
                    }
                    packs.iter().collect()
                };
                let mut sent = 0;
                for pack in packs {
                    let Some(id) = pack
                        .get("id")
                        .and_then(|value| value.as_str())
                        .and_then(|value| u128::from_str_radix(&value.replace('-', ""), 16).ok())
                    else {
                        return serde_json::json!({"ok": false, "sent": sent, "error": "invalid_pack_id"});
                    };
                    let Some(url) = pack.get("url").and_then(|value| value.as_str()) else {
                        return serde_json::json!({"ok": false, "sent": sent, "error": "invalid_url"});
                    };
                    let sha1 = pack.get("sha1").and_then(|value| value.as_str()).unwrap_or("");
                    let prompt = pack.get("prompt").map(horizon_client::packs::component);
                    let required = pack.get("required").and_then(|value| value.as_bool()).unwrap_or(false);
                    if !self.game.native_send_resource_pack(player, id, url, sha1, prompt.as_ref(), required) {
                        return serde_json::json!({"ok": false, "sent": sent, "error": "pack_send_failed"});
                    }
                    sent += 1;
                }
                serde_json::json!({"ok": true, "sent": sent})
            }
            op::RESOURCE_PACK_REMOVE => {
                let id = data
                    .get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|v| u128::from_str_radix(v, 16).ok());
                serde_json::json!({"ok": self.game.native_pop_resource_pack(player, id)})
            }
            op::PLAYER_GET => {
                let snapshot = self
                    .game
                    .native_server_players()
                    .into_iter()
                    .find(|snapshot| snapshot.uuid == player)
                    .or_else(|| {
                        self.game
                            .native_acting_player()
                            .filter(|snapshot| snapshot.uuid == player)
                    });
                let Some(snapshot) = snapshot else {
                    return fail("player_not_online");
                };
                serde_json::json!({
                    "ok": true,
                    "uuid": format!("{:032x}", snapshot.uuid),
                    "name": snapshot.name,
                    "dimension": snapshot.dimension,
                    "position": snapshot.position,
                    "yaw": snapshot.yaw,
                    "pitch": snapshot.pitch,
                    "game_mode": snapshot.game_mode,
                    "permission_level": snapshot.permission,
                    "health": snapshot.health,
                })
            }
            op::PLAYER_TELEPORT => {
                let Some(dimension) = data.get("dimension").and_then(|v| v.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(position) = data.get("position").and_then(|v| v.as_array()) else {
                    return fail("invalid_position");
                };
                if position.len() != 3 {
                    return fail("invalid_position");
                }
                let Some(position) = position
                    .iter()
                    .map(serde_json::Value::as_f64)
                    .collect::<Option<Vec<_>>>()
                    .and_then(|p| <[f64; 3]>::try_from(p).ok())
                else {
                    return fail("invalid_position");
                };
                let yaw = data.get("yaw").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let pitch = data.get("pitch").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                serde_json::json!({
                    "ok": self.game.native_teleport_player(player, dimension, position, yaw, pitch)
                })
            }
            op::PLAYER_GAME_MODE_SET => {
                let Some(mode) = data.get("mode").and_then(|v| v.as_i64()) else {
                    return fail("invalid_game_mode");
                };
                serde_json::json!({
                    "ok": self.game.native_set_game_mode(player, mode as i32)
                })
            }
            op::WORLD_DIMENSIONS_LIST => {
                let dimensions: Vec<_> = self
                    .game
                    .world_dimensions()
                    .into_iter()
                    .map(|dimension| dimension.id())
                    .collect();
                serde_json::json!({"ok": true, "dimensions": dimensions})
            }
            op::WORLD_GAMERULE_GET => {
                let Some(dimension) = data.get("dimension").and_then(|v| v.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(rule) = data.get("rule").and_then(|v| v.as_str()) else {
                    return fail("invalid_rule");
                };
                let value = self
                    .game
                    .native_level(dimension)
                    .and_then(|level| self.game.native_level_game_rule(level, rule));
                match value {
                    Some(GameRuleValue::Bool(value)) => {
                        serde_json::json!({"ok": true, "value": value})
                    }
                    Some(GameRuleValue::Int(value)) => {
                        serde_json::json!({"ok": true, "value": value})
                    }
                    None => fail("unknown_or_unloaded_rule"),
                }
            }
            op::WORLD_GAMERULE_SET => {
                let Some(dimension) = data.get("dimension").and_then(|v| v.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(rule) = data.get("rule").and_then(|v| v.as_str()) else {
                    return fail("invalid_rule");
                };
                let Some(value) = game_rule_value(data.get("value")) else {
                    return fail("invalid_rule_value");
                };
                let changed = self
                    .game
                    .native_level(dimension)
                    .is_some_and(|level| self.game.native_level_set_game_rule(level, rule, value));
                serde_json::json!({"ok": changed})
            }
            op::WORLD_CUSTOM_GAMERULE_GET => {
                let Some(dimension) = data.get("dimension").and_then(|v| v.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(rule) = data.get("rule").and_then(|v| v.as_str()) else {
                    return fail("invalid_rule");
                };
                match self.game.native_dimension_game_rule(dimension, rule) {
                    Some(GameRuleValue::Bool(value)) => {
                        serde_json::json!({"ok": true, "value": value})
                    }
                    Some(GameRuleValue::Int(value)) => {
                        serde_json::json!({"ok": true, "value": value})
                    }
                    None => fail("unknown_custom_dimension_or_rule"),
                }
            }
            op::WORLD_CUSTOM_GAMERULE_SET => {
                let Some(dimension) = data.get("dimension").and_then(|v| v.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(rule) = data.get("rule").and_then(|v| v.as_str()) else {
                    return fail("invalid_rule");
                };
                let Some(value) = game_rule_value(data.get("value")) else {
                    return fail("invalid_rule_value");
                };
                serde_json::json!({
                    "ok": self.game.native_dimension_set_game_rule(dimension, rule, Some(value))
                })
            }
            op::WORLD_CUSTOM_GAMERULE_CLEAR => {
                let Some(dimension) = data.get("dimension").and_then(|v| v.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(rule) = data.get("rule").and_then(|v| v.as_str()) else {
                    return fail("invalid_rule");
                };
                serde_json::json!({
                    "ok": self.game.native_dimension_set_game_rule(dimension, rule, None)
                })
            }
            op::WORLD_DIMENSION_UNLOAD => {
                let Some(dimension) = data.get("dimension").and_then(|v| v.as_str()) else {
                    return fail("invalid_dimension");
                };
                serde_json::json!({"ok": self.game.native_unload_dimension(dimension)})
            }
            op::SCOREBOARD_SIDEBAR_SET => {
                let Some(title) = data.get("title") else {
                    return fail("missing_title");
                };
                let Some(lines) = data.get("lines").and_then(|value| value.as_array()) else {
                    return fail("invalid_lines");
                };
                if lines.len() > 15 {
                    return fail("too_many_lines");
                }
                let title = if let Some(text) = title.as_str() {
                    Text::literal(text).resolve_legacy()
                } else {
                    horizon_client::packs::component(title)
                };
                let mut sidebar = NativeSidebar {
                    title,
                    lines: Vec::with_capacity(lines.len()),
                };
                for line in lines {
                    let Some(left) = line.get("left") else {
                        return fail("invalid_line");
                    };
                    let left = if let Some(text) = left.as_str() {
                        Text::literal(text).resolve_legacy()
                    } else {
                        horizon_client::packs::component(left)
                    };
                    let right = line.get("right").filter(|value| !value.is_null()).map(|value| {
                        if let Some(text) = value.as_str() {
                            Text::literal(text).resolve_legacy()
                        } else {
                            horizon_client::packs::component(value)
                        }
                    });
                    sidebar.lines.push((left, right));
                }
                serde_json::json!({"ok": self.game.native_sidebar(player, Some(&sidebar))})
            }
            op::SCOREBOARD_SIDEBAR_CLEAR => {
                serde_json::json!({"ok": self.game.native_sidebar(player, None)})
            }
            op::ENTITY_TEXT_DISPLAY_CREATE => {
                let Some(dimension) = data.get("dimension").and_then(|value| value.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(position) = json_position(data.get("position")) else {
                    return fail("invalid_position");
                };
                let Some(text) = data.get("text") else {
                    return fail("missing_text");
                };
                let component = if let Some(text) = text.as_str() {
                    Text::literal(text).resolve_legacy().component()
                } else {
                    text.clone()
                };
                let line_width = data.get("line_width").and_then(|value| value.as_u64()).unwrap_or(200).clamp(1, 2000) as i32;
                let background = data.get("background").and_then(|value| value.as_u64()).unwrap_or(0x4000_0000).min(u64::from(u32::MAX)) as u32;
                let mut nbt = Compound::new();
                nbt.insert("text".into(), component_nbt(&component));
                nbt.insert("line_width".into(), Tag::Int(line_width));
                nbt.insert("background".into(), Tag::Int(background as i32));
                nbt.insert("text_opacity".into(), Tag::Byte(-1));
                nbt.insert("shadow".into(), Tag::Byte(i8::from(data.get("shadow").and_then(|value| value.as_bool()).unwrap_or(false))));
                nbt.insert("see_through".into(), Tag::Byte(i8::from(data.get("see_through").and_then(|value| value.as_bool()).unwrap_or(false))));
                let alignment = match data.get("alignment").and_then(|value| value.as_str()).unwrap_or("center") {
                    "left" => 0,
                    "center" => 1,
                    "right" => 2,
                    _ => return fail("invalid_alignment"),
                };
                nbt.insert("alignment".into(), Tag::Byte(alignment));
                let billboard = match data.get("billboard").and_then(|value| value.as_str()).unwrap_or("center") {
                    "fixed" => 0,
                    "vertical" => 1,
                    "horizontal" => 2,
                    "center" => 3,
                    _ => return fail("invalid_billboard"),
                };
                nbt.insert("billboard".into(), Tag::Byte(billboard));
                match self.game.native_summon(dimension, "minecraft:text_display", position, nbt) {
                    Some(entity_id) => serde_json::json!({"ok": true, "entity_id": entity_id}),
                    None => fail("entity_spawn_failed"),
                }
            }
            op::ENTITY_MOVE => {
                let Some(dimension) = data.get("dimension").and_then(|value| value.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(entity_id) = data.get("entity_id").and_then(|value| value.as_u64()).and_then(|id| u32::try_from(id).ok()) else {
                    return fail("invalid_entity_id");
                };
                let Some(position) = json_position(data.get("position")) else {
                    return fail("invalid_position");
                };
                let yaw = data.get("yaw").and_then(|value| value.as_f64()).unwrap_or(0.0) as f32;
                let pitch = data.get("pitch").and_then(|value| value.as_f64()).unwrap_or(0.0) as f32;
                serde_json::json!({"ok": self.game.native_move_entity(dimension, entity_id, position, yaw, pitch)})
            }
            op::ENTITY_REMOVE => {
                let Some(dimension) = data.get("dimension").and_then(|value| value.as_str()) else {
                    return fail("invalid_dimension");
                };
                let Some(entity_id) = data.get("entity_id").and_then(|value| value.as_u64()).and_then(|id| u32::try_from(id).ok()) else {
                    return fail("invalid_entity_id");
                };
                serde_json::json!({"ok": self.game.native_remove_entity(dimension, entity_id)})
            }
            op::PLUGIN_STORAGE_GET | op::PLUGIN_STORAGE_SET | op::PLUGIN_STORAGE_DELETE | op::PLUGIN_STORAGE_LIST => {
                plugin_storage(operation, &data)
            }
            _ => fail("unsupported_operation"),
        }
    }
}

#[cfg(feature = "server-plugins")]
fn plugin_storage(operation: &str, data: &serde_json::Value) -> serde_json::Value {
    use horizon_server_plugin_api::operation as op;
    use std::{fs, path::PathBuf};

    let Some(namespace) = data.get("plugin").and_then(|value| value.as_str()) else {
        return serde_json::json!({"ok": false, "error": "invalid_plugin_id"});
    };
    if namespace.is_empty()
        || namespace.len() > 64
        || namespace == "."
        || namespace == ".."
        || !namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return serde_json::json!({"ok": false, "error": "invalid_plugin_id"});
    }
    let key = data.get("key").and_then(|value| value.as_str());
    if operation != op::PLUGIN_STORAGE_LIST && key.is_none() {
        return serde_json::json!({"ok": false, "error": "invalid_key"});
    }
    if key.is_some_and(|key| {
        key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    }) {
        return serde_json::json!({"ok": false, "error": "invalid_key"});
    }
    let Ok(root) = std::env::current_dir() else {
        return serde_json::json!({"ok": false, "error": "storage_unavailable"});
    };
    let plugin_dir: PathBuf = root.join("plugins").join(namespace);
    let path = plugin_dir.join("storage.json");
    if !path.exists() {
        let legacy = root
            .join("plugins")
            .join("data")
            .join(format!("{namespace}.json"));
        if legacy.exists()
            && fs::create_dir_all(&plugin_dir)
                .and_then(|()| fs::rename(&legacy, &path))
                .is_err()
        {
            return serde_json::json!({"ok": false, "error": "storage_migration_failed"});
        }
    }
    let mut values = match fs::read(&path) {
        Ok(bytes) if bytes.len() <= 4 * 1024 * 1024 => match serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes) {
            Ok(values) => values,
            Err(_) => return serde_json::json!({"ok": false, "error": "storage_corrupt"}),
        },
        Ok(_) => return serde_json::json!({"ok": false, "error": "storage_too_large"}),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(_) => return serde_json::json!({"ok": false, "error": "storage_unavailable"}),
    };
    match operation {
        op::PLUGIN_STORAGE_GET => serde_json::json!({"ok": true, "value": values.get(key.expect("validated key"))}),
        op::PLUGIN_STORAGE_LIST => {
            let prefix = key.unwrap_or("");
            let after = data.get("after").and_then(|value| value.as_str()).unwrap_or("");
            let limit = data.get("limit").and_then(|value| value.as_u64()).unwrap_or(128).clamp(1, 256) as usize;
            let mut keys: Vec<_> = values
                .keys()
                .filter(|candidate| candidate.starts_with(prefix) && candidate.as_str() > after)
                .cloned()
                .collect();
            keys.sort();
            let has_more = keys.len() > limit;
            keys.truncate(limit);
            let next = has_more.then(|| keys.last().cloned()).flatten();
            serde_json::json!({"ok": true, "keys": keys, "next": next})
        }
        op::PLUGIN_STORAGE_DELETE => {
            let removed = values.remove(key.expect("validated key")).is_some();
            if removed && !write_plugin_storage(&path, &values) {
                return serde_json::json!({"ok": false, "error": "storage_write_failed"});
            }
            serde_json::json!({"ok": true, "removed": removed})
        }
        op::PLUGIN_STORAGE_SET => {
            let Some(value) = data.get("value") else {
                return serde_json::json!({"ok": false, "error": "missing_value"});
            };
            let Ok(encoded) = serde_json::to_vec(value) else {
                return serde_json::json!({"ok": false, "error": "invalid_value"});
            };
            if encoded.len() > 8_192 {
                return serde_json::json!({"ok": false, "error": "value_too_large"});
            }
            values.insert(key.expect("validated key").to_owned(), value.clone());
            if !write_plugin_storage(&path, &values) {
                return serde_json::json!({"ok": false, "error": "storage_write_failed"});
            }
            serde_json::json!({"ok": true})
        }
        _ => serde_json::json!({"ok": false, "error": "unsupported_operation"}),
    }
}

#[cfg(feature = "server-plugins")]
fn write_plugin_storage(path: &std::path::Path, values: &serde_json::Map<String, serde_json::Value>) -> bool {
    use std::{fs, io::Write};
    let Ok(bytes) = serde_json::to_vec(values) else { return false };
    if bytes.len() > 4 * 1024 * 1024 { return false; }
    let Some(parent) = path.parent() else { return false };
    if fs::create_dir_all(parent).is_err() { return false; }
    let temp = path.with_extension("json.tmp");
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() { let _ = fs::remove_file(temp); return false; }
    true
}

#[cfg(feature = "server-plugins")]
fn game_rule_value(value: Option<&serde_json::Value>) -> Option<GameRuleValue> {
    match value? {
        serde_json::Value::Bool(value) => Some(GameRuleValue::Bool(*value)),
        serde_json::Value::Number(value) => {
            Some(GameRuleValue::Int(i32::try_from(value.as_i64()?).ok()?))
        }
        _ => None,
    }
}

#[cfg(feature = "server-plugins")]
fn json_position(value: Option<&serde_json::Value>) -> Option<[f64; 3]> {
    let values = value?.as_array()?;
    if values.len() != 3 {
        return None;
    }
    values
        .iter()
        .map(serde_json::Value::as_f64)
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

#[cfg(feature = "server-plugins")]
fn root() -> Option<std::path::PathBuf> {
    std::env::current_dir().ok()
}

#[cfg(feature = "server-plugins")]
pub(super) fn register_commands(game: &mut Game<'_>, commands: &mut Vec<NativeCommand>) {
    if game.options.dedicated.is_none() {
        return;
    }
    let Some(root) = root() else {
        return;
    };
    let mut host = GameHost {
        game,
        commands: Some(commands),
    };
    horizon_server_plugin_loader::register_commands(&root, &mut host);
}

#[cfg(feature = "server-plugins")]
pub(super) fn permission_value(
    game: &Game<'_>,
    player: u128,
    node: &str,
    default_level: u8,
) -> Option<bool> {
    if game.options.dedicated.is_none() {
        return None;
    }
    let root = root()?;
    horizon_server_plugin_loader::permission_value(&root, player, node, default_level)
}

#[cfg(feature = "server-plugins")]
pub(super) fn perform_command(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    if *ran || game.options.dedicated.is_none() {
        return;
    }
    let Some(root) = root() else {
        return;
    };
    let mut host = GameHost {
        game,
        commands: None,
    };
    *ran = horizon_server_plugin_loader::perform_command(&root, &mut host, player.uuid, command);
}

#[cfg(feature = "server-plugins")]
pub(super) fn menu_clicked(
    game: &mut Game<'_>,
    player: u128,
    window: i32,
    slot: i32,
    button: i32,
    mode: i32,
) {
    if game.options.dedicated.is_none() {
        return;
    }
    let Some(root) = root() else {
        return;
    };
    let mut host = GameHost {
        game,
        commands: None,
    };
    horizon_server_plugin_loader::menu_click(&root, &mut host, player, window, slot, button, mode);
}

#[cfg(feature = "server-plugins")]
pub(super) fn dispatch_event(
    game: &mut Game<'_>,
    kind: u32,
    player: u128,
    name: &str,
    position: [i32; 3],
    data_id: u32,
    payload: serde_json::Value,
) -> bool {
    if game.options.dedicated.is_none() {
        return false;
    }
    let Some(root) = root() else {
        return false;
    };
    let event = Event {
        kind,
        player,
        name: name.to_owned(),
        dimension: game.dim.id().to_owned(),
        window_id: -1,
        position,
        data_id,
        payload,
    };
    let mut host = GameHost {
        game,
        commands: None,
    };
    horizon_server_plugin_loader::dispatch_event(&root, &mut host, &event)
}

#[cfg(feature = "server-plugins")]
pub(super) fn menu_closed(game: &mut Game<'_>, player: u128, window: i32) {
    if game.options.dedicated.is_none() {
        return;
    }
    let Some(root) = root() else {
        return;
    };
    let event = Event {
        kind: horizon_server_plugin_api::event::MENU_CLOSED,
        player,
        name: String::new(),
        dimension: game.dim.id().to_owned(),
        window_id: window,
        position: [0; 3],
        data_id: 0,
        payload: serde_json::Value::Null,
    };
    let mut host = GameHost {
        game,
        commands: None,
    };
    horizon_server_plugin_loader::dispatch_event(&root, &mut host, &event);
}

#[cfg(feature = "server-plugins")]
pub(super) fn block_break(game: &mut Game<'_>, pos: [i32; 3], data_id: u32) -> bool {
    let Some(player) = game.native_acting_player() else {
        return false;
    };
    let (uuid, name) = (player.uuid, player.name.clone());
    dispatch_event(
        game,
        horizon_server_plugin_api::event::BLOCK_BREAK,
        uuid,
        &name,
        pos,
        data_id,
        serde_json::Value::Null,
    )
}

#[cfg(feature = "server-plugins")]
pub(super) fn player_join(game: &mut Game<'_>, player: u128, name: &str) {
    dispatch_event(
        game,
        horizon_server_plugin_api::event::PLAYER_JOIN,
        player,
        name,
        [0; 3],
        0,
        serde_json::Value::Null,
    );
}

#[cfg(feature = "server-plugins")]
pub(super) fn player_quit(game: &mut Game<'_>, player: u128, name: &str) {
    dispatch_event(
        game,
        horizon_server_plugin_api::event::PLAYER_QUIT,
        player,
        name,
        [0; 3],
        0,
        serde_json::Value::Null,
    );
}

#[cfg(feature = "server-plugins")]
pub(super) fn player_changed_dimension(
    game: &mut Game<'_>,
    player: u128,
    name: &str,
    from: &str,
    to: &str,
) {
    dispatch_event(
        game,
        horizon_server_plugin_api::event::PLAYER_CHANGED_DIMENSION,
        player,
        name,
        [0; 3],
        0,
        serde_json::json!({"from": from, "to": to}),
    );
}

#[cfg(feature = "server-plugins")]
pub(super) fn player_teleported(
    game: &mut Game<'_>,
    player: u128,
    name: &str,
    from: [f64; 3],
    to: [f64; 3],
    dimension: &str,
) {
    dispatch_event(
        game,
        horizon_server_plugin_api::event::PLAYER_TELEPORT,
        player,
        name,
        to.map(|value| value.floor() as i32),
        0,
        serde_json::json!({"from": from, "to": to, "dimension": dimension}),
    );
}

#[cfg(feature = "server-plugins")]
pub(super) fn player_chat(game: &mut Game<'_>, player: u128, name: &str, text: &str) {
    dispatch_event(
        game,
        horizon_server_plugin_api::event::CHAT,
        player,
        name,
        [0; 3],
        0,
        serde_json::json!({"message": text, "phase": "after_broadcast"}),
    );
}

#[cfg(feature = "server-plugins")]
pub(super) fn block_place(game: &mut Game<'_>, pos: [i32; 3], data_id: u32) {
    let Some(player) = game.native_acting_player() else {
        return;
    };
    dispatch_event(
        game,
        horizon_server_plugin_api::event::BLOCK_PLACE,
        player.uuid,
        &player.name,
        pos,
        data_id,
        serde_json::Value::Null,
    );
}

#[cfg(feature = "server-plugins")]
pub(super) fn player_death(
    game: &mut Game<'_>,
    entity: u32,
    damage_type: &str,
    attacker: u32,
) -> bool {
    let Some(player) = game
        .native_server_players()
        .into_iter()
        .find(|player| player.entity_id == entity as i32)
    else {
        return false;
    };
    dispatch_event(
        game,
        horizon_server_plugin_api::event::PLAYER_DEATH,
        player.uuid,
        &player.name,
        player.position.map(|value| value.floor() as i32),
        0,
        serde_json::json!({"damage_type": damage_type, "attacker_entity_id": attacker}),
    )
}

#[cfg(feature = "server-plugins")]
pub(super) fn resource_pack_status(
    game: &mut Game<'_>,
    player: u128,
    name: &str,
    pack: u128,
    status: i32,
) {
    dispatch_event(
        game,
        horizon_server_plugin_api::event::RESOURCE_PACK_STATUS,
        player,
        name,
        [0; 3],
        status as u32,
        serde_json::json!({"pack_id": format!("{pack:032x}"), "status": status}),
    );
}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn register_commands(_: &mut Game<'_>, _: &mut Vec<NativeCommand>) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn permission_value(_: &Game<'_>, _: u128, _: &str, _: u8) -> Option<bool> {
    None
}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn perform_command(_: &mut Game<'_>, _: &NativeServerPlayer, _: &str, _: &mut bool) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn menu_clicked(_: &mut Game<'_>, _: u128, _: i32, _: i32, _: i32, _: i32) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn menu_closed(_: &mut Game<'_>, _: u128, _: i32) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn block_break(_: &mut Game<'_>, _: [i32; 3], _: u32) -> bool {
    false
}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn player_join(_: &mut Game<'_>, _: u128, _: &str) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn player_quit(_: &mut Game<'_>, _: u128, _: &str) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn player_changed_dimension(_: &mut Game<'_>, _: u128, _: &str, _: &str, _: &str) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn player_teleported(
    _: &mut Game<'_>,
    _: u128,
    _: &str,
    _: [f64; 3],
    _: [f64; 3],
    _: &str,
) {
}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn player_chat(_: &mut Game<'_>, _: u128, _: &str, _: &str) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn block_place(_: &mut Game<'_>, _: [i32; 3], _: u32) {}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn player_death(_: &mut Game<'_>, _: u32, _: &str, _: u32) -> bool {
    false
}

#[cfg(not(feature = "server-plugins"))]
pub(super) fn resource_pack_status(_: &mut Game<'_>, _: u128, _: &str, _: u128, _: i32) {}
