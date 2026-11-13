// The world closing: the SMP writes its files and lets its state go.
fn smp_world_lifecycle(phase: crate::game::native_lifecycle::WorldPhase) {
    use crate::game::native_lifecycle::WorldPhase;
    if matches!(
        phase,
        WorldPhase::WorldCloseFinal | WorldPhase::WorldCloseFailed
    ) {
        crate::game::smp_glue::closed();
    }
}
