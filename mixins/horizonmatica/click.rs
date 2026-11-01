// A click on one of the settings page's buttons.
fn horizonmatica_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    if mod_id == crate::game::horizonmatica::ID {
        crate::game::horizonmatica::settings_click(game, button);
    }
}
