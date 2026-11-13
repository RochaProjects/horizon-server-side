// A piston pushes, pulls or destroys blocks: kept out of the land others
// claimed.
fn smp_piston_move(
    dimension: crate::game::LevelDimension,
    piston: horizon_core::position::BlockPos,
    direction: [i32; 3],
    push: &[horizon_core::position::BlockPos],
    destroy: &[horizon_core::position::BlockPos],
    canceled: &mut bool,
) {
    let dimension = crate::game::smp_glue::dimension_id(dimension);
    let step = |p: &horizon_core::position::BlockPos| {
        [p.x + direction[0], p.y + direction[1], p.z + direction[2]]
    };
    let touched: Vec<[i32; 3]> = push
        .iter()
        .flat_map(|p| [[p.x, p.y, p.z], step(p)])
        .chain(destroy.iter().map(|p| [p.x, p.y, p.z]))
        .collect();
    let piston = [piston.x, piston.y, piston.z];
    if !crate::game::smp_glue::world_allows(dimension, piston, &touched) {
        *canceled = true;
    }
}
