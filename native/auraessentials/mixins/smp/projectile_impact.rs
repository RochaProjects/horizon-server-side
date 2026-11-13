// A projectile hits something: in another's claim, only a member's shot acts.
fn smp_projectile_impact(
    game: &mut crate::game::Game<'_>,
    dimension: &str,
    projectile: u32,
    owner: u32,
    entity: Option<u32>,
    entity_kind: &str,
    block: [i32; 3],
    at: [f64; 3],
    canceled: &mut bool,
) {
    crate::game::smp_claims_glue::impact(
        game,
        dimension,
        projectile,
        owner,
        entity,
        entity_kind,
        block,
        at,
        canceled,
    );
}
