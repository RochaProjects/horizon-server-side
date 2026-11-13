// A player uses an entity: a claim's animals, frames and stands keep to
// its members.
fn smp_interact_entity(
    game: &mut crate::game::Game<'_>,
    entity: i32,
    kind: &str,
    claimed: &mut bool,
) {
    crate::game::smp_claims_glue::interact_entity(game, entity, kind, claimed);
}
