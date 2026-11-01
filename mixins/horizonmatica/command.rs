// `/horizonmatica ...` and `/hm ...`, run on the client before the server sees them.
fn horizonmatica_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    if !*ran && game.native_on(crate::game::horizonmatica::ID) && crate::game::horizonmatica::command(game, command) {
        *ran = true;
    }
}
