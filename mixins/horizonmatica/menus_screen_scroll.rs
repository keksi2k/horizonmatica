// horizon-menus: the wheel on horizonmatica's menu screen.
fn horizonmatica_screen_scroll(game: &mut crate::game::Game<'_>, delta: f32) {
    crate::game::horizonmatica_menus::hook_scroll(game, delta);
}
