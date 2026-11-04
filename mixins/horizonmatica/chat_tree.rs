// The tree the chat completes with, including the mod's subcommands.
fn horizonmatica_chat_tree(game: &mut crate::game::Game<'_>, base: Option<&horizon_client::command::Dispatcher>) -> std::ops::ControlFlow<horizon_client::command::Dispatcher> {
    if game.native_on(crate::game::horizonmatica::ID) {
        std::ops::ControlFlow::Break(crate::game::horizonmatica::chat_tree(game, base))
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
