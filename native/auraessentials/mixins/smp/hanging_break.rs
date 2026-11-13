// A painting or frame is about to break: not in another's claim by a player
// or a blast.
fn smp_hanging_break(
    game: &mut crate::game::Game<'_>,
    player: Option<u128>,
    pos: [i32; 3],
    cancel: &mut bool,
) {
    crate::game::smp_claims_glue::hanging_break(game, player, pos, cancel);
}
