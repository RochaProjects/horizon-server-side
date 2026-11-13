// A player fills or empties a bucket: kept out of the land others claimed.
fn smp_fill_bucket(game: &mut crate::game::Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    crate::game::smp_glue::guard(game, pos, canceled);
}
