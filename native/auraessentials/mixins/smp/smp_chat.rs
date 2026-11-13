// The SMP's chat pings (horizon-mods native/aura, smp_ping.rs): the hub
// hears a player's chat line (`native_player_chat`) away from the game, so
// the line is only queued here, and the SMP reads the queue at the end of
// the tick. Added to the client library as `smp_chat`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

static LINES: Mutex<Vec<(u128, String)>> = Mutex::new(Vec::new());
static WAITING: AtomicBool = AtomicBool::new(false);
/// The most lines kept for one tick.
const MAX: usize = 64;

/// A chat line `sender` sent.
pub fn heard(sender: u128, message: &str) {
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if lines.len() < MAX {
        lines.push((sender, message.chars().take(256).collect()));
        WAITING.store(true, Relaxed);
    }
}

/// The lines sent since the last call: an atomic load when there are none.
pub fn take() -> Vec<(u128, String)> {
    if !WAITING.swap(false, Relaxed) {
        return Vec::new();
    }
    std::mem::take(&mut *LINES.lock().unwrap_or_else(|e| e.into_inner()))
}
