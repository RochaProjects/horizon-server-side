// Fire is about to be set: lava outside a claim lights nothing in it.
fn smp_block_ignite(
    dimension: crate::game::LevelDimension,
    pos: horizon_core::position::BlockPos,
    cause: &str,
    cancel: &mut bool,
) {
    let dimension = crate::game::smp_glue::dimension_id(dimension);
    crate::game::smp_claims_glue::ignite(dimension, [pos.x, pos.y, pos.z], cause, cancel);
}
