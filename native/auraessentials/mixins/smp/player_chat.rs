// A player's chat line: queued for the SMP's pings, read at the tick's end.
fn smp_player_chat(player: u128, message: &str) {
    crate::smp_chat::heard(player, message);
}
