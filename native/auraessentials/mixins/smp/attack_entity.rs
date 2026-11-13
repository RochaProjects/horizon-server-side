// A player hits an entity: a claim's frames, armor stands and animals are
// kept from players its owner doesn't trust.
fn smp_attack_entity(
    game: &mut crate::game::Game<'_>,
    kind: &str,
    pos: [f64; 3],
    canceled: &mut bool,
) {
    crate::game::smp_glue::attack(game, kind, pos, canceled);
}
