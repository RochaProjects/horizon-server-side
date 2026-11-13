// The SMP hears projectiles' impacts (claims).
fn smp_hears_projectile_impacts(game: &crate::game::Game<'_>, hears: &mut bool) {
    crate::game::smp_claims_glue::hears(game, hears);
}
