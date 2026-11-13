pub(super) fn aura_essentials_resource_pack_status(
    game: &mut crate::game::Game<'_>,
    player: u128,
    pack: u128,
    status: i32,
) {
    crate::game::smp_glue::resource_pack_status(game, player, pack, status);
}
