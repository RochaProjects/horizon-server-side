// Chat pings in the game (rules in smp_ping.rs): `/pingsound`, the lines
// heard by `native_player_chat` (queued in the client library's `smp_chat`,
// as that hook runs in the server's hub) and the whispers heard by
// `native_player_command`, matched and played at the end of the tick they
// were said in. A tick with nothing said costs two atomic loads. Added to
// the game as `game::smp_ping_glue`.

use crate::game::{
    Game,
    native_server::{NativeCommand, NativeServerPlayer},
    smp_economy::Out,
    smp_ping::{self, Pings, Said},
    smp_store,
};
use horizon_client::text::{self, Text};
use horizon_protocol::packets::play::{self, Clientbound as C};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// `pings.json`, opened at the first use.
static PINGS: Mutex<Option<Result<Pings, String>>> = Mutex::new(None);
/// Whispers said since the last tick: (sender, recipient's name).
static WHISPERS: Mutex<Vec<(u128, String)>> = Mutex::new(Vec::new());
static WHISPERED: AtomicBool = AtomicBool::new(false);
/// The most lines kept for one tick (a flood is not worth more pings).
const MAX: usize = 64;

pub(in crate::game) fn commands(commands: &mut Vec<NativeCommand>) {
    for usage in ["pingsound", "pingsound on", "pingsound off"] {
        commands.push(NativeCommand::new(usage));
    }
}

fn with_pings<R>(game: &Game<'_>, f: impl FnOnce(&mut Pings) -> R) -> Option<R> {
    let dir = game.native_world_dir()?.join(smp_store::DIR);
    let mut guard = PINGS.lock().unwrap_or_else(|e| e.into_inner());
    let pings = guard.get_or_insert_with(|| {
        let opened = Pings::open(&dir);
        if let Err(e) = &opened {
            log::error!("[AuraEssentials] pings are off: {e}");
        }
        opened
    });
    pings.as_mut().ok().map(f)
}

/// `/pingsound [on|off]` (`native_perform_command`).
pub(in crate::game) fn perform(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    if command.split_whitespace().next() != Some("pingsound") {
        return;
    }
    *ran = true;
    let out = with_pings(game, |p| p.command(player.uuid, command))
        .flatten()
        .unwrap_or_else(|| {
            vec![Out::Error(
                "Pings are off: pings.json can't be read (see the server log).".into(),
            )]
        });
    for o in out {
        let line = match o {
            Out::Reply(s) => Text::literal(s).with_color(text::GREEN),
            Out::Error(s) => Text::literal(s).with_color(text::RED),
            _ => continue,
        };
        game.native_tell(player.uuid, line);
    }
}

/// A command a player typed (`native_player_command`): a whisper is noted
/// for the tick's pings.
pub(in crate::game) fn command_heard(player: u128, command: &str) {
    let Some(name) = smp_ping::whisper_to(command) else {
        return;
    };
    let mut w = WHISPERS.lock().unwrap_or_else(|e| e.into_inner());
    if w.len() < MAX {
        w.push((player, name.to_owned()));
        WHISPERED.store(true, Relaxed);
    }
}

/// The end of a tick: the pings of what was said in it.
pub(in crate::game) fn tick(game: &mut Game<'_>) {
    let chat = horizon_client::smp_chat::take();
    let whispers = if WHISPERED.swap(false, Relaxed) {
        std::mem::take(&mut *WHISPERS.lock().unwrap_or_else(|e| e.into_inner()))
    } else {
        Vec::new()
    };
    if chat.is_empty() && whispers.is_empty() {
        return;
    }
    let players = game.native_server_players();
    let online: Vec<(u128, String)> = players.iter().map(|p| (p.uuid, p.name.clone())).collect();
    let now = chrono::Utc::now().timestamp_millis();
    let said = chat
        .into_iter()
        .map(|(s, line)| (s, Said::Chat(line)))
        .chain(
            whispers
                .into_iter()
                .map(|(s, name)| (s, Said::Whisper(name))),
        );
    let said: Vec<(u128, Said)> = said.collect();
    let pings = with_pings(game, |p| {
        said.iter()
            .flat_map(|(s, what)| {
                p.pinged(*s, what, &online, now)
                    .into_iter()
                    .map(move |t| (*s, t))
            })
            .collect::<Vec<(u128, u128)>>()
    })
    .unwrap_or_default();
    let name = |u: u128| {
        players
            .iter()
            .find(|p| p.uuid == u)
            .map_or("?", |p| p.name.as_str())
    };
    for &(sender, uuid) in &pings {
        if let Some(p) = players.iter().find(|p| p.uuid == uuid) {
            // At most one line each 3 s per player: the log shows pings work.
            log::info!("[AuraEssentials] ping: {} by {}", p.name, name(sender));
            game.native_send(uuid, ping_sound(p.position, now));
        }
    }
}

/// The ping, for one player at `at` (the `player` sound source).
fn ping_sound(at: [f64; 3], seed: i64) -> C {
    C::Sound(play::Sound {
        sound: (-1, Some((smp_ping::SOUND.to_owned(), None))),
        source: 7,
        x: (at[0] * 8.0) as i32,
        y: (at[1] * 8.0) as i32,
        z: (at[2] * 8.0) as i32,
        volume: 1.0,
        pitch: 1.0,
        seed,
    })
}
