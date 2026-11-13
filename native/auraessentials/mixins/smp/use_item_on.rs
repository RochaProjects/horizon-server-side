// A player uses an item on a block: nothing placed against another's claim
// from outside it.
fn smp_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    face: [i32; 3],
    claimed: &mut bool,
) {
    crate::game::smp_claims_glue::use_item_on(game, stack, pos, face, claimed);
}
