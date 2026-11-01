// The mouse wheel: in move mode it moves the schematic (and is kept from the hotbar).
fn horizonmatica_wheel(game: &mut crate::game::Game<'_>, steps: f32, claimed: &mut bool) {
    if !*claimed {
        crate::game::horizonmatica::wheel(game, steps, claimed);
    }
}
