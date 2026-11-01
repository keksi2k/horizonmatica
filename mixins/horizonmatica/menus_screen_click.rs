// horizon-menus: a left press on horizonmatica's menu screen.
fn horizonmatica_screen_click(game: &mut crate::game::Game<'_>) {
    crate::game::horizonmatica_menus::hook_click(game);
}
