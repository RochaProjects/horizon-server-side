// A player breaks a block: kept out of the land others claimed.
fn smp_destroy_block(game: &mut crate::game::Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    crate::game::smp_glue::guard(game, pos, canceled);
}
