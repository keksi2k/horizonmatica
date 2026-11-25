// Horizonmatica's support module in the game's `game` module (`crate::game::horizonmatica_game`,
// declared in horizon-mod.toml): what the overlay needs of the game beyond the mod API. It runs
// the overlay's tick with the game's block models and world sections (`horizonmatica::overlay`).
// `super` is `crate::game`.

struct Ops<'a, 'g>(&'a super::Game<'g>);

/// The game's numeric state for an API state.
fn engine(state: mod_api::BlockState) -> Option<horizon_world::registry::BlockStateId> {
    horizon_gameplay::api_values::TryIntoEngine::<()>::try_into_engine(&state, &()).ok()
}

fn ours(v: &horizon_client::mesher::Vertex) -> horizonmatica::overlay::Vertex {
    horizonmatica::overlay::Vertex {
        pos: v.pos,
        uv: v.uv,
        layer: v.layer,
        color: v.color,
        light: v.light,
        normal: v.normal,
        glint_uv: v.glint_uv,
    }
}

impl horizonmatica::overlay::GameOps for Ops<'_, '_> {
    fn white_uv(&self) -> [f32; 2] {
        horizon_client::gfx::lightning::white_uv(&self.0.gui.atlas.font.white())
    }

    fn section(&self, k: [i32; 3]) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        let chunk = self.0.world.chunks.get(&(k[0], k[2]))?;
        let i = chunk.section_index(k[1] * 16)?;
        let section = chunk.sections.get(i)?.clone();
        Some(section as std::sync::Arc<dyn std::any::Any + Send + Sync>)
    }

    fn block_model(&self, state: mod_api::BlockState, at: [f32; 3], light: u32, out: &mut Vec<horizonmatica::overlay::Vertex>) {
        let Some(id) = engine(state) else { return };
        let mut quads = Vec::new();
        self.0.block_model_vertices(id, 0, glam::Mat4::from_translation(glam::Vec3::from_array(at)), light, true, &mut quads);
        out.extend(quads.iter().map(ours));
    }

    fn special_model(&self, state: mod_api::BlockState, at: [f32; 3], out: &mut Vec<horizonmatica::overlay::Vertex>) {
        let Some(id) = engine(state) else { return };
        let mut quads = Vec::new();
        self.0.special_block_model(id, glam::Mat4::from_translation(glam::Vec3::from_array(at)), 0xFF, &mut quads);
        out.extend(quads.iter().map(ours));
    }

    fn solid_cube(&self, state: mod_api::BlockState) -> bool {
        engine(state).is_some_and(|id| self.0.res.facts().get(id).solid_render)
    }
}

// ---------------------------------------------------------------- the hands (printer, easy place)

/// The player's hands, for `horizonmatica::hands`.
struct Hands<'a, 'g>(&'a mut super::Game<'g>);

fn face_dir(f: [i32; 3]) -> horizon_core::geometry::Direction {
    match f {
        [0, 1, 0] => horizon_core::geometry::Direction::Up,
        [0, -1, 0] => horizon_core::geometry::Direction::Down,
        [0, 0, -1] => horizon_core::geometry::Direction::North,
        [0, 0, 1] => horizon_core::geometry::Direction::South,
        [-1, 0, 0] => horizon_core::geometry::Direction::West,
        _ => horizon_core::geometry::Direction::East,
    }
}

fn our_hit(h: &super::Hit) -> horizonmatica::hands::Hit {
    horizonmatica::hands::Hit { pos: h.pos, face: h.face, point: h.point }
}

impl Hands<'_, '_> {
    /// Tells the server the player looks along `yaw`/`pitch` (`ServerboundMovePlayerPacket.Rot`).
    fn send_look(&self, yaw: f32, pitch: f32) {
        let (on_ground, collision) = self.0.player.as_ref().map_or((false, false), |p| (p.ground.on_ground, p.collision.horizontal));
        self.0.send(horizon_protocol::packets::play::Serverbound::MovePlayerRot(horizon_protocol::packets::play::MovePlayerRot {
            yaw,
            pitch,
            flags: u8::from(on_ground) | u8::from(collision) << 1,
        }));
    }
}

impl horizonmatica::hands::Hands for Hands<'_, '_> {
    fn host(&self) -> &dyn mod_api::ClientHost {
        &*self.0
    }

    fn playing(&self) -> bool {
        let host: &dyn mod_api::ClientHost = &*self.0;
        host.playing()
    }

    fn eye(&self) -> Option<[f64; 3]> {
        self.0.eye_position()
    }

    fn look(&self) -> (f32, f32) {
        (self.0.yaw, self.0.pitch)
    }

    fn aim(&self) -> Option<([f64; 3], [f64; 3])> {
        self.0.aim()
    }

    fn reach(&self) -> f64 {
        self.0.reach()
    }

    fn remote(&self) -> bool {
        self.0.remote.is_some()
    }

    fn creative(&self) -> bool {
        self.0.creative
    }

    fn spectator(&self) -> bool {
        self.0.spectator()
    }

    fn using(&self) -> bool {
        self.0.using.is_some()
    }

    fn sneaking(&self) -> bool {
        self.0.keys.raw(&winit::keyboard::KeyCode::ShiftLeft) || self.0.keys.raw(&winit::keyboard::KeyCode::ShiftRight)
    }

    fn entity_targeted(&self) -> bool {
        self.0.pick_entity_hit().is_some()
    }

    fn selected(&self) -> usize {
        self.0.selected
    }

    fn slot_item(&self, i: usize) -> Option<String> {
        self.0.inventory.get(i).and_then(Option::as_ref).map(|s| s.item.to_string())
    }

    fn clip_ray(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<horizonmatica::hands::Hit> {
        self.0.clip_ray(origin, dir, reach).as_ref().map(our_hit)
    }

    fn facts(&self, p: [i32; 3]) -> horizonmatica::hands::Facts {
        let id = self.0.world.block(p[0], p[1], p[2]);
        let facts = self.0.res.facts();
        let f = facts.get(id);
        horizonmatica::hands::Facts {
            air: facts.is_air(id),
            replaceable: f.replaceable,
            no_outline: f.outline.is_empty(),
            fluid: f.fluid != horizon_client::blocks::Fluid::None && !f.waterlogged,
            name: f.name.to_string(),
        }
    }

    fn outline(&self, p: [i32; 3]) -> std::sync::Arc<[[f64; 6]]> {
        let id = self.0.world.block(p[0], p[1], p[2]);
        self.0.res.facts().get(id).outline.clone()
    }

    fn places_block(&self, item: &str) -> bool {
        self.0.pack.assets.registry.default_state(item).is_some()
    }

    fn placement(&mut self, hit: &horizonmatica::hands::Hit, yaw: f32, pitch: f32, sneak: bool, item: &str) -> Option<([i32; 3], mod_api::BlockState)> {
        let ctx = horizon_world::level::PlaceContext {
            clicked_pos: horizon_core::position::BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
            face: face_dir(hit.face),
            hit: horizon_core::geometry::Vec3::new(hit.point[0], hit.point[1], hit.point[2]),
            inside: false,
            yaw,
            pitch,
            sneaking: sneak,
            item: item.to_string(),
            can_use_game_master_blocks: self.0.can_use_game_master_blocks(),
        };
        let preview = self.0.with_level(|b, level, ticks| b.placement_preview(level, ticks, &ctx))?;
        let state = horizon_gameplay::api_values::TryIntoApi::try_into_api(&preview.state).ok()?;
        Some(([preview.pos.x, preview.pos.y, preview.pos.z], state))
    }

    fn click(&mut self, yaw: f32, pitch: f32, hit: &horizonmatica::hands::Hit, sneak: bool) -> Result<(), String> {
        let forced = super::Hit { pos: hit.pos, face: hit.face, point: hit.point, inside: false };
        if self.0.remote.is_some() {
            // The look goes out before the click; the real one goes back the next tick
            // (`restore_look`), and the forced hit stays until then (the click is made with the
            // client's input this tick).
            self.send_look(yaw, pitch);
            self.0.forced_hit = Some(forced);
            let _ = self.0.play_mouse(winit::event::MouseButton::Right, true);
            let _ = self.0.play_mouse(winit::event::MouseButton::Right, false);
            return Ok(());
        }
        let before = (self.0.yaw, self.0.pitch);
        // Sneaking is read from the held Shift key: held for a sneaking click, let go otherwise.
        let shift = winit::keyboard::KeyCode::ShiftLeft;
        let shift_held = self.0.keys.raw(&shift);
        if sneak {
            self.0.keys.insert(shift);
        } else {
            self.0.keys.remove(&shift);
        }
        self.0.yaw = yaw;
        self.0.pitch = pitch;
        self.0.forced_hit = Some(forced);
        let used = self.0.use_with_hand();
        self.0.forced_hit = None;
        self.0.use_delay = 4;
        self.0.yaw = before.0;
        self.0.pitch = before.1;
        if shift_held {
            self.0.keys.insert(shift);
        } else {
            self.0.keys.remove(&shift);
        }
        used.map_err(|e| e.to_string())
    }

    fn restore_look(&mut self) {
        self.0.forced_hit = None;
        let (yaw, pitch) = (self.0.yaw, self.0.pitch);
        self.send_look(yaw, pitch);
    }

    fn select(&mut self, slot: usize) {
        self.0.selected = slot;
    }

    fn creative_set(&mut self, slot: usize, item: &str) {
        if let Some(s) = self.0.inventory.get_mut(slot) {
            *s = Some(horizon_client::items::ItemStack::new(item, 1));
        }
    }

    fn take_in_hand(&mut self, item: &str) -> Result<(), String> {
        let game = &mut *self.0;
        game.select_queued_hotbar();
        if let Some(slot) = (0..9).find(|&i| game.inventory[i].as_ref().is_some_and(|s| &*s.item == item)) {
            game.selected = slot;
            return Ok(());
        }
        if game.creative {
            horizon_client::pick_item::pick(&mut game.inventory[..36], &mut game.selected, &horizon_client::items::ItemStack::new(item, 1), true);
            return Ok(());
        }
        let Some(slot) = (9..36).find(|&i| game.inventory[i].as_ref().is_some_and(|s| &*s.item == item)) else {
            return Err("none carried".into());
        };
        // `getSuitableHotbarSlot`: from the selected slot on, an empty one, else one without enchantments.
        let from = game.selected;
        let order = (0..9).map(|i| (from + i) % 9);
        let hotbar = order
            .clone()
            .find(|&i| game.inventory[i].is_none())
            .or_else(|| order.clone().find(|&i| game.inventory[i].as_ref().is_some_and(|s| s.enchantments().is_empty())))
            .unwrap_or(from);
        if game.remote.is_some() {
            if !game.remote_menu_ops(&[(Some(slot), hotbar as i32, horizon_client::menu::Input::Swap)]) {
                return Err("the inventory can't be changed right now".into());
            }
        } else {
            game.inventory.swap(hotbar, slot);
        }
        game.selected = hotbar;
        Ok(())
    }

    fn release_use(&mut self) {
        self.0.right = false;
    }

    fn use_held(&self) -> bool {
        self.0.right
    }
}

/// Once a client tick (the `tick_pre` mixin).
pub fn tick(game: &mut super::Game<'_>) {
    horizonmatica::fix::install(fix_state, horizon_world::DATA_VERSION, legacy_state);
    horizonmatica::memory::install(total_ram);
    {
        let game = &*game;
        horizonmatica::overlay::tick(game, &Ops(game));
    }
    horizonmatica::hands::tick(&mut Hands(game));
}

/// The computer's RAM, as the game's debug screen reads it.
fn total_ram() -> Option<u64> {
    horizon_threads::process_memory::usage().map(|[_, _, ram]| ram)
}

/// A mouse button while playing (the `mouse` mixin): whether the mod took it.
pub fn mouse(game: &mut super::Game<'_>, button: winit::event::MouseButton, pressed: bool) -> bool {
    let b = match button {
        winit::event::MouseButton::Left => 0,
        winit::event::MouseButton::Right => 1,
        winit::event::MouseButton::Middle => 2,
        _ => return false,
    };
    horizonmatica::hands::mouse(&mut Hands(game), b, pressed)
}

// ---------------------------------------------------------------- old schematics

/// A 1.12 `blockId << 4 | data` as a 1.13 state (`horizonmatica::fix::Legacy`): the game's data
/// fixer turns an old falling block's `TileID`/`Data` into its `BlockState` with vanilla's
/// `BlockStateData` table.
fn legacy_state(id: u32) -> Option<(String, Vec<(String, String)>)> {
    use horizon_core::nbt::Tag;
    let mut tag = Tag::compound([
        ("id", Tag::String("minecraft:falling_block".into())),
        ("TileID", Tag::Int((id >> 4) as i32)),
        ("Data", Tag::Byte((id & 0x0F) as i8)),
    ]);
    // 1.12.2's data version, up to the flattening (1.13's first snapshots).
    horizon_world::datafix::update(horizon_world::datafix::schema::Ty::Entity, &mut tag, 1343, 1451);
    state_of(tag.get("BlockState")?)
}

/// One block state through the game's data fixer (`horizonmatica::fix::Fixer`).
fn fix_state(name: &str, properties: &[(String, String)], from: i32) -> Option<(String, Vec<(String, String)>)> {
    let name_tag = horizon_core::nbt::Tag::String(name.into());
    let mut tag = if properties.is_empty() {
        horizon_core::nbt::Tag::compound([("Name", name_tag)])
    } else {
        let props = properties.iter().map(|(k, v)| (k.as_str(), horizon_core::nbt::Tag::String(v.as_str().into())));
        horizon_core::nbt::Tag::compound([("Name", name_tag), ("Properties", horizon_core::nbt::Tag::compound(props))])
    };
    horizon_world::datafix::update(horizon_world::datafix::schema::Ty::BlockState, &mut tag, from, horizon_world::DATA_VERSION);
    state_of(&tag)
}

/// A block state tag as name and properties.
fn state_of(tag: &horizon_core::nbt::Tag) -> Option<(String, Vec<(String, String)>)> {
    // The fixer's newest block state format names the fields `id` and `properties`.
    let Some(horizon_core::nbt::Tag::String(name)) = tag.get("id").or_else(|| tag.get("Name")) else { return None };
    let mut out = Vec::new();
    if let Some(horizon_core::nbt::Tag::Compound(props)) = tag.get("properties").or_else(|| tag.get("Properties")) {
        for (k, v) in props {
            if let horizon_core::nbt::Tag::String(v) = v {
                out.push((k.to_string_lossy(), v.to_string_lossy()));
            }
        }
    }
    Some((name.to_string_lossy(), out))
}
