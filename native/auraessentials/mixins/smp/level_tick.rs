// After a level's tick: in the Overworld, the `/rtp`s waiting for their land
// read it (the game reads only the level it ticks).
fn smp_level_tick_post(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::smp_glue::level_ticked(game, dimension);
}
