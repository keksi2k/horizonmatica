// The info box and the material list.
fn horizonmatica_hud(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {
    crate::game::horizonmatica::hud(game, batch, corners);
}
