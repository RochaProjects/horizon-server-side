// An entity joins the level: where a projectile left from.
fn smp_entity_join_level(entity: u32, kind: &str, pos: [f64; 3], loaded: bool) {
    crate::game::smp_claims_glue::joined(entity, kind, pos, loaded);
}
