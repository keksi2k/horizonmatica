// Each game tick: verifier scan, overlay rebuild, printer.
fn horizonmatica_tick(game: &mut crate::game::Game<'_>) {
    crate::game::horizonmatica::tick(game);
}
