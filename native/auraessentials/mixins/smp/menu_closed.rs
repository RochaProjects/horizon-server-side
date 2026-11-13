// A menu a native mod opened closed: a trade's ends, nothing moved.
fn smp_menu_closed(game: &mut crate::game::Game<'_>, uuid: u128, window: i32) {
    if !crate::game::smp_invsee_glue::menu_closed(game, uuid, window) {
        crate::game::smp_glue::menu_closed(game, uuid, window);
    }
}
