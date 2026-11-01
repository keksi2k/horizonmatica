// horizon-menus: horizonmatica's menu screen doesn't pause the game.
fn horizonmatica_screen_pauses(game: &crate::game::Game<'_>) -> std::ops::ControlFlow<bool> {
    crate::game::horizonmatica_menus::hook_pauses(game)
}
