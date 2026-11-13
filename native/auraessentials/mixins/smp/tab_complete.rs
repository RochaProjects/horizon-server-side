fn aura_essentials_tab_complete(
    game: &mut crate::game::Game<'_>,
    player: u128,
    name: &str,
    label: &str,
    arguments: &[String],
    range: [usize; 2],
    suggestions: &mut Vec<horizon_client::mod_api::Suggestion>,
) {
    let _ = name;
    crate::game::smp_glue::home_suggestions(game, player, label, arguments, range, suggestions);
}
