// horizon-menus: the Debug Options screen closed (Esc, F3+F6): so did horizonmatica's menu.
fn horizonmatica_screen_close(game: &mut crate::game::Game<'_>) {
    crate::game::horizonmatica_menus::hook_close(game);
}
