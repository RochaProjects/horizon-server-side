// A player right clicks a block: kept out of the land others claimed.
fn smp_right_click_block(game: &mut crate::game::Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    crate::game::smp_glue::guard(game, pos, canceled);
}
