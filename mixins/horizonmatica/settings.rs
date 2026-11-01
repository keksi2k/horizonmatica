// The mod's settings page.
fn horizonmatica_settings(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id == crate::game::horizonmatica::ID {
        crate::game::horizonmatica::settings_buttons(game, buttons);
    }
}
