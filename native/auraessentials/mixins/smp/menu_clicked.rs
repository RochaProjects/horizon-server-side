// A click in a menu a native mod opened: the SMP's trade menu, when it is one.
fn smp_menu_clicked(game: &mut crate::game::Game<'_>, uuid: u128, window: i32, slot: i32, mode: i32) {
    if !crate::game::smp_invsee_glue::menu_clicked(game, uuid, window, slot, mode) {
        crate::game::smp_glue::menu_clicked(game, uuid, window, slot, mode);
    }
}
