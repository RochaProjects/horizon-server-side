// A player's command on the server: the SMP's own, when it is one.
fn smp_perform_command(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    if !*ran {
        crate::game::smp_ping_glue::perform(game, player, command, ran);
    }
    if !*ran {
        crate::game::smp_invsee_glue::perform(game, player, command, ran);
    }
    if !*ran {
        crate::game::smp_glue::perform(game, player, command, ran);
    }
}
