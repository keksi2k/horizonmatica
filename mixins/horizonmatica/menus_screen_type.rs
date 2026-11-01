// horizon-menus: text from the input method on horizonmatica's menu screen.
fn horizonmatica_screen_type(game: &mut crate::game::Game<'_>, typed: &str) {
    crate::game::horizonmatica_menus::hook_type(game, typed);
}
