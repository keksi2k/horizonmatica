// horizon-menus: the left button let go on horizonmatica's menu screen.
fn horizonmatica_screen_release(game: &mut crate::game::Game<'_>) {
    crate::game::horizonmatica_menus::hook_release(game);
}
