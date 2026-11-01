// The mod's keys (Options > Mods lists and rebinds them).
fn horizonmatica_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::horizonmatica::key(game, key, pressed);
}
