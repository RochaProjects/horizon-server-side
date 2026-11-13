// A dispenser acts on the block in front: not into another's claim.
fn smp_dispense_item(
    dimension: &str,
    pos: [i32; 3],
    facing: horizon_core::geometry::Direction,
    stack: &horizon_client::items::ItemStack,
    answer: &mut Option<horizon_client::mod_api::NativeDispensed>,
) {
    crate::game::smp_claims_glue::dispense(dimension, pos, facing.normal(), stack, answer);
}
