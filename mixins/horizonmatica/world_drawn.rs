// The world is drawn: the overlay goes on top (see bridge.rs).
fn horizonmatica_world_drawn(world: &mut crate::gfx::native_render::WorldDrawn<'_>) {
    crate::gfx::native_render::horizonmatica_bridge::draw(world);
}
