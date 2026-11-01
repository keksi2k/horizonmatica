// Each frame: keeps the renderer's options in step and clears it outside a world.
fn horizonmatica_frame(game: &mut crate::game::Game<'_>) {
    crate::game::horizonmatica::frame(game);
}
