// horizon-menus: draws horizonmatica's menu screen, in the Debug Options screen's place.
fn horizonmatica_screen_draw(game: &mut crate::game::Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    crate::game::horizonmatica_menus::draw(game, batch, crate::game::horizonmatica::screen);
}
