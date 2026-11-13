// A fluid flows or fire spreads into a block: kept out of the land others
// claimed.
fn smp_block_spread(
    dimension: crate::game::LevelDimension,
    from: horizon_core::position::BlockPos,
    to: horizon_core::position::BlockPos,
    canceled: &mut bool,
) {
    let dimension = crate::game::smp_glue::dimension_id(dimension);
    let (from, to) = ([from.x, from.y, from.z], [to.x, to.y, to.z]);
    if !crate::game::smp_glue::world_allows(dimension, from, &[to]) {
        *canceled = true;
    }
}
