//! What the mod does with the player's hands: the printer (places what the schematic wants within
//! reach), easy place (right click places the block the schematic wants where the view meets it)
//! and the middle click on a wanted block (takes that block in hand).
//!
//! A click must carry its own look and land on a chosen face, and whether it gives the block
//! wanted is asked of the game's own placement; the mod API has neither, so the game side
//! (`src/native/game_support.rs`) gives them as `Hands` each tick and on each mouse button.

use crate::i18n::item_name;
use crate::runtime;
use crate::world::{StateId, World};
use horizonmatica_core::verify::{self, Status};
use horizonmatica_core::{BlockState, materials, printer};
use mod_api::ClientHost;
use std::cell::RefCell;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Time the printer may take per tick.
const PRINT_BUDGET: Duration = Duration::from_millis(3);

/// The block a click aims at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub pos: [i32; 3],
    pub face: [i32; 3],
    pub point: [f64; 3],
}

/// The facts of a block in the client world that a click depends on.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub air: bool,
    pub replaceable: bool,
    /// No outline: nothing to click (light, structure void...).
    pub no_outline: bool,
    /// Water or lava itself (not a waterlogged block).
    pub fluid: bool,
    pub name: String,
}

/// The player's hands as the game side gives them.
pub trait Hands {
    fn host(&self) -> &dyn ClientHost;
    /// Playing in a world (no screen open).
    fn playing(&self) -> bool;
    fn eye(&self) -> Option<[f64; 3]>;
    /// The player's yaw and pitch.
    fn look(&self) -> (f32, f32);
    /// The eye and view direction.
    fn aim(&self) -> Option<([f64; 3], [f64; 3])>;
    /// Block reach.
    fn reach(&self) -> f64;
    /// Through a server (the integrated one of a single-player world too).
    fn remote(&self) -> bool;
    fn creative(&self) -> bool;
    fn spectator(&self) -> bool;
    /// An item is being used (eaten, drawn...).
    fn using(&self) -> bool;
    fn sneaking(&self) -> bool;
    /// An entity is under the crosshair.
    fn entity_targeted(&self) -> bool;
    fn selected(&self) -> usize;
    /// The item in main inventory slot `i` (0..36, 0..9 the hotbar).
    fn slot_item(&self, i: usize) -> Option<String>;
    /// The first block along a ray, within `reach`.
    fn clip_ray(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<Hit>;
    fn facts(&self, p: [i32; 3]) -> Facts;
    /// Outline boxes in block-local coordinates.
    fn outline(&self, p: [i32; 3]) -> std::sync::Arc<[[f64; 6]]>;
    /// Whether `item` places a block.
    fn places_block(&self, item: &str) -> bool;
    /// What a click on `hit` with `item` in hand would place, and where.
    fn placement(&mut self, hit: &Hit, yaw: f32, pitch: f32, sneak: bool, item: &str) -> Option<([i32; 3], StateId)>;
    /// Uses the item in hand on `hit`, looking along `yaw`/`pitch`, sneaking or not. The camera
    /// doesn't move: locally the look is back before the next frame; on a server the look is sent
    /// just before the click (`restore_look` the next tick).
    fn click(&mut self, yaw: f32, pitch: f32, hit: &Hit, sneak: bool) -> Result<(), String>;
    /// After a click on a server: the player's real look sent back.
    fn restore_look(&mut self);
    fn select(&mut self, slot: usize);
    /// Puts one `item` in hotbar slot `slot` (creative).
    fn creative_set(&mut self, slot: usize, item: &str);
    /// Takes `item` in hand as the game's pick block does.
    fn take_in_hand(&mut self, item: &str) -> Result<(), String>;
    /// Lets go of the use button (the game's own held use stops).
    fn release_use(&mut self);
    /// The use button is held.
    fn use_held(&self) -> bool;
}

/// What the hands remember between ticks.
#[derive(Default)]
struct Mind {
    /// On a server, the tick a click went out in: its look is given back once that tick is over
    /// (a click made on a mouse press is only carried out in the next tick).
    server_look: Option<u64>,
    next_print: u64,
    /// The tick each hotbar slot was last used by the printer.
    hotbar_used: [u64; 9],
    /// Easy place: the right button is down; its press was taken (so its release is too); the
    /// tick the next placement may come; the item last asked for and until when that waits.
    right_down: bool,
    easy_taken: bool,
    easy_next: u64,
    easy_pick: Option<(String, u64)>,
    /// The mod's own click is being made: the mouse hook leaves it to the game.
    own_click: bool,
    /// The middle button's press was taken (so its release is too).
    middle_taken: bool,
}

thread_local! {
    static MIND: RefCell<Mind> = RefCell::new(Mind::default());
}

fn mind<R>(f: impl FnOnce(&mut Mind) -> R) -> R {
    MIND.with_borrow_mut(f)
}

fn now() -> u64 {
    runtime::with(|s| s.ticks)
}

fn status_at(h: &dyn Hands, p: [i32; 3]) -> Option<(Status, BlockState)> {
    let w = World::new(h.host());
    runtime::with(|s| runtime::status_at(&w, s, p))
}

fn item_for(h: &dyn Hands, wanted: &BlockState) -> Option<String> {
    let w = World::new(h.host());
    runtime::with(|s| runtime::item_for(&w, s, wanted))
}

fn wanted_id(h: &dyn Hands, wanted: &BlockState) -> Option<StateId> {
    let w = World::new(h.host());
    runtime::with(|s| runtime::wanted_id(&w, s, wanted))
}

/// The block in the world at `p` with its properties.
fn actual(h: &dyn Hands, p: [i32; 3]) -> Option<BlockState> {
    let w = World::new(h.host());
    let id = w.block(p)?;
    Some(runtime::with(|s| s.blocks.state(&w, id)))
}

fn state_of(h: &dyn Hands, id: StateId) -> BlockState {
    let w = World::new(h.host());
    runtime::with(|s| s.blocks.state(&w, id))
}

fn loaded(h: &dyn Hands, p: [i32; 3]) -> bool {
    World::new(h.host()).loaded(p)
}

fn held(h: &dyn Hands) -> Option<String> {
    h.slot_item(h.selected())
}

/// Minecraft's yaw and pitch facing from `eye` towards `at`.
fn yaw_pitch(eye: [f64; 3], at: [f64; 3]) -> (f32, f32) {
    let d = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
    let horizontal = (d[0] * d[0] + d[2] * d[2]).sqrt();
    ((-d[0]).atan2(d[2]).to_degrees() as f32, (-d[1]).atan2(horizontal).to_degrees() as f32)
}

// ---------------------------------------------------------------- each tick

/// Once a client tick, from the game side.
pub fn tick(h: &mut dyn Hands) {
    if !World::new(h.host()).in_world() || !runtime::with(|s| s.settings_ready()) {
        return;
    }
    printer_step(h);
    easy_place_step(h);
}

/// Blocks whose use would open a screen or change them, which the printer must not click.
fn interactive(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "minecraft:crafting_table", "minecraft:crafter", "minecraft:lever", "minecraft:repeater", "minecraft:comparator",
        "minecraft:note_block", "minecraft:daylight_detector", "minecraft:anvil", "minecraft:chipped_anvil",
        "minecraft:damaged_anvil", "minecraft:enchanting_table", "minecraft:grindstone", "minecraft:loom",
        "minecraft:cartography_table", "minecraft:smithing_table", "minecraft:stonecutter", "minecraft:cake",
        "minecraft:bell", "minecraft:respawn_anchor", "minecraft:composter", "minecraft:cauldron",
        "minecraft:water_cauldron", "minecraft:lava_cauldron", "minecraft:jukebox", "minecraft:lectern",
        "minecraft:redstone_wire", "minecraft:dragon_egg", "minecraft:beacon", "minecraft:flower_pot",
        "minecraft:chiseled_bookshelf", "minecraft:decorated_pot", "minecraft:vault", "minecraft:trial_spawner",
    ];
    EXACT.contains(&name)
        || [
            "_door", "_trapdoor", "_fence_gate", "_button", "_bed", "_sign", "_hanging_sign", "_shulker_box", "chest", "barrel", "furnace",
            "smoker", "hopper", "dispenser", "dropper", "potted_", "candle", "brewing_stand", "_cake",
        ]
        .iter()
        .any(|p| name.contains(p))
}

/// What the printer does at a spot.
#[derive(Clone, Debug)]
enum Job {
    /// Place the missing block.
    Place,
    /// Click the block that is there with more of its item (half slab to double, candles...).
    Stack,
    /// Click the placed block this many times to set it (repeater delay, comparator mode...).
    Adjust(u32),
}

/// A click: the look the server is told, where it lands, and whether it is made sneaking.
struct Click {
    yaw: f32,
    pitch: f32,
    hit: Hit,
    sneak: bool,
}

fn printer_step(h: &mut dyn Hands) {
    let now = now();
    // On a server, the click went out in an earlier tick: give it the player's real look back.
    if mind(|m| m.server_look.is_some_and(|t| t < now)) {
        mind(|m| m.server_look = None);
        h.restore_look();
    }
    let (on, settings) = runtime::with(|s| (s.settings.printer_on && s.slots.iter().any(|x| x.shown) && !s.moving, s.settings.printer));
    if !on || !h.playing() || now < mind(|m| m.next_print) {
        return;
    }
    let Some(eye) = h.eye() else { return };
    let reach = settings.reach.min(h.reach());
    let layer = runtime::with(|s| s.layer);
    let feet = [eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32];

    // Everything there is to do in reach, bottom up and nearest first...
    let mut jobs: Vec<(u8, bool, i32, bool, usize, [i32; 3], BlockState, Job)> = Vec::new();
    let held = held(h);
    let mut waiting = None;
    let mut unsupported = 0usize;
    let mut pillar = None;
    let mut stuck = 0usize;
    for (order, p) in printer::positions_in_reach(eye, reach).into_iter().enumerate() {
        if !layer.contains(p[1]) || !loaded(h, p) || runtime::with(|s| s.recent.blocked(p, now, settings.retry_ticks)) {
            continue;
        }
        let Some((status, wanted)) = status_at(h, p) else { continue };
        if wanted.is_air() || matches!(wanted.name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column") {
            continue;
        }
        let mut needs_air = false;
        let job = match status {
            Status::Missing => {
                // The other half of a door or bed comes with its first half.
                if materials::items_for_state(&wanted) == 0 || p == feet || p == [feet[0], feet[1] + 1, feet[2]] {
                    continue;
                }
                // Sand, gravel, anvils...: only onto something, or they fall.
                if printer::falls(&wanted) {
                    let below = [p[0], p[1] - 1, p[2]];
                    let f = h.facts(below);
                    if f.air || f.replaceable || f.fluid {
                        let comes = status_at(h, below).is_some_and(|(_, w)| !w.is_air());
                        waiting = Some(if comes {
                            tr!("printer.waits_below", item_name(&wanted.name), p[0], p[1], p[2])
                        } else {
                            tr!("printer.would_fall", item_name(&wanted.name), p[0], p[1], p[2])
                        });
                        continue;
                    }
                }
                // An observer waits for the block it watches, or placing that would fire it;
                // not when that block couldn't be placed.
                if let Some(d) = printer::observed(&wanted) {
                    let watched = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    let stuck = runtime::with(|s| s.recent.blocked(watched, now, settings.retry_ticks));
                    if !stuck && status_at(h, watched).is_some_and(|(st, w)| st == Status::Missing && !w.is_air()) {
                        waiting = Some(tr!("printer.observer", p[0], p[1], p[2]));
                        continue;
                    }
                }
                // No real block with a face in view: without air place it can only wait for one.
                needs_air = !has_support(h, eye, reach, p, &wanted);
                if needs_air && !settings.air_place {
                    unsupported += 1;
                    if pillar.is_none()
                        && let Some(axis) = wanted.prop("axis")
                        && has_support(h, eye, reach, p, &BlockState::air())
                    {
                        pillar = Some(tr!("printer.axis", item_name(&wanted.name), p[0], p[1], p[2], axis));
                    }
                    continue;
                }
                Job::Place
            }
            Status::WrongState => {
                let Some(actual) = actual(h, p) else { continue };
                if verify::upgradable(&wanted, &actual) {
                    Job::Stack
                } else if let Some(n) = verify::config_clicks(&wanted, &actual) {
                    Job::Adjust(n)
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        if !matches!(job, Job::Adjust(_)) {
            let key = stuck_key(h, eye, p, &wanted);
            if runtime::with(|s| s.stuck.holds(p, key, now)) {
                stuck += 1;
                continue;
            }
        }
        let other_item = item_for(h, &wanted) != held;
        jobs.push((printer::place_class(&wanted), needs_air, p[1], other_item, order, p, wanted, job));
    }
    // ...done in redstone's order: plain blocks, then what carries a signal, then what makes one;
    // within that, what has a real block to click against before what needs air place, bottom up,
    // and in each layer what the hand already holds (a change of hand costs a tick), nearest first.
    jobs.sort_by_key(|(class, needs_air, y, other_item, order, ..)| (*class, *needs_air, *y, *other_item, *order));

    let mut note = waiting.or(pillar).unwrap_or_else(|| {
        if unsupported > 0 {
            tr!("printer.unsupported", unsupported)
        } else if stuck > 0 {
            tr!("printer.stuck", stuck)
        } else {
            String::new()
        }
    });
    let deadline = Instant::now() + PRINT_BUDGET;
    for (.., p, wanted, job) in jobs {
        if Instant::now() > deadline {
            break;
        }
        let item = match &job {
            Job::Adjust(_) => None,
            _ => match item_for(h, &wanted) {
                Some(item) => Some(item),
                None => continue,
            },
        };
        if let Some(item) = &item
            && let Err(e) = can_take(h, item)
        {
            note = format!("{}: {e}", item_name(item));
            runtime::with(|s| s.recent.mark(p, now));
            continue;
        }
        // The aim first: it only asks what a click would place, with no need of the item in hand,
        // so a block that can't be placed yet doesn't cost a change of hand and a tick.
        let click = match (&job, &item) {
            (Job::Place, Some(item)) => aim_for(h, eye, reach, p, &wanted, item, settings.air_place, PlacementStage::PrinterFirst),
            (Job::Stack, Some(item)) => aim_stack(h, eye, reach, p, &wanted, item, settings.air_place),
            (Job::Adjust(_), _) => aim_block(h, eye, reach, p),
            _ => None,
        };
        let Some(click) = click else {
            let key = stuck_key(h, eye, p, &wanted);
            runtime::with(|s| s.stuck.mark(p, key, now));
            note = tr!("printer.no_way", item_name(&wanted.name), p[0], p[1], p[2]);
            continue;
        };
        if let Some(item) = &item {
            match select(h, item) {
                Err(e) => {
                    note = format!("{}: {}", item_name(item), hand_error(e));
                    runtime::with(|s| s.recent.mark(p, now));
                    continue;
                }
                // Through a server (also the integrated one of a single-player world) the click
                // would go out before the slot change and place what was in hand before: the
                // change goes out this tick, the click the next.
                Ok(true) if h.remote() => {
                    runtime::with(|s| s.printer_note = tr!("printer.taking", item_name(item)));
                    return;
                }
                Ok(_) => {}
            }
        }
        if let Err(e) = do_click(h, &click) {
            runtime::with(|s| s.printer_note = tr!("printer.click_failed", e));
            return;
        }
        let what = match job {
            Job::Place => tr!("printer.placed", item_name(&wanted.name)),
            Job::Stack => tr!("printer.stacked", item_name(&wanted.name)),
            Job::Adjust(n) => tr!("printer.adjusted", item_name(&wanted.name), n),
        };
        let remote = h.remote();
        runtime::with(|s| {
            // An adjusted block gets its next click soon; on a server once its answer is in.
            if !matches!(job, Job::Adjust(_)) {
                s.placed += 1;
                s.recent.mark(p, now);
            } else if remote {
                s.recent.mark(p, now);
            }
            s.printer_note = tr!("printer.at", what, p[0], p[1], p[2]);
        });
        mind(|m| m.next_print = now + u64::from(settings.interval.max(1)));
        return;
    }
    if !note.is_empty() {
        runtime::with(|s| s.printer_note = note);
    }
}

// ---------------------------------------------------------------- easy place

/// Whether easy place has a say right now.
fn easy_active(h: &dyn Hands) -> bool {
    let (on, shown, moving) = runtime::with(|s| (s.settings.easy_place, s.slots.iter().any(|x| x.shown), s.moving));
    on && shown && !moving && h.playing() && !h.spectator() && !runtime::tool_held(&World::new(h.host()))
}

/// The block the view asks for, for easy place: the player's own two cells can't take one.
fn easy_target(h: &dyn Hands) -> Option<([i32; 3], BlockState)> {
    let eye = h.eye()?;
    let feet = [eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32];
    let (cell, status, wanted) = first_wanted(h, &[feet, [feet[0], feet[1] + 1, feet[2]]])?;
    (status == Status::Missing && materials::items_for_state(&wanted) > 0).then_some((cell, wanted))
}

/// Each tick: the game's own held use is stopped before it places a block, the block looked at
/// is taken in hand, and with the button down the next one goes in.
fn easy_place_step(h: &mut dyn Hands) {
    // A screen took the release of the button with it.
    if !h.playing() {
        mind(|m| {
            m.right_down = false;
            m.easy_taken = false;
        });
        return;
    }
    if !easy_active(h) {
        return;
    }
    if h.use_held() && easy_place_refuses(h) {
        h.release_use();
    }
    if h.using() || easy_select(h) {
        return;
    }
    let (down, next) = mind(|m| (m.right_down, m.easy_next));
    if down && now() >= next {
        easy_place_click(h);
    }
}

/// Takes the block looked at in hand. True when the hand is changing: the click waits a tick, so
/// it can't go out with what was held before.
fn easy_select(h: &mut dyn Hands) -> bool {
    if runtime::with(|s| s.settings.printer_on) {
        return false;
    }
    let Some((_, wanted)) = easy_target(h) else { return false };
    let Some(item) = item_for(h, &wanted) else { return false };
    if held(h).as_ref() == Some(&item) {
        mind(|m| m.easy_pick = None);
        return false;
    }
    let now = now();
    if mind(|m| m.easy_pick.as_ref().is_some_and(|(i, until)| *i == item && now < *until)) {
        return true;
    }
    let taken = h.take_in_hand(&item);
    match taken {
        Ok(()) => {
            mind(|m| m.easy_pick = Some((item, now + 5)));
            runtime::with(|s| s.easy_note.clear());
        }
        Err(e) => {
            runtime::with(|s| s.easy_note = format!("{}: {}", item_name(&item), hand_error(e)));
            mind(|m| m.easy_pick = Some((item, now + 20)));
        }
    }
    true
}

/// Places the block the schematic wants where the view first meets one, when it is the item in
/// hand and a click gives exactly that block. True when the click belongs to easy place.
fn easy_place_click(h: &mut dyn Hands) -> bool {
    if !easy_active(h) {
        return false;
    }
    let Some(eye) = h.eye() else { return false };
    let Some((p, wanted)) = easy_target(h) else { return false };
    let now = now();
    let (interval, air, retry) = runtime::with(|s| (s.settings.easy_interval, s.settings.easy_air, s.settings.printer.retry_ticks));
    // Just clicked and the world hasn't caught up: a second click could make a slab double.
    if runtime::with(|s| s.recent.blocked(p, now, retry)) {
        return true;
    }
    let held = held(h);
    if item_for(h, &wanted) != held {
        return false;
    }
    let Some(item) = held else { return false };
    let reach = h.reach();
    let Some(click) = aim_for(h, eye, reach, p, &wanted, &item, air, PlacementStage::Final) else {
        // With air place it would go: then nothing but the missing neighbour is in the way.
        let lonely = !air && aim_for(h, eye, reach, p, &wanted, &item, true, PlacementStage::Final).is_some();
        let name = item_name(&wanted.name);
        let why = if lonely { tr!("easy.lonely", name) } else { tr!("easy.cant", name) };
        runtime::with(|s| s.easy_note = why);
        return false;
    };
    if do_click(h, &click).is_err() {
        return false;
    }
    mind(|m| m.easy_next = now + u64::from(interval.max(1)));
    runtime::with(|s| {
        s.placed += 1;
        s.recent.mark(p, now);
        s.easy_note.clear();
    });
    true
}

/// With easy place active, whether this click would make the game place a block on its own
/// (asked of the game's placement, so food, buckets and an empty hand don't count). With nothing
/// to click yet a block in hand counts too: the aim can move before the game gets to the press.
/// Using a chest, door or lever stays possible; Shift places against them, so not then.
fn easy_place_refuses(h: &mut dyn Hands) -> bool {
    if !easy_active(h) {
        return false;
    }
    let Some(item) = held(h) else { return false };
    let Some((origin, dir)) = h.aim() else { return false };
    let reach = h.reach();
    let Some(hit) = h.clip_ray(origin, dir, reach) else {
        return h.places_block(&item);
    };
    let sneaking = h.sneaking();
    if !sneaking && interactive(&h.facts(hit.pos).name) {
        return false;
    }
    let (yaw, pitch) = h.look();
    h.placement(&hit, yaw, pitch, sneaking, &item).is_some()
}

// ---------------------------------------------------------------- the mouse

/// A mouse button while playing (0 left, 1 right, 2 middle), from the game side before the game
/// handles it. Whether the mod took it (the game then doesn't see it).
pub fn mouse(h: &mut dyn Hands, b: usize, pressed: bool) -> bool {
    // The mod's own clicks come through here too: those go to the game.
    if mind(|m| m.own_click) {
        return false;
    }
    if b == 1 {
        mind(|m| m.right_down = pressed);
    }
    // A release goes where its press went.
    if !pressed {
        return match b {
            1 => mind(|m| std::mem::take(&mut m.easy_taken)),
            2 => mind(|m| std::mem::take(&mut m.middle_taken)),
            _ => false,
        };
    }
    if !h.playing() || !World::new(h.host()).in_world() || !runtime::with(|s| s.settings_ready()) {
        return false;
    }
    if runtime::tool_held(&World::new(h.host())) {
        return false;
    }
    match b {
        // Easy place: the press is taken when it places, and when the game's own use would
        // place a block the schematic doesn't want there.
        1 => {
            let taken = easy_place_click(h) || easy_place_refuses(h);
            mind(|m| m.easy_taken = taken);
            taken
        }
        // Middle click on a block the schematic still wants: that block, as the game's own pick.
        2 => {
            let taken = pick_wanted(h);
            mind(|m| m.middle_taken = taken);
            taken
        }
        _ => false,
    }
}

/// The first cell along the view (up to the block it hits) where the schematic wants something
/// that isn't there, skipping `skip`.
fn first_wanted(h: &dyn Hands, skip: &[[i32; 3]]) -> Option<([i32; 3], Status, BlockState)> {
    let (origin, dir) = h.aim()?;
    let reach = h.reach();
    let limit = h.clip_ray(origin, dir, reach).map_or(reach, |hit| (0..3).map(|i| (hit.point[i] - origin[i]).powi(2)).sum::<f64>().sqrt());
    let layer = runtime::with(|s| s.layer);
    let cell_at = |t: f64| [0, 1, 2].map(|i| (origin[i] + dir[i] * t).floor() as i32);
    let mut last = cell_at(0.0);
    let mut t = 0.0;
    while t < limit {
        t = (t + 0.02).min(limit);
        let cell = cell_at(t);
        if cell == last {
            continue;
        }
        last = cell;
        if !layer.contains(cell[1]) || skip.contains(&cell) {
            continue;
        }
        let Some((status, wanted)) = status_at(h, cell) else { continue };
        if status != Status::Correct
            && !wanted.is_air()
            && !matches!(wanted.name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column")
        {
            return Some((cell, status, wanted));
        }
    }
    None
}

/// Takes the block the schematic wants where the player looks, like the game's own pick block.
/// Whether the click was taken: not with the overlay off, nor when it isn't on such a block.
fn pick_wanted(h: &mut dyn Hands) -> bool {
    if h.spectator() || !runtime::with(|s| s.settings.render && s.slots.iter().any(|x| x.shown)) || h.entity_targeted() {
        return false;
    }
    let Some((.., wanted)) = first_wanted(h, &[]) else { return false };
    let Some(item) = item_for(h, &wanted) else { return false };
    if let Err(e) = h.take_in_hand(&item) {
        runtime::say(&format!("{}: {}", item_name(&item), hand_error(e)));
    }
    true
}

// ---------------------------------------------------------------- clicks

fn do_click(h: &mut dyn Hands, click: &Click) -> Result<(), String> {
    let remote = h.remote();
    let now = now();
    mind(|m| m.own_click = true);
    let r = h.click(click.yaw, click.pitch, &click.hit, click.sneak);
    mind(|m| {
        m.own_click = false;
        if remote {
            m.server_look = Some(now);
        }
    });
    r
}

/// Whether `item` can be put in hand: it is in the hotbar, or the player is in creative.
fn can_take(h: &dyn Hands, item: &str) -> Result<(), String> {
    if h.creative() || (0..9).any(|i| h.slot_item(i).as_deref() == Some(item)) {
        return Ok(());
    }
    Err(if (9..36).any(|i| h.slot_item(i).as_deref() == Some(item)) { tr!("hand.not_hotbar") } else { tr!("hand.none") })
}

/// What the game side (`take_in_hand`, which can't reach the texts) said, in words.
fn hand_error(e: String) -> String {
    match e.as_str() {
        "none carried" => tr!("hand.none"),
        "the inventory can't be changed right now" => tr!("hand.busy"),
        _ => e,
    }
}

/// Whether the block at `support` can be clicked to place against it. Sneaking lets chests, doors
/// and the like be clicked without opening them.
fn clickable(h: &dyn Hands, support: [i32; 3], sneak: bool) -> bool {
    let f = h.facts(support);
    !(f.air || f.replaceable || f.no_outline || (!sneak && interactive(&f.name)))
}

/// Whether a real neighbour of `p` shows the eye a face to click. On a server the click isn't
/// made sneaking.
fn has_support(h: &dyn Hands, eye: [f64; 3], reach: f64, p: [i32; 3], wanted: &BlockState) -> bool {
    let sneak = !h.remote();
    let open = h.facts(p).air;
    printer::FACES.iter().any(|&f| {
        (!open || (printer::face_fits(wanted, f) && printer::face_in_view(eye, reach, p, f))) && clickable(h, [p[0] + f[0], p[1] + f[1], p[2] + f[2]], sneak)
    })
}

/// What decides whether a block can be placed from here: where the eye is (to a quarter block), the
/// spot and its neighbours, and what is wanted there.
fn stuck_key(h: &dyn Hands, eye: [f64; 3], p: [i32; 3], wanted: &BlockState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hs = std::collections::hash_map::DefaultHasher::new();
    eye.map(|v| (v * 4.0).floor() as i64).hash(&mut hs);
    {
        let w = World::new(h.host());
        w.block(p).hash(&mut hs);
        for f in printer::FACES {
            w.block([p[0] + f[0], p[1] + f[1], p[2] + f[2]]).hash(&mut hs);
        }
    }
    wanted_id(h, wanted).hash(&mut hs);
    hs.finish()
}

/// Puts `item` in hand from the hotbar. In creative only the hotbar is used: an empty slot, or
/// one whose item the schematic doesn't need any more, or the one the printer used longest ago,
/// gets the item. Whether the hand changed.
fn select(h: &mut dyn Hands, item: &str) -> Result<bool, String> {
    let now = now();
    can_take(h, item)?;
    if let Some(i) = (0..9).find(|&i| h.slot_item(i).as_deref() == Some(item)) {
        let changed = h.selected() != i;
        h.select(i);
        mind(|m| m.hotbar_used[i] = now);
        return Ok(changed);
    }
    let needed: HashSet<String> = runtime::with(|s| {
        let Some(slot) = s.sel() else { return HashSet::new() };
        let left = if s.tally().is_some() { &s.missing } else { &slot.totals };
        left.iter().filter(|(_, n)| **n > 0).map(|(k, _)| k.clone()).collect()
    });
    let used = mind(|m| m.hotbar_used);
    let items: Vec<Option<String>> = (0..9).map(|i| h.slot_item(i)).collect();
    let slot = (0..9)
        .find(|&i| items[i].is_none())
        .or_else(|| (0..9).filter(|&i| items[i].as_ref().is_some_and(|s| !needed.contains(s))).min_by_key(|&i| used[i]))
        .or_else(|| (0..9).min_by_key(|&i| used[i]))
        .unwrap_or(0);
    h.creative_set(slot, item);
    h.select(slot);
    mind(|m| m.hotbar_used[slot] = now);
    Ok(true)
}

#[derive(Clone, Copy)]
enum PlacementStage {
    PrinterFirst,
    Stack(u32),
    Final,
}

/// Asks the game's own placement what a click would place; whether that is (on the way to)
/// `wanted` at `p`.
#[allow(clippy::too_many_arguments)]
fn would_place(h: &mut dyn Hands, p: [i32; 3], wanted: &BlockState, wanted_id: StateId, item: &str, hit: &Hit, yaw: f32, pitch: f32, sneak: bool, stage: PlacementStage) -> bool {
    let Some((pos, state)) = h.placement(hit, yaw, pitch, sneak, item) else { return false };
    if pos != p {
        return false;
    }
    if state == wanted_id {
        return true;
    }
    let got = state_of(h, state);
    match stage {
        PlacementStage::PrinterFirst => verify::placement_ok(wanted, &got) || verify::upgradable(wanted, &got),
        PlacementStage::Stack(remaining) => {
            verify::stack_remaining(wanted, &got).is_some_and(|after| after < remaining)
        }
        PlacementStage::Final => verify::placement_ok(wanted, &got),
    }
}

fn face_axes(f: [i32; 3]) -> (usize, usize, usize) {
    let axis = f.iter().position(|&v| v != 0).unwrap_or(1);
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    (axis, u, v)
}

/// A look and click that make vanilla's placement put `wanted` at `p`: points on each face of
/// the neighbours (sneaking too, when not on a server), asking the game's own placement what each
/// would place. With `air`, a spot no neighbour can be clicked for is clicked itself ("air
/// place": the clicked block is replaceable, so the placement goes into it), with any look.
#[allow(clippy::too_many_arguments)]
fn aim_for(
    h: &mut dyn Hands,
    eye: [f64; 3],
    reach: f64,
    p: [i32; 3],
    wanted: &BlockState,
    item: &str,
    air: bool,
    stage: PlacementStage,
) -> Option<Click> {
    let wanted_id = wanted_id(h, wanted)?;
    const OFFSETS: [f64; 5] = [0.0, -0.3, 0.3, -0.42, 0.42];
    let sneaks: &[bool] = if h.remote() { &[false] } else { &[false, true] };
    let open = h.facts(p).air;
    for &sneak in sneaks {
        for f in printer::FACES {
            let support = [p[0] + f[0], p[1] + f[1], p[2] + f[2]];
            if (open && !(printer::face_fits(wanted, f) && printer::face_in_view(eye, reach, p, f))) || !clickable(h, support, sneak) {
                continue;
            }
            // The face of `support` that touches `p`.
            let centre = [0, 1, 2].map(|i| f64::from(p[i]) + 0.5 + f64::from(f[i]) * 0.5);
            let (_, u, v) = face_axes(f);
            for du in OFFSETS {
                for dv in OFFSETS {
                    let mut point = centre;
                    point[u] += du;
                    point[v] += dv;
                    let (yaw, pitch) = yaw_pitch(eye, point);
                    let pitch = pitch.clamp(-90.0, 90.0);
                    let dir = runtime::view_vector(yaw, pitch);
                    let Some(hit) = h.clip_ray(eye, dir, reach) else { continue };
                    if hit.pos != support && hit.pos != p {
                        continue;
                    }
                    if would_place(h, p, wanted, wanted_id, item, &hit, yaw, pitch, sneak, stage) {
                        return Some(Click { yaw, pitch, hit, sneak });
                    }
                }
            }
        }
    }
    if !air {
        return None;
    }
    // Air place: a point on one of `p`'s own faces, looked at naturally first, then any look
    // (stairs, signs and the like take their facing from the look, not the face).
    for &sneak in sneaks {
        for d in printer::FACES {
            let (axis, _, _) = face_axes(d);
            // Off-centre heights on the sides decide top/bottom halves.
            let heights: &[f64] = if axis == 1 { &[0.0] } else { &[0.0, 0.3, -0.3] };
            for &dy in heights {
                let mut point = [0, 1, 2].map(|i| f64::from(p[i]) + 0.5 + f64::from(d[i]) * 0.5);
                point[1] += dy;
                let dist = (0..3).map(|i| (point[i] - eye[i]).powi(2)).sum::<f64>().sqrt();
                if dist > reach {
                    continue;
                }
                let hit = Hit { pos: p, face: d, point };
                let (ny, np) = yaw_pitch(eye, point);
                let natural = std::iter::once((ny, np.clamp(-90.0, 90.0)));
                let any = (0..16).flat_map(|i| [0.0f32, 50.0, -50.0, 90.0, -90.0].map(move |pitch| (i as f32 * 22.5 - 180.0, pitch)));
                for (yaw, pitch) in natural.chain(any) {
                    if would_place(h, p, wanted, wanted_id, item, &hit, yaw, pitch, sneak, stage) {
                        return Some(Click { yaw, pitch, hit, sneak });
                    }
                }
            }
        }
    }
    None
}

/// Add one item to the existing stack, only if the predicted state gets closer to the target.
fn aim_stack(h: &mut dyn Hands, eye: [f64; 3], reach: f64, p: [i32; 3], wanted: &BlockState, item: &str, air: bool) -> Option<Click> {
    let wanted_id = wanted_id(h, wanted)?;
    let actual = actual(h, p)?;
    let stage = PlacementStage::Stack(verify::stack_remaining(wanted, &actual)?);
    let outline = h.outline(p);
    let cell = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    const OFFSETS: [f64; 3] = [0.0, -0.3, 0.3];
    // Short blocks (snow, candles, slabs) need a ray aimed at their actual outline. The
    // cell faces also give us rays to neighbouring supports for attaching another face.
    for bounds in outline.iter().copied().chain(std::iter::once(cell)) {
        for face in printer::FACES {
            let (axis, u, v) = face_axes(face);
            let mut centre = [0, 1, 2].map(|i| f64::from(p[i]) + (bounds[i] + bounds[i + 3]) * 0.5);
            centre[axis] = f64::from(p[axis]) + bounds[axis + if face[axis] > 0 { 3 } else { 0 }];
            for du in OFFSETS {
                for dv in OFFSETS {
                    let mut point = centre;
                    point[u] += du * (bounds[u + 3] - bounds[u]);
                    point[v] += dv * (bounds[v + 3] - bounds[v]);
                    let (yaw, pitch) = yaw_pitch(eye, point);
                    let pitch = pitch.clamp(-90.0, 90.0);
                    let dir = runtime::view_vector(yaw, pitch);
                    if let Some(hit) = h.clip_ray(eye, dir, reach)
                        && would_place(h, p, wanted, wanted_id, item, &hit, yaw, pitch, false, stage)
                    {
                        return Some(Click { yaw, pitch, hit, sneak: false });
                    }
                    if air && (0..3).map(|i| (point[i] - eye[i]).powi(2)).sum::<f64>() <= reach * reach {
                        let hit = Hit { pos: p, face, point };
                        if would_place(h, p, wanted, wanted_id, item, &hit, yaw, pitch, false, stage) {
                            return Some(Click { yaw, pitch, hit, sneak: false });
                        }
                    }
                }
            }
        }
    }
    None
}

/// A plain (not sneaking) click on the block at `p`, to use it: a repeater's delay, a
/// comparator's mode, a note block's note.
fn aim_block(h: &dyn Hands, eye: [f64; 3], reach: f64, p: [i32; 3]) -> Option<Click> {
    let centre = [f64::from(p[0]) + 0.5, f64::from(p[1]) + 0.1, f64::from(p[2]) + 0.5];
    let (yaw, pitch) = yaw_pitch(eye, centre);
    let pitch = pitch.clamp(-90.0, 90.0);
    let dir = runtime::view_vector(yaw, pitch);
    let hit = match h.clip_ray(eye, dir, reach) {
        Some(hit) if hit.pos == p => hit,
        _ => Hit { pos: p, face: [0, 1, 0], point: centre },
    };
    Some(Click { yaw, pitch, hit, sneak: false })
}
