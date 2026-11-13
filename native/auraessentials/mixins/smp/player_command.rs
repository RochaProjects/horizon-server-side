// A command a player typed: a /msg pings its recipient.
fn smp_player_command(player: u128, command: &str) {
    crate::game::smp_ping_glue::command_heard(player, command);
}
