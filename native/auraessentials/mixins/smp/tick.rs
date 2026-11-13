// The end of each server tick: the SMP's accounts and files.
fn smp_server_tick(
    game: &mut crate::game::Game<'_>,
    phase: crate::game::native_lifecycle::ServerTickPhase,
) {
    if matches!(
        phase,
        crate::game::native_lifecycle::ServerTickPhase::ServerTickEnd { .. }
    ) {
        {
            let _span = horizon_client::spans::guard("smp_glue");
            crate::game::smp_glue::tick(game);
        }
        {
            let _span = horizon_client::spans::guard("smp_invsee");
            crate::game::smp_invsee_glue::tick(game);
        }
        {
            let _span = horizon_client::spans::guard("smp_ping");
            crate::game::smp_ping_glue::tick(game);
        }
    }
}
