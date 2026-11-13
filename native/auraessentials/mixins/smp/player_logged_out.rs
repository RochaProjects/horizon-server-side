// A player left: where they stood is kept for the operators' `/auraadmin lastpos`.
fn smp_player_logged_out(game: &mut crate::game::Game<'_>, player: u128, name: &str) {
    crate::game::smp_glue::player_left(game, player, name);
}
