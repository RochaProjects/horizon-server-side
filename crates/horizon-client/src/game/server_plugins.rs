//! Runtime server plugins are a separate layer: only the narrow C ABI loader
//! is linked here, while plugin logic stays in independently built libraries.

use super::{
    Game,
    native_server::{NativeCommand, NativeServerPlayer},
};
use horizon_client::{items::ItemStack, lan::data::component_nbt, text::Text};
use horizon_server_plugin_loader::{Event, Host, MenuItem};
use std::path::Path;

struct GameHost<'g, 'a, 'c> {
    game: &'g mut Game<'a>,
    commands: Option<&'c mut Vec<NativeCommand>>,
}

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
                let name = Text::legacy(&format!("§r{color}{}", item.name));
                stack.data_mut().custom_name = Some(component_nbt(&name.component()));
            }
            if !item.lore.is_empty() {
                let lore = serde_json::Value::Array(
                    item.lore
                        .iter()
                        .map(|line| Text::legacy(line).component())
                        .collect(),
                );
                stack
                    .data_mut()
                    .other
                    .push(("minecraft:lore".into(), component_nbt(&lore)));
            }
            stacks[item.slot] = Some(stack);
        }
        self.game
            .native_open_menu(player, rows, Text::literal(title), stacks)
    }
}

fn root() -> Option<std::path::PathBuf> {
    std::env::current_dir().ok()
}

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

pub(super) fn dispatch_event(
    game: &mut Game<'_>,
    kind: u32,
    player: u128,
    name: &str,
    position: [i32; 3],
    data_id: u32,
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
    };
    let mut host = GameHost {
        game,
        commands: None,
    };
    horizon_server_plugin_loader::dispatch_event(&root, &mut host, &event)
}

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
    };
    let mut host = GameHost {
        game,
        commands: None,
    };
    horizon_server_plugin_loader::dispatch_event(&root, &mut host, &event);
}
