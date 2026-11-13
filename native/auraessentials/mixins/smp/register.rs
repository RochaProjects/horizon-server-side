// The SMP's commands in every player's command tree.
fn smp_register_commands(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    crate::game::smp_glue::commands(commands);
    crate::game::smp_invsee_glue::commands(commands);
    crate::game::smp_ping_glue::commands(commands);
}
