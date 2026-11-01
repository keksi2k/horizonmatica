// Horizonmatica: the game-side module. horizon-mixins inserts this file as
// `pub(crate) mod horizonmatica` into `crates/horizon-client/src/game.rs`, so it is a child of the
// `game` module and may use `Game`'s fields and private methods (the world, the inventory, the
// view, `clip_ray`, `with_level`, `block_model_vertices`...).
//
// The helpers (one per hook) call into here; the game-independent logic is the
// `horizonmatica_core` crate. The overlay is drawn by appending quads to the frame's entity geometry
// (`draw`, called from a HEAD hook on `Game::cart_geometry`), in the entity renderer's
// translucent pass, so it needs no render hook point.

use super::Game;
use horizon_client::extras::Corner;
use horizon_client::extras_hud::Corners;
use horizon_client::hud::HudBatch;
use horizon_client::mesher::Vertex;
use horizon_client_render::gfx::native_render::horizonmatica_bridge as bridge;
use super::horizonmatica_menus as menus;
use horizon_core::geometry::{Direction, Vec3};
use horizon_core::position::BlockPos;
use horizon_world::level::PlaceContext;
use horizon_world::registry::BlockStateId;
use horizonmatica_core::formats::Format;
use horizonmatica_core::materials::{self, Material};
use horizonmatica_core::printer::{self, PrinterSettings, Recent};
use horizonmatica_core::verify::{self, Status, Tally};
use horizonmatica_core::{BlockState, LayerMode, Mirror, Placement, Rotation, Schematic};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) const ID: &str = "horizonmatica";
pub(crate) const VERSION: &str = "0.1.0";
pub(crate) const DESCRIPTION: &str = "Schematic overlay, verifier, material list and printer (auto-build) for      .litematic, .schem, .schematic and .nbt files. Based on Litematica by masa (maruohon) and Sakura-Ryoko,      and the Litematica Printer by aleksilassila. Not made or endorsed by them.";

/// Who Horizonmatica owes its design to (`/horizonmatica credits`, the settings page).
const CREDITS: &[&str] = &[
    "Horizonmatica is a Horizon re-creation of Litematica. All credit for the idea and design goes to:",
    "  Litematica by masa (maruohon), github.com/maruohon/litematica, maintained by Sakura-Ryoko (github.com/sakura-ryoko/litematica)",
    "  Litematica Printer by aleksilassila, github.com/aleksilassila/litematica-printer (maintained by Sakura-Ryoko)",
    "  Horizon by the horizon-rust team, github.com/horizon-rust/horizon",
    "  File formats: Litematica (.litematic), Sponge/WorldEdit (.schem), MCEdit (.schematic), Mojang (structure .nbt)",
    "No code was copied from Litematica or the printer; it is not made or endorsed by their authors.",
];

/// Keys: (name, label, default).
pub(crate) const KEYS: &[(&str, &str, &str)] = &[
    ("open_menu", "Open the Horizonmatica menu", "key.keyboard.m"),
    ("toggle_render", "Toggle schematic rendering", "key.keyboard.v"),
    ("layer_up", "Layer up", "key.keyboard.page.up"),
    ("layer_down", "Layer down", "key.keyboard.page.down"),
    ("layer_mode", "Cycle layer mode", "key.keyboard.home"),
    ("toggle_printer", "Toggle printer (auto-build)", "key.keyboard.k"),
    ("toggle_materials", "Toggle material list", "key.keyboard.j"),
    ("move_mode", "Move mode (wheel/arrows move the schematic)", "key.keyboard.n"),
];

/// Cells the verifier scans per tick.
const SCAN_PER_TICK: u64 = 32_768;
/// Most quads the overlay keeps (4 vertices each).
const MAX_QUADS: usize = 300_000;
/// Fully lit (sky 15, block 15 as 16x fixed point): ghosts show their texture's own colours.
const FULL_BRIGHT: u32 = 240 | (240 << 8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Missing blocks as translucent block models, mismatches as coloured boxes.
    Ghost,
    /// Everything as coloured boxes.
    Boxes,
}

#[derive(Clone, Debug)]
struct Settings {
    render: bool,
    /// Ghosts and boxes show through terrain.
    through: bool,
    mode: Mode,
    alpha: f32,
    radius: i32,
    show_extra: bool,
    show_wrong: bool,
    strict: bool,
    hud: bool,
    materials_hud: bool,
    printer_on: bool,
    printer: PrinterSettings,
    /// Redstone's on/off (powered, lit...) counts as a wrong state.
    power: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            render: true,
            through: false,
            mode: Mode::Ghost,
            alpha: 0.55,
            radius: 32,
            show_extra: true,
            show_wrong: true,
            strict: false,
            hud: true,
            materials_hud: false,
            printer_on: false,
            printer: PrinterSettings { interval: 3, ..PrinterSettings::default() },
            power: true,
        }
    }
}

#[derive(Default)]
struct Scan {
    revision: u64,
    index: u64,
    tally: Tally,
    missing: BTreeMap<String, u64>,
    unloaded: u64,
}

/// The overlay's geometry lives in the render side's `horizonmatica_bridge`, per section; this is
/// what the game side remembers of it.
#[derive(Default)]
struct Mesh {
    /// The world block every section's positions are relative to.
    anchor: [i32; 3],
    /// Built sections: the world section each was built against and its quad count.
    built: HashMap<[i32; 3], (Option<Arc<horizon_client::chunk::Section>>, usize)>,
    /// Quads of all sections.
    quads: usize,
}

/// What the game makes of a palette entry (cached per placement revision).
#[derive(Clone, Copy, Debug)]
struct Resolved {
    id: Option<BlockStateId>,
    air: bool,
    /// A full opaque cube (`solid_render`): ghost faces between two are hidden.
    cube: bool,
}

#[derive(Default)]
struct State {
    settings_loaded: bool,
    /// The world (and dimension) the placement belongs to: `world_key`.
    world_key: Option<String>,
    /// Where the player is in the menu.
    nav: menus::Nav,
    /// The tick the printer last used each hotbar slot (creative picks reuse the oldest).
    hotbar_used: [u64; 9],
    /// The menu screen: its tab, the file picked on Load, the files listed and what is known of
    /// them, the last message, the origin its fields show.
    ui_tab: usize,
    ui_file: Option<String>,
    ui_files: Vec<(String, u64)>,
    ui_files_at: Option<std::time::Instant>,
    ui_info: HashMap<String, Result<FileInfo, String>>,
    ui_message: String,
    ui_origin: Option<[i32; 3]>,
    settings: Settings,
    placement: Option<Placement>,
    layer: LayerMode,
    /// Wanted state → game state id (None: the game doesn't know the block).
    ids: HashMap<BlockState, Option<BlockStateId>>,
    /// Game state id → its name and properties.
    states: HashMap<u32, BlockState>,
    /// Item per wanted state (None: no item, such as air or fire).
    items: HashMap<BlockState, Option<String>>,
    unknown: Vec<String>,
    totals: BTreeMap<String, u64>,
    scan: Scan,
    tally: Option<Tally>,
    missing: BTreeMap<String, u64>,
    unloaded: u64,
    /// What the built sections show (placement, layers, display settings): another key builds
    /// them all again.
    mesh_key: Option<(u64, LayerMode, Mode, bool, bool, bool, bool, u32)>,
    mesh: Mesh,
    /// Palette entry (region, index) -> what the game makes of it, for `resolved_rev`.
    resolved: HashMap<(usize, u32), Resolved>,
    resolved_rev: u64,
    recent: Recent,
    next_print: u64,
    /// On a server: the last click sent its own look and aimed through `forced_hit`;
    /// the next tick gives the server the player's real look back.
    server_look: bool,
    /// Move mode: the wheel and the arrow keys move the placement. Off, nothing but a
    /// command moves it, so it can't be pushed by accident.
    moving: bool,
    /// Wheel steps not yet a whole block (touchpads send fractions).
    wheel: f32,
    /// The credits line was shown this session (on the first load).
    credited: bool,
    printer_note: String,
    placed: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(f)
}

// ---------------------------------------------------------------- settings

/// The menu (horizon-menus): the main page and its sub-pages, and the schema of every setting
/// with its default (from `Settings::default()`).
fn menu() -> &'static menus::Menu {
    static MENU: std::sync::OnceLock<menus::Menu> = std::sync::OnceLock::new();
    MENU.get_or_init(|| {
        let d = Settings::default();
        menus::Menu::new("Horizonmatica")
            .page("main", "Horizonmatica", |p| {
                p.list("status", 4);
                p.link("load", "Load schematic");
                p.link("placement", "Placement");
                p.link("printer", "Printer (auto-build)");
                p.link("display", "Display");
                p.link("hud", "HUD & material list");
                p.link("credits", "Credits");
            })
            .page("load", "Load schematic", |p| {
                p.action("reload", "Reload the last schematic");
                p.action("folder", "Open the schematics folder");
                p.list("files", 10);
            })
            .page("placement", "Placement", |p| {
                p.list("placement", 8);
                p.action("here", "Move it to my position");
                p.action("rotate_ccw", "Rotate counter-clockwise");
                p.action("unload", "Unload the schematic");
            })
            .page("printer", "Printer (auto-build)", |p| {
                p.list("printer", 4);
                p.number("printer_interval", "One block every", f64::from(d.printer.interval), (1.0, 20.0, 1.0), 0, " ticks");
                p.number("printer_reach", "Reach", d.printer.reach, (1.0, 6.0, 0.5), 1, " blocks");
                p.toggle("air_place", "Air place (no neighbour needed)", d.printer.air_place);
            })
            .page("display", "Display", |p| {
                p.toggle("render", "Rendering", d.render);
                p.toggle("through", "See through blocks", d.through);
                p.choice("mode", "Mode", &[("ghost", "Ghost blocks"), ("boxes", "Boxes")], "ghost");
                p.percent("alpha", "Ghost opacity", f64::from(d.alpha), 0.1, 1.0, 0.05);
                p.number("radius", "Render radius", f64::from(d.radius), (16.0, 128.0, 16.0), 0, " blocks");
                p.toggle("show_extra", "Show extra blocks", d.show_extra);
                p.toggle("show_wrong", "Show wrong blocks", d.show_wrong);
                p.toggle("strict", "Strict states (connections, stair shapes)", d.strict);
                p.toggle("power", "Redstone on/off counts as wrong", d.power);
            })
            .page("hud", "HUD & material list", |p| {
                p.toggle("hud", "Info HUD", d.hud);
                p.toggle("materials_hud", "Material list HUD", d.materials_hud);
                p.action("materials", "Print the material list to chat");
            })
            .page("credits", "Credits", |p| {
                p.info("Horizonmatica re-creates Litematica for Horizon.");
                p.info("Litematica: masa (maruohon)");
                p.info("Maintained by Sakura-Ryoko");
                p.info("Litematica Printer: aleksilassila");
                p.info("Horizon: the horizon-rust team");
                p.info("Not made or endorsed by them.");
                p.action("credits_chat", "Show the full credits in chat");
            })
    })
}

fn load_settings(game: &Game<'_>, s: &mut State) {
    if s.settings_loaded {
        return;
    }
    s.settings_loaded = true;
    let m = menu();
    let st = &menus::GameStore { game, id: ID };
    let d = Settings::default();
    s.settings = Settings {
        render: m.bool(st, "render"),
        through: m.bool(st, "through"),
        mode: if m.choice(st, "mode") == "boxes" { Mode::Boxes } else { Mode::Ghost },
        alpha: m.number(st, "alpha") as f32,
        radius: m.number(st, "radius") as i32,
        show_extra: m.bool(st, "show_extra"),
        show_wrong: m.bool(st, "show_wrong"),
        strict: m.bool(st, "strict"),
        hud: m.bool(st, "hud"),
        materials_hud: m.bool(st, "materials_hud"),
        // A session's switch: settings are read again after every menu click.
        printer_on: s.settings.printer_on,
        printer: PrinterSettings {
            interval: m.number(st, "printer_interval") as u32,
            reach: m.number(st, "printer_reach"),
            air_place: m.bool(st, "air_place"),
            ..d.printer
        },
        power: m.bool(st, "power"),
    };
}

/// Writes `s` in the menu's format (commands change settings too).
fn save_settings(game: &Game<'_>, s: &Settings) {
    let set = |k: &str, v: String| game.native_set_setting(ID, k, &v);
    set("render", s.render.to_string());
    set("through", s.through.to_string());
    set("mode", if s.mode == Mode::Boxes { "boxes" } else { "ghost" }.into());
    set("alpha", s.alpha.to_string());
    set("radius", s.radius.to_string());
    set("show_extra", s.show_extra.to_string());
    set("show_wrong", s.show_wrong.to_string());
    set("strict", s.strict.to_string());
    set("hud", s.hud.to_string());
    set("materials_hud", s.materials_hud.to_string());
    set("printer_interval", s.printer.interval.to_string());
    set("printer_reach", s.printer.reach.to_string());
    set("air_place", s.printer.air_place.to_string());
    set("power", s.power.to_string());
}

/// Saves where the placement is: as the last one (`/horizonmatica reload`) and as this world's,
/// which comes back when the world is joined again.
fn save_last(game: &Game<'_>, p: &Placement) {
    let v = format!(
        "{}|{}|{}|{}|{}|{}",
        p.file_name,
        p.origin[0],
        p.origin[1],
        p.origin[2],
        p.rotation.name(),
        p.mirror.name()
    );
    game.native_set_setting(ID, "last", &v);
    if let Some(key) = world_key(game) {
        game.native_set_setting(ID, &key, &v);
    }
}

/// The settings key of the world played and its dimension: a single-player world by its save
/// folder, a server by its address. None outside a world.
fn world_key(game: &Game<'_>) -> Option<String> {
    if game.title.is_some() || game.player.is_none() {
        return None;
    }
    let world = match game.server_address() {
        Some(address) if game.integrated.is_none() => format!("server_{address}"),
        _ => format!("world_{}", game.save.root.file_name()?.to_string_lossy()),
    };
    let key = format!("placed.{world}.{:?}", game.dim);
    Some(key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect())
}

/// A saved placement: file, origin, rotation, mirror.
fn parse_saved(v: &str) -> Option<(String, ([i32; 3], Rotation, Mirror))> {
    let parts: Vec<&str> = v.split('|').collect();
    if parts.len() != 6 || parts[0].is_empty() {
        return None;
    }
    let n = |i: usize| parts[i].parse::<i32>().ok();
    let at = ([n(1)?, n(2)?, n(3)?], Rotation::parse(parts[4]).unwrap_or_default(), Mirror::parse(parts[5]).unwrap_or_default());
    Some((parts[0].to_string(), at))
}

/// Joining a world (or another dimension of it): its placement comes back where it was left;
/// one from another world goes.
fn restore_step(game: &mut Game<'_>) {
    let Some(key) = world_key(game) else { return };
    if with(|s| s.world_key.as_deref() == Some(key.as_str())) {
        return;
    }
    with(|s| {
        s.world_key = Some(key.clone());
        if s.placement.is_some() {
            reset_caches(s);
            s.placement = None;
            s.settings.printer_on = false;
            s.moving = false;
            s.mesh = Mesh::default();
        }
    });
    bridge::clear();
    let Some((file, at)) = game.native_setting(ID, &key).as_deref().and_then(parse_saved) else { return };
    match load(game, &file, Some(at)) {
        Ok(_) => say(
            game,
            &format!("Restored {file} at {} {} {} (/hm unload removes it)", at.0[0], at.0[1], at.0[2]),
        ),
        Err(e) => say(game, &format!("Couldn't restore {file}: {e}")),
    }
}

// ---------------------------------------------------------------- the mod list

pub(super) fn listed() -> super::native_hooks::NativeMod {
    super::native_hooks::NativeMod {
        id: ID.into(),
        title: format!("Horizonmatica {VERSION}"),
        description: DESCRIPTION.into(),
        keys: KEYS.iter().map(|(n, l, d)| [n.to_string(), l.to_string(), d.to_string()]).collect(),
    }
}

/// The mod's page in Options > Mods: the menu's page the player is on.
pub(super) fn settings_buttons(game: &Game<'_>, buttons: &mut Vec<String>) {
    let mut nav = with(|s| {
        load_settings(game, s);
        std::mem::take(&mut s.nav)
    });
    let store = menus::GameStore { game, id: ID };
    buttons.extend(menu().render(&mut nav, &store, &mut |list| menu_list(list)));
    with(|s| s.nav = nav);
}

pub(super) fn settings_click(game: &mut Game<'_>, button: usize) {
    let mut nav = with(|s| std::mem::take(&mut s.nav));
    let reverse = menus::reverse(game);
    let event = {
        let mut store = menus::GameStore { game: &*game, id: ID };
        menu().click(&mut nav, &mut store, button, reverse)
    };
    with(|s| {
        s.nav = nav;
        s.settings_loaded = false;
        load_settings(game, s);
        s.mesh_key = None;
    });
    if let menus::Event::Action(action) = event {
        menu_action(game, &action);
    }
}

/// The rows the menu's lists show now: `(label, action)`, no action for a plain line.
fn menu_list(list: &str) -> Vec<(String, String)> {
    let row = |label: String, action: &str| (label, action.to_string());
    let printer = |on: bool| row(format!("Printer: {} (click to switch)", if on { "ON" } else { "OFF" }), "printer_toggle");
    match list {
        "status" => with(|s| {
            let Some(p) = &s.placement else {
                return vec![row("No schematic loaded: Load schematic below".into(), "")];
            };
            let name = if p.schematic.metadata.name.is_empty() { &p.file_name } else { &p.schematic.metadata.name };
            let mut rows = vec![row(format!("Loaded: {name}"), "")];
            if let Some(t) = &s.tally {
                rows.push(row(format!("{:.1}% done, {} missing", t.percent(), t.missing), ""));
            }
            rows.push(printer(s.settings.printer_on));
            rows
        }),
        "files" => {
            let Ok(dir) = schematics_dir() else { return vec![row("No schematics folder".into(), "")] };
            let names = horizonmatica_core::list_schematics(&dir);
            if names.is_empty() {
                return vec![row("No files yet: .litematic, .schem, .schematic, .nbt".into(), "")];
            }
            names.into_iter().map(|n| (n.clone(), format!("load:{n}"))).collect()
        }
        "placement" => with(|s| {
            let Some(p) = &s.placement else { return vec![row("No schematic loaded".into(), "")] };
            vec![
                row(format!("Origin {} {} {}", p.origin[0], p.origin[1], p.origin[2]), ""),
                row(format!("Rotation: {} (click: clockwise)", p.rotation.name()), "rotate_cw"),
                row(format!("Mirror: {} (click: next)", p.mirror.name()), "mirror_next"),
                row(format!("Layers: {} (click: next mode)", s.layer.describe()), "layer_next"),
                row(
                    format!("Move mode: {} (wheel, arrows, PgUp/PgDn)", if s.moving { "ON" } else { "OFF" }),
                    "move_mode",
                ),
            ]
        }),
        "printer" => with(|s| {
            let mut rows = vec![printer(s.settings.printer_on), row(format!("Placed this session: {}", s.placed), "")];
            if !s.printer_note.is_empty() {
                rows.push(row(s.printer_note.clone(), ""));
            }
            rows
        }),
        _ => Vec::new(),
    }
}

/// The next layer mode (all, this layer, above, below), as the key cycles them.
fn next_layer_mode() -> &'static str {
    with(|s| match s.layer {
        LayerMode::All => "here",
        LayerMode::Single(_) => "above",
        LayerMode::Above(_) => "below",
        _ => "all",
    })
}

/// A menu row's action.
fn menu_action(game: &mut Game<'_>, action: &str) {
    let r = match action {
        "folder" => {
            match schematics_dir() {
                Ok(dir) => {
                    let _ = std::fs::create_dir_all(&dir);
                    super::packs::open_folder(&dir);
                }
                Err(e) => say(game, &format!("Error: {e}")),
            }
            return;
        }
        "move_mode" => {
            toggle_move_mode(game);
            return;
        }
        "credits_chat" => {
            for line in CREDITS {
                game.native_chat(line);
            }
            return;
        }
        "mirror_next" => {
            let next = with(|s| match s.placement.as_ref().map(|p| p.mirror) {
                // x flips x (FRONT_BACK), z flips z (LEFT_RIGHT).
                Some(Mirror::None) => "x",
                Some(Mirror::FrontBack) => "z",
                _ => "none",
            });
            run(game, &["mirror", next])
        }
        "layer_next" => {
            let next = next_layer_mode();
            run(game, &["layer", next])
        }
        "reload" => run(game, &["reload"]),
        "here" => run(game, &["origin", "here"]),
        "rotate_cw" => run(game, &["rotate", "cw"]),
        "rotate_ccw" => run(game, &["rotate", "ccw"]),
        "unload" => run(game, &["unload"]),
        "printer_toggle" => run(game, &["printer"]),
        "materials" => run(game, &["materials"]),
        other => match other.strip_prefix("load:") {
            Some(file) => run(game, &["load", file]),
            None => return,
        },
    };
    report(game, r);
}

// ---------------------------------------------------------------- block lookups

fn to_world_state(s: &BlockState) -> horizon_world::chunk::BlockState {
    horizon_world::chunk::BlockState { name: s.name.clone(), properties: s.properties.clone().into() }
}

/// The game's id for a schematic state; properties the game doesn't know are left out.
fn resolve(game: &Game<'_>, s: &BlockState) -> Option<BlockStateId> {
    let registry = &game.pack.assets.registry;
    if let Ok(id) = registry.resolve(&to_world_state(s)) {
        return Some(id);
    }
    let default = registry.default_state(&s.name)?;
    let mut best = default;
    for (k, v) in &s.properties {
        let mut probe = registry.state(best)?.clone();
        if probe.properties.contains_key(k) {
            probe.properties.insert(k.clone(), v.clone());
            if let Some(id) = registry.id(&probe) {
                best = id;
            }
        }
    }
    Some(best)
}

fn wanted_id(game: &Game<'_>, s: &mut State, w: &BlockState) -> Option<BlockStateId> {
    if let Some(id) = s.ids.get(w) {
        return *id;
    }
    let id = resolve(game, w);
    if id.is_none() && !s.unknown.contains(&w.name) {
        s.unknown.push(w.name.clone());
    }
    s.ids.insert(w.clone(), id);
    id
}

fn game_state(game: &Game<'_>, states: &mut HashMap<u32, BlockState>, id: BlockStateId) -> BlockState {
    states
        .entry(id.0)
        .or_insert_with(|| match game.pack.assets.registry.state(id) {
            Some(st) => BlockState { name: st.name.clone(), properties: st.properties.clone().into() },
            None => BlockState::air(),
        })
        .clone()
}

fn item_for(game: &Game<'_>, s: &mut State, w: &BlockState) -> Option<String> {
    if let Some(i) = s.items.get(w) {
        return i.clone();
    }
    let item = match w.name.as_str() {
        "minecraft:water" | "minecraft:bubble_column" => Some("minecraft:water_bucket".to_string()),
        "minecraft:lava" => Some("minecraft:lava_bucket".to_string()),
        _ => wanted_id(game, s, w).and_then(|id| {
            let item = &game.res.facts().get(id).item;
            (!item.is_empty() && &**item != "minecraft:air").then(|| item.to_string())
        }),
    };
    s.items.insert(w.clone(), item.clone());
    item
}

fn loaded(game: &Game<'_>, w: [i32; 3]) -> bool {
    let shape = &game.world.shape;
    w[1] >= shape.min_y
        && w[1] < shape.min_y + shape.height
        && game.world.chunks.get(&(w[0] >> 4, w[2] >> 4)).is_some()
}

fn resolve_ref(game: &Game<'_>, s: &mut State, r: (usize, u32)) -> Option<Resolved> {
    let rev = s.placement.as_ref()?.revision;
    if s.resolved_rev != rev {
        s.resolved.clear();
        s.resolved_rev = rev;
    }
    if let Some(v) = s.resolved.get(&r) {
        return Some(*v);
    }
    let state = s.placement.as_ref()?.palette_state(r).clone();
    let id = wanted_id(game, s, &state);
    let res = Resolved { id, air: state.is_air(), cube: id.is_some_and(|i| game.res.facts().get(i).solid_render) };
    s.resolved.insert(r, res);
    Some(res)
}

/// The verifier's verdict at `w` (None: outside the schematic or nothing to say), with where its
/// wanted state is and what the game makes of it. The common cases (the right block, air where
/// a block is missing) need no state compared.
fn quick_status(game: &Game<'_>, s: &mut State, w: [i32; 3]) -> Option<(Status, (usize, u32), Resolved)> {
    let r = s.placement.as_ref()?.wanted_ref(w)?;
    let res = resolve_ref(game, s, r)?;
    let actual_id = game.world.block(w[0], w[1], w[2]);
    if !res.air && res.id == Some(actual_id) {
        return Some((Status::Correct, r, res));
    }
    let facts = game.res.facts();
    if facts.is_air(actual_id) {
        return (!res.air).then_some((Status::Missing, r, res));
    }
    let f = facts.get(actual_id);
    let replaceable = f.replaceable || (f.fluid != horizon_client::blocks::Fluid::None && !f.waterlogged);
    let wanted = s.placement.as_ref()?.palette_state(r).clone();
    let actual = game_state(game, &mut s.states, actual_id);
    let status = verify::classify_power(Some(&wanted), &actual, replaceable, s.settings.strict, s.settings.power)?;
    Some((status, r, res))
}

/// The verifier's verdict at `w` and the state wanted there.
fn status_at(game: &Game<'_>, s: &mut State, w: [i32; 3]) -> Option<(Status, BlockState)> {
    let (status, r, _) = quick_status(game, s, w)?;
    Some((status, s.placement.as_ref()?.palette_state(r).clone()))
}

// ---------------------------------------------------------------- loading

fn schematics_dir() -> Result<PathBuf, String> {
    let config = Game::native_config_dir()?;
    let dir = config.parent().map_or_else(|| config.join("schematics"), |g| g.join("schematics"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn feet(game: &Game<'_>) -> Option<[i32; 3]> {
    let p = game.native_player()?;
    Some([p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32])
}

fn reset_caches(s: &mut State) {
    s.ids.clear();
    s.items.clear();
    s.unknown.clear();
    s.totals.clear();
    s.scan = Scan::default();
    s.tally = None;
    s.missing.clear();
    s.unloaded = 0;
    s.mesh_key = None;
    s.recent.clear();
}

/// Counts every item the placement needs (once per load).
fn count_totals(game: &Game<'_>, s: &mut State) {
    let Some(p) = s.placement.take() else { return };
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for (ri, region) in p.schematic.regions.iter().enumerate() {
        let palette = &p.palettes()[ri];
        let mut histogram = vec![0u64; palette.len()];
        for y in 0..region.size[1] {
            for z in 0..region.size[2] {
                for x in 0..region.size[0] {
                    histogram[region.palette_index([x, y, z]) as usize] += 1;
                }
            }
        }
        for (i, n) in histogram.into_iter().enumerate() {
            let st = &palette[i];
            let per = u64::from(materials::items_for_state(st));
            if n == 0 || per == 0 {
                continue;
            }
            if let Some(item) = item_for(game, s, st) {
                *counts.entry(item).or_insert(0) += n * per;
            }
        }
    }
    s.totals = counts;
    s.placement = Some(p);
}

/// Brings an older file's states up to this game version with Horizon's data fixer (1.12 names
/// to flattened ones, `grass` to `short_grass`, ...). Returns how many palette entries changed.
fn upgrade_states(schematic: &mut Schematic) -> usize {
    use horizon_world::datafix::{self, schema::Ty};
    use horizon_world::nbt::Tag as N;
    let from = schematic.data_version as i32;
    let to = horizon_world::DATA_VERSION;
    if from <= 0 || from >= to {
        return 0;
    }
    let mut changed = 0;
    for region in &mut schematic.regions {
        for state in &mut region.palette {
            if state.is_air() {
                continue;
            }
            let name = N::String(state.name.as_str().into());
            let mut tag = if state.properties.is_empty() {
                N::compound([("Name", name)])
            } else {
                let props = state.properties.iter().map(|(k, v)| (k.as_str(), N::String(v.as_str().into())));
                N::compound([("Name", name), ("Properties", N::compound(props))])
            };
            datafix::update(Ty::BlockState, &mut tag, from, to);
            // The fixer's newest block state format names the fields `id` and `properties`.
            let Some(N::String(name)) = tag.get("id").or_else(|| tag.get("Name")) else { continue };
            let mut properties = BTreeMap::new();
            if let Some(N::Compound(props)) = tag.get("properties").or_else(|| tag.get("Properties")) {
                for (k, v) in props {
                    if let N::String(v) = v {
                        properties.insert(k.to_string_lossy(), v.to_string_lossy());
                    }
                }
            }
            let new = BlockState { name: name.to_string_lossy(), properties };
            if new != *state {
                *state = new;
                changed += 1;
            }
        }
    }
    schematic.data_version = i64::from(to);
    changed
}

fn load(game: &mut Game<'_>, query: &str, at: Option<([i32; 3], Rotation, Mirror)>) -> Result<String, String> {
    let dir = schematics_dir()?;
    let file = horizonmatica_core::find_schematic(&dir, query)?;
    let bytes = std::fs::read(dir.join(&file)).map_err(|e| format!("{file}: {e}"))?;
    let (mut schematic, format) = horizonmatica_core::formats::load(&bytes).map_err(|e| format!("{file}: {e}"))?;
    let from_version = schematic.data_version;
    let upgraded = upgrade_states(&mut schematic);
    let (origin, rotation, mirror) = match at {
        Some(a) => a,
        None => (feet(game).ok_or("not in a world")?, Rotation::None, Mirror::None),
    };
    let mut placement = Placement::new(file.clone(), schematic, origin);
    placement.set_rotation(rotation);
    placement.set_mirror(mirror);
    let name = if placement.schematic.metadata.name.is_empty() {
        file.clone()
    } else {
        placement.schematic.metadata.name.clone()
    };
    let (lo, hi) = placement.bounds();
    let size = [0, 1, 2].map(|i| hi[i] - lo[i] + 1);
    save_last(game, &placement);
    let unknown = with(|s| {
        load_settings(game, s);
        reset_caches(s);
        s.placement = Some(placement);
        s.layer = LayerMode::All;
        count_totals(game, s);
        s.unknown.clone()
    });
    if !with(|s| std::mem::replace(&mut s.credited, true)) {
        say(
            game,
            "Horizonmatica is based on Litematica by masa (maruohon) & Sakura-Ryoko and the Litematica Printer              by aleksilassila. /hm credits",
        );
    }
    let mut msg = format!(
        "Loaded {name} ({}x{}x{}, {} regions) at {} {} {}",
        size[0],
        size[1],
        size[2],
        with(|s| s.placement.as_ref().map_or(0, |p| p.schematic.regions.len())),
        origin[0],
        origin[1],
        origin[2]
    );
    if format != Format::Litematic {
        msg.push_str(&format!(" · {}", format.name()));
    }
    if upgraded > 0 {
        msg.push_str(&format!(" · {upgraded} block states updated from data version {from_version}"));
    }
    if !unknown.is_empty() {
        msg += &format!(". Unknown blocks (skipped): {}", unknown.join(", "));
    }
    Ok(msg)
}

// ---------------------------------------------------------------- commands

pub(super) fn commands(commands: &mut Vec<&'static str>) {
    commands.push("horizonmatica");
    commands.push("hm");
}

fn say(game: &mut Game<'_>, text: &str) {
    game.native_chat(&format!("[Horizonmatica] {text}"));
}

const HELP: &[&str] = &[
    "/horizonmatica list | load <name> | unload | reload | folder | info",
    "/horizonmatica origin here | origin <x> <y> <z> | move <dx> <dy> <dz> | rotate cw|ccw|180|none | mirror none|x|z",
    "/horizonmatica layer all | <y> | above <y> | below <y> | range <a> <b> | up | down | here",
    "/horizonmatica render on|off | through on|off | mode ghost|boxes | verify | materials",
    "/horizonmatica printer on|off | air on|off | speed <ticks> | reach <blocks>",
    "/horizonmatica credits: who made the original Litematica and its printer",
    "(/hm works as a short form)",
];

/// Runs `/horizonmatica …`; true when the command was ours.
pub(super) fn command(game: &mut Game<'_>, line: &str) -> bool {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("horizonmatica" | "hm") => {}
        _ => return false,
    }
    let args: Vec<&str> = words.collect();
    let result = run(game, &args);
    match result {
        Ok(lines) => {
            for l in lines {
                say(game, &l);
            }
        }
        Err(e) => say(game, &format!("Error: {e}")),
    }
    true
}

fn int(s: Option<&&str>, what: &str) -> Result<i32, String> {
    s.ok_or_else(|| format!("missing {what}"))?.parse::<i32>().map_err(|_| format!("{what} must be a number"))
}

fn on_off(s: Option<&&str>, current: bool) -> Result<bool, String> {
    match s.copied() {
        None | Some("toggle") => Ok(!current),
        Some("on" | "true" | "1") => Ok(true),
        Some("off" | "false" | "0") => Ok(false),
        Some(v) => Err(format!("expected on or off, not {v}")),
    }
}

fn need_placement(s: &State) -> Result<(), String> {
    if s.placement.is_none() { Err("no schematic loaded (/horizonmatica load <name>)".into()) } else { Ok(()) }
}

fn run(game: &mut Game<'_>, args: &[&str]) -> Result<Vec<String>, String> {
    with(|s| load_settings(game, s));
    let sub = args.first().copied().unwrap_or("help");
    let rest = &args[args.len().min(1)..];
    let changed = |game: &mut Game<'_>| {
        with(|s| {
            s.mesh_key = None;
            if let Some(p) = &s.placement {
                save_last(game, p);
            }
        })
    };
    match sub {
        "help" | "?" => Ok(HELP.iter().map(|s| s.to_string()).collect()),
        "credits" | "about" => Ok(CREDITS.iter().map(|s| s.to_string()).collect()),
        "folder" => Ok(vec![format!("Schematics folder: {}", schematics_dir()?.display())]),
        "list" => {
            let dir = schematics_dir()?;
            let names = horizonmatica_core::list_schematics(&dir);
            if names.is_empty() {
                return Ok(vec![format!("No schematic files (.litematic, .schem, .schematic, .nbt) in {}", dir.display())]);
            }
            Ok(vec![format!("{} schematics: {}", names.len(), names.join(", "))])
        }
        "load" => {
            if rest.is_empty() {
                return Err("usage: /horizonmatica load <name>".into());
            }
            Ok(vec![load(game, &rest.join(" "), None)?])
        }
        "reload" => {
            let last = game.native_setting(ID, "last").ok_or("nothing loaded before")?;
            let (file, at) = parse_saved(&last).ok_or("the saved placement is unreadable")?;
            Ok(vec![load(game, &file, Some(at))?])
        }
        "unload" => {
            with(|s| {
                reset_caches(s);
                s.placement = None;
                s.settings.printer_on = false;
                s.moving = false;
                s.mesh = Mesh::default();
            });
            bridge::clear();
            // Gone from this world too: it doesn't come back on the next join.
            if let Some(key) = world_key(game) {
                game.native_set_setting(ID, &key, "");
            }
            Ok(vec!["Unloaded.".into()])
        }
        "info" | "verify" => with(|s| {
            need_placement(s)?;
            let p = s.placement.as_ref().unwrap();
            let (lo, hi) = p.bounds();
            let mut out = vec![format!(
                "{} — origin {} {} {}, rotation {}, mirror {}, box {:?}..{:?}, layer {}",
                p.file_name,
                p.origin[0],
                p.origin[1],
                p.origin[2],
                p.rotation.name(),
                p.mirror.name(),
                lo,
                hi,
                s.layer.describe()
            )];
            match &s.tally {
                Some(t) => out.push(format!(
                    "{:.1}% done: {} correct, {} missing, {} wrong block, {} wrong state, {} extra{}",
                    t.percent(),
                    t.correct,
                    t.missing,
                    t.wrong_block,
                    t.wrong_state,
                    t.extra,
                    if s.unloaded > 0 { format!(" ({} cells in unloaded chunks)", s.unloaded) } else { String::new() }
                )),
                None => out.push("Verifier: still scanning…".into()),
            }
            Ok(out)
        }),
        "origin" => {
            let at = match rest.first().copied() {
                Some("here") | None => feet(game).ok_or("not in a world")?,
                _ => [int(rest.first(), "x")?, int(rest.get(1), "y")?, int(rest.get(2), "z")?],
            };
            with(|s| {
                need_placement(s)?;
                s.placement.as_mut().unwrap().set_origin(at);
                s.scan = Scan::default();
                Ok::<_, String>(())
            })?;
            changed(game);
            Ok(vec![format!("Origin set to {} {} {}", at[0], at[1], at[2])])
        }
        "move" | "nudge" => {
            let d = [int(rest.first(), "dx")?, int(rest.get(1), "dy")?, int(rest.get(2), "dz")?];
            let o = with(|s| {
                need_placement(s)?;
                let p = s.placement.as_mut().unwrap();
                p.nudge(d);
                s.scan = Scan::default();
                Ok::<_, String>(p.origin)
            })?;
            changed(game);
            Ok(vec![format!("Origin now {} {} {}", o[0], o[1], o[2])])
        }
        "rotate" => {
            let r = with(|s| {
                need_placement(s)?;
                let p = s.placement.as_mut().unwrap();
                let r = match rest.first().copied() {
                    None | Some("cw") => p.rotation.then(Rotation::Clockwise90),
                    Some("ccw") => p.rotation.then(Rotation::CounterClockwise90),
                    Some(v) => Rotation::parse(v).ok_or("rotate cw, ccw, 180 or none")?,
                };
                p.set_rotation(r);
                s.scan = Scan::default();
                Ok::<_, String>(r)
            })?;
            changed(game);
            Ok(vec![format!("Rotation: {}", r.name())])
        }
        "mirror" => {
            let m = Mirror::parse(rest.first().copied().unwrap_or("none")).ok_or("mirror none, x or z")?;
            with(|s| {
                need_placement(s)?;
                s.placement.as_mut().unwrap().set_mirror(m);
                s.scan = Scan::default();
                Ok::<_, String>(())
            })?;
            changed(game);
            Ok(vec![format!("Mirror: {}", m.name())])
        }
        "layer" => {
            let feet_y = feet(game).map_or(64, |f| f[1]);
            let l = with(|s| {
                need_placement(s)?;
                let base = s.placement.as_ref().unwrap().bounds().0[1];
                let current = match s.layer {
                    LayerMode::All => base,
                    LayerMode::Single(y) | LayerMode::Above(y) | LayerMode::Below(y) | LayerMode::Range(y, _) => y,
                };
                s.layer = match rest.first().copied() {
                    None | Some("all") => LayerMode::All,
                    Some("up") => match s.layer {
                        LayerMode::All => LayerMode::Single(base),
                        l => l.shifted(1),
                    },
                    Some("down") => match s.layer {
                        LayerMode::All => LayerMode::Single(base),
                        l => l.shifted(-1),
                    },
                    Some("here") => LayerMode::Single(feet_y),
                    Some("above") => LayerMode::Above(rest.get(1).map_or(Ok(current), |_| int(rest.get(1), "y"))?),
                    Some("below") => LayerMode::Below(rest.get(1).map_or(Ok(current), |_| int(rest.get(1), "y"))?),
                    Some("range") => LayerMode::Range(int(rest.get(1), "min y")?, int(rest.get(2), "max y")?),
                    Some(_) => LayerMode::Single(int(rest.first(), "y")?),
                };
                s.mesh_key = None;
                Ok::<_, String>(s.layer)
            })?;
            Ok(vec![format!("Layers: {}", l.describe())])
        }
        "render" | "mode" | "through" | "xray" => {
            let msg = with(|s| {
                let st = &mut s.settings;
                let msg = match sub {
                    "render" => {
                        st.render = on_off(rest.first(), st.render)?;
                        format!("Rendering {}", if st.render { "on" } else { "off" })
                    }
                    "through" | "xray" => {
                        st.through = on_off(rest.first(), st.through)?;
                        format!("See through blocks {}", if st.through { "on" } else { "off" })
                    }
                    _ => {
                        st.mode = match rest.first().copied() {
                            Some("ghost" | "ghosts") => Mode::Ghost,
                            Some("box" | "boxes") => Mode::Boxes,
                            None => if st.mode == Mode::Ghost { Mode::Boxes } else { Mode::Ghost },
                            Some(v) => return Err(format!("mode ghost or boxes, not {v}")),
                        };
                        format!("Mode: {}", if st.mode == Mode::Ghost { "ghost blocks" } else { "boxes" })
                    }
                };
                s.mesh_key = None;
                Ok::<_, String>(msg)
            })?;
            with(|s| save_settings(game, &s.settings));
            Ok(vec![msg])
        }
        "materials" | "mats" => {
            let list = material_list(game);
            let shown = with(|s| {
                s.settings.materials_hud = !s.settings.materials_hud;
                save_settings(game, &s.settings);
                s.settings.materials_hud
            });
            if list.is_empty() {
                return Ok(vec!["No schematic loaded, or it needs no items.".into()]);
            }
            let mut out = vec![format!(
                "Materials ({} kinds; HUD list {}): item — missing / total (carried)",
                list.len(),
                if shown { "shown" } else { "hidden" }
            )];
            for m in list.iter().take(15) {
                out.push(format!("{} — {} / {} ({})", short(&m.item), m.missing, m.total, m.available));
            }
            if list.len() > 15 {
                out.push(format!("… and {} more", list.len() - 15));
            }
            Ok(out)
        }
        "printer" | "print" => {
            let msg = with(|s| {
                let st = &mut s.settings;
                let msg = match rest.first().copied() {
                    Some("speed") => {
                        st.printer.interval = int(rest.get(1), "ticks")?.clamp(1, 20) as u32;
                        format!("Printer: one block every {} ticks", st.printer.interval)
                    }
                    Some("reach") => {
                        let r: f64 = rest.get(1).ok_or("missing reach")?.parse().map_err(|_| "reach must be a number")?;
                        st.printer.reach = r.clamp(1.0, 6.0);
                        format!("Printer reach: {:.1}", st.printer.reach)
                    }
                    Some("air") => {
                        st.printer.air_place = on_off(rest.get(1), st.printer.air_place)?;
                        format!(
                            "Printer air place {}",
                            if st.printer.air_place { "ON — places without a neighbouring block" } else { "OFF" }
                        )
                    }
                    v => {
                        st.printer_on = on_off(v.as_ref(), st.printer_on)?;
                        s.recent.clear();
                        format!(
                            "Printer {}",
                            if st.printer_on { "ON — places missing blocks within reach" } else { "OFF" }
                        )
                    }
                };
                Ok::<_, String>(msg)
            })?;
            with(|s| save_settings(game, &s.settings));
            Ok(vec![msg])
        }
        other => Err(format!("unknown subcommand {other}; /horizonmatica help")),
    }
}

fn short(item: &str) -> &str {
    item.strip_prefix("minecraft:").unwrap_or(item)
}

// ---------------------------------------------------------------- keys

pub(super) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(ID) || !game.native_playing() {
        return;
    }
    let bound = |name: &str, default: &str| game.native_mod_key(ID, name, default) == key;
    let hit = KEYS.iter().find(|(n, _, d)| bound(n, d)).map(|(n, _, _)| *n);
    if hit == Some("move_mode") {
        toggle_move_mode(game);
        return;
    }
    if hit == Some("open_menu") {
        // As Litematica's main menu: always from its first page.
        // Horizonmatica's own screen; the mod's page in Options > Mods when it can't open.
        if !menus::open_screen(game) {
            with(|s| s.nav.reset());
            if !menus::open_page(game, ID) {
                say(game, "The menu is in Options > Mods > Horizonmatica");
            }
        }
        return;
    }
    // In move mode its keys come first (Page up/down and Home move instead of changing layers).
    if with(|s| s.moving) && move_key(game, key) {
        return;
    }
    let Some(action) = hit else { return };
    let args: &[&str] = match action {
        "toggle_render" => &["render"],
        "layer_up" => &["layer", "up"],
        "layer_down" => &["layer", "down"],
        "toggle_printer" => &["printer"],
        "toggle_materials" => &["materials"],
        "layer_mode" => {
            let next = next_layer_mode();
            let r = run(game, &["layer", next]);
            report(game, r);
            return;
        }
        _ => return,
    };
    if action == "toggle_materials" {
        // The HUD list only: don't fill the chat each press.
        with(|s| {
            s.settings.materials_hud = !s.settings.materials_hud;
            save_settings(game, &s.settings);
        });
        return;
    }
    let r = run(game, args);
    report(game, r);
}

// ---------------------------------------------------------------- move mode

fn toggle_move_mode(game: &mut Game<'_>) {
    let on = with(|s| {
        s.placement.as_ref()?;
        s.moving = !s.moving;
        s.wheel = 0.0;
        Some(s.moving)
    });
    match on {
        None => say(game, "Move mode: no schematic loaded"),
        Some(true) => say(
            game,
            "Move mode ON: wheel = along your view, arrows = horizontal, Page up/down = up/down,              R = rotate, Home = to you, Shift = 5 blocks. N or Enter: done",
        ),
        Some(false) => {
            with(|s| {
                if let Some(p) = &s.placement {
                    save_last(game, p);
                }
            });
            say(game, "Move mode OFF: schematic locked")
        }
    }
}

/// Blocks per step: 5 with Shift held.
fn move_step(game: &Game<'_>) -> i32 {
    use winit::keyboard::KeyCode;
    if game.keys.contains(&KeyCode::ShiftLeft) || game.keys.contains(&KeyCode::ShiftRight) { 5 } else { 1 }
}

/// The unit axis closest to `dir`.
fn dominant(dir: [f64; 3]) -> [i32; 3] {
    let i = (0..3).max_by(|&a, &b| dir[a].abs().total_cmp(&dir[b].abs())).unwrap_or(0);
    let mut out = [0; 3];
    out[i] = if dir[i] < 0.0 { -1 } else { 1 };
    out
}

/// Moves the placement by `d` without a chat line (the HUD shows the origin).
fn shift(d: [i32; 3]) {
    with(|s| {
        if let Some(p) = s.placement.as_mut() {
            p.nudge(d);
            s.scan = Scan::default();
            s.mesh_key = None;
        }
    });
}

/// A key in move mode: whether it was one of move mode's.
fn move_key(game: &mut Game<'_>, key: &str) -> bool {
    let k = move_step(game);
    let view = super::view_vector(game.yaw, 0.0);
    let fwd = dominant([view[0], 0.0, view[2]]);
    let right = [-fwd[2], 0, fwd[0]];
    let d = match key {
        "key.keyboard.up" => fwd,
        "key.keyboard.down" => fwd.map(|v| -v),
        "key.keyboard.right" => right,
        "key.keyboard.left" => right.map(|v| -v),
        "key.keyboard.page.up" => [0, 1, 0],
        "key.keyboard.page.down" => [0, -1, 0],
        "key.keyboard.r" => {
            let r = run(game, &["rotate", "cw"]);
            if let Err(e) = r {
                say(game, &format!("Error: {e}"));
            }
            return true;
        }
        "key.keyboard.home" => {
            let r = run(game, &["origin", "here"]);
            if let Err(e) = r {
                say(game, &format!("Error: {e}"));
            }
            return true;
        }
        "key.keyboard.enter" | "key.keyboard.keypad.enter" => {
            toggle_move_mode(game);
            return true;
        }
        _ => return false,
    };
    shift(d.map(|v| v * k));
    true
}

/// The wheel in move mode: the placement moves along the axis you look along, away from you
/// when scrolled up. Claimed, so the hotbar stays put.
pub(super) fn wheel(game: &mut Game<'_>, steps: f32, claimed: &mut bool) {
    if !game.native_on(ID) || !game.native_playing() || !with(|s| s.moving && s.placement.is_some()) {
        return;
    }
    *claimed = true;
    let n = with(|s| {
        s.wheel += steps;
        let n = s.wheel.trunc();
        s.wheel -= n;
        n as i32
    });
    if n == 0 {
        return;
    }
    let axis = dominant(super::view_vector(game.yaw, game.pitch));
    let k = n * move_step(game);
    shift(axis.map(|v| v * k));
}

fn report(game: &mut Game<'_>, r: Result<Vec<String>, String>) {
    match r {
        Ok(lines) => {
            for l in lines {
                say(game, &l);
            }
        }
        Err(e) => say(game, &format!("Error: {e}")),
    }
}

// ---------------------------------------------------------------- per tick

pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() || !game.native_on(ID) {
        return;
    }
    if game.player.is_none() {
        return;
    }
    with(|s| load_settings(game, s));
    restore_step(game);
    scan_step(game);
    rebuild_mesh(game);
    printer_step(game);
}

/// Leaving a world, or the mod switched off: the overlay is dropped. Otherwise the render side
/// gets the current draw options (they need no new geometry).
pub(super) fn frame(game: &mut Game<'_>) {
    let active = game.player.is_some() && game.native_on(ID);
    with(|s| {
        if !active {
            if !s.mesh.built.is_empty() || s.mesh_key.is_some() {
                s.mesh = Mesh::default();
                s.mesh_key = None;
                bridge::clear();
            }
            return;
        }
        bridge::options(s.settings.through, s.settings.render && s.placement.is_some());
    });
}

fn scan_step(game: &Game<'_>) {
    with(|s| {
        let Some(p) = &s.placement else { return };
        let revision = p.revision;
        let (lo, hi) = p.bounds();
        let size = [0, 1, 2].map(|i| (hi[i] - lo[i] + 1) as u64);
        let volume = size[0] * size[1] * size[2];
        if s.scan.revision != revision {
            s.scan = Scan { revision, ..Scan::default() };
        }
        let end = (s.scan.index + SCAN_PER_TICK).min(volume);
        let mut i = s.scan.index;
        while i < end {
            let x = (i % size[0]) as i32;
            let z = ((i / size[0]) % size[2]) as i32;
            let y = (i / (size[0] * size[2])) as i32;
            i += 1;
            let w = [lo[0] + x, lo[1] + y, lo[2] + z];
            if !loaded(game, w) {
                if s.placement.as_ref().unwrap().wanted(w).is_some_and(|b| !b.is_air()) {
                    s.scan.unloaded += 1;
                }
                continue;
            }
            let Some((status, wanted)) = status_at(game, s, w) else { continue };
            s.scan.tally.add(status);
            if matches!(status, Status::Missing | Status::WrongBlock) {
                let per = u64::from(materials::items_for_state(&wanted));
                if per > 0
                    && let Some(item) = item_for(game, s, &wanted)
                {
                    *s.scan.missing.entry(item).or_insert(0) += per;
                }
            }
        }
        s.scan.index = i;
        if i >= volume {
            let done = std::mem::replace(&mut s.scan, Scan { revision, ..Scan::default() });
            s.tally = Some(done.tally);
            s.missing = done.missing;
            s.unloaded = done.unloaded;
        }
    });
}

fn argb_to_vertex(argb: u32, shade: f32) -> u32 {
    let [a, r, g, b] = argb.to_be_bytes();
    let f = |c: u8| (f32::from(c) * shade).round().clamp(0.0, 255.0) as u32;
    f(r) | f(g) << 8 | f(b) << 16 | u32::from(a) << 24
}

/// A box's six faces as quads (positions relative to the anchor), on the atlas's white texel.
fn push_box(out: &mut Vec<Vertex>, white: [f32; 2], min: [f32; 3], max: [f32; 3], argb: u32) {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    let faces: [([[f32; 3]; 4], f32); 6] = [
        ([[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]], 1.0),
        ([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]], 0.5),
        ([[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]], 0.8),
        ([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], 0.8),
        ([[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]], 0.6),
        ([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]], 0.6),
    ];
    for (corners, shade) in faces {
        let color = argb_to_vertex(argb, shade);
        for pos in corners {
            out.push(Vertex {
                pos,
                uv: white,
                layer: 0,
                color,
                light: FULL_BRIGHT,
                normal: [0; 4],
                glint_uv: [0.0, 0.0],
            });
        }
    }
}

/// A block face's colour with vanilla's directional shade (`BlockModelShaper`: up 1, down 0.5,
/// north/south 0.8, east/west 0.6), from its normal.
fn shade(color: u32, normal: [i8; 4]) -> u32 {
    let [nx, ny, nz, _] = normal.map(i32::from);
    let shade = if ny > 64 {
        255
    } else if ny < -64 {
        127
    } else if nz.abs() >= nx.abs() {
        204
    } else {
        153
    };
    let [r, g, b, a] = color.to_le_bytes();
    let f = |c: u8| (u32::from(c) * shade / 255) as u8;
    u32::from_le_bytes([f(r), f(g), f(b), a])
}

/// The world's section `k` (chunk x, section y, chunk z), shared: the world copies a section
/// before writing to it while someone else holds it, so a different pointer later means it
/// changed (or its chunk was loaded again).
fn world_section(game: &Game<'_>, k: [i32; 3]) -> Option<Arc<horizon_client::chunk::Section>> {
    let chunk = game.world.chunks.get(&(k[0], k[2]))?;
    let i = chunk.section_index(k[1] * 16)?;
    chunk.sections.get(i).cloned()
}

fn same_section(a: &Option<Arc<horizon_client::chunk::Section>>, b: &Option<Arc<horizon_client::chunk::Section>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Time the overlay may take per tick to build sections.
const MESH_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);

/// Keeps the overlay up to date, one 16x16x16 section at a time: a section is built again only
/// when its world section changed (a block set, a chunk loaded) or what it shows did (the
/// placement, layers, display settings), nearest first, and only as many per tick as fit in
/// `MESH_BUDGET`. The render side keeps each section's buffer, so a change uploads one section.
fn rebuild_mesh(game: &Game<'_>) {
    let Some(me) = feet(game) else { return };
    let start = std::time::Instant::now();
    let Some((key, (lo, hi), radius)) = with(|s| {
        let p = s.placement.as_ref()?;
        let st = &s.settings;
        let key = (p.revision, s.layer, st.mode, st.show_extra, st.show_wrong, st.strict, st.power, st.alpha.to_bits());
        Some((key, p.bounds(), st.radius))
    }) else {
        if with(|s| std::mem::take(&mut s.mesh).built.len()) > 0 {
            bridge::clear();
        }
        return;
    };
    // What every section shows changed: start over from the nearest.
    if with(|s| s.mesh_key != Some(key)) {
        with(|s| {
            s.mesh_key = Some(key);
            s.mesh = Mesh { anchor: lo, ..Mesh::default() };
        });
        bridge::clear();
        bridge::set_anchor(lo);
    }
    let a = [0, 1, 2].map(|i| lo[i].max(me[i] - radius) >> 4);
    let b = [0, 1, 2].map(|i| hi[i].min(me[i] + radius) >> 4);
    // Out of range: dropped.
    let gone: Vec<[i32; 3]> =
        with(|s| s.mesh.built.keys().filter(|k| (0..3).any(|i| k[i] < a[i] || k[i] > b[i])).copied().collect());
    for k in gone {
        with(|s| {
            if let Some((_, n)) = s.mesh.built.remove(&k) {
                s.mesh.quads -= n;
            }
        });
        bridge::remove_section(k);
    }
    // New or changed: built again, nearest first.
    let mut todo = Vec::new();
    with(|s| {
        for sy in a[1]..=b[1] {
            for sz in a[2]..=b[2] {
                for sx in a[0]..=b[0] {
                    let k = [sx, sy, sz];
                    let now = world_section(game, k);
                    let stale = s.mesh.built.get(&k).is_none_or(|(snap, _)| !same_section(snap, &now));
                    if stale {
                        let d = [0, 1, 2].map(|i| i64::from(k[i] * 16 + 8 - me[i]));
                        todo.push((d[0] * d[0] + d[1] * d[1] + d[2] * d[2], k, now));
                    }
                }
            }
        }
    });
    todo.sort_by_key(|t| t.0);
    for (_, k, snap) in todo {
        if with(|s| s.mesh.quads >= MAX_QUADS && !s.mesh.built.contains_key(&k)) {
            with(|s| s.printer_note = format!("overlay full ({MAX_QUADS} faces): lower the render radius"));
            break;
        }
        let quads = build_section(game, k);
        let n = quads.len() / 4;
        with(|s| {
            let old = s.mesh.built.insert(k, (snap, n)).map_or(0, |(_, n)| n);
            s.mesh.quads = s.mesh.quads + n - old;
        });
        bridge::put_section(k, quads);
        if start.elapsed() > MESH_BUDGET {
            break;
        }
    }
}

/// The overlay's quads for section `k`: ghosts of missing blocks (the faces between two ghost
/// cubes left out), boxes for the rest, each quad with its outward normal for the render side.
fn build_section(game: &Game<'_>, k: [i32; 3]) -> Vec<Vertex> {
    let mut quads = Vec::new();
    let white = horizon_client::gfx::lightning::white_uv(&game.gui.atlas.font.white());
    with(|s| {
        let Some(p) = s.placement.as_ref() else { return };
        let (lo, hi) = p.bounds();
        let anchor = s.mesh.anchor;
        let ghost_alpha = (s.settings.alpha.clamp(0.05, 1.0) * 255.0) as u32;
        let (mode, show_extra, show_wrong) = (s.settings.mode, s.settings.show_extra, s.settings.show_wrong);
        let layer = s.layer;
        let a = [0, 1, 2].map(|i| (k[i] * 16).max(lo[i]));
        let b = [0, 1, 2].map(|i| (k[i] * 16 + 15).min(hi[i]));
        // Per quad: the block it belongs to, and whether that block is a ghost cube.
        let mut blocks: Vec<([i32; 3], bool)> = Vec::new();
        for y in a[1]..=b[1] {
            if !layer.contains(y) {
                continue;
            }
            for z in a[2]..=b[2] {
                for x in a[0]..=b[0] {
                    let w = [x, y, z];
                    if !loaded(game, w) {
                        continue;
                    }
                    let Some((status, _, res)) = quick_status(game, s, w) else { continue };
                    let rel = [0, 1, 2].map(|i| (w[i] - anchor[i]) as f32);
                    let inflated = |e: f32| ([rel[0] - e, rel[1] - e, rel[2] - e], [rel[0] + 1.0 + e, rel[1] + 1.0 + e, rel[2] + 1.0 + e]);
                    let mut cube = false;
                    match status {
                        Status::Correct => {}
                        Status::Missing => {
                            let start = quads.len();
                            if mode == Mode::Ghost
                                && let Some(id) = res.id
                            {
                                let at = glam::Mat4::from_translation(glam::Vec3::from_array(rel));
                                // With `diffuse` each face comes with its own normal (a repeater's
                                // top lies below the block's middle and still faces up); the
                                // directional shade the game would bake in is added here.
                                game.block_model_vertices(id, 0, at, FULL_BRIGHT, true, &mut quads);
                                for v in &mut quads[start..] {
                                    v.color = shade(v.color, v.normal);
                                }
                                // Chests, shulker boxes, banners, heads, bells...: the block
                                // entity's model (a double chest's half by its `type`).
                                game.special_block_model(id, at, 0xFF, &mut quads);
                                // The ghost's opacity in the colour's alpha.
                                for v in &mut quads[start..] {
                                    v.color = (v.color & 0x00FF_FFFF) | ghost_alpha << 24;
                                }
                                cube = res.cube;
                            }
                            if quads.len() == start {
                                let (lo, hi) = inflated(-0.2);
                                push_box(&mut quads, white, lo, hi, Status::Missing.overlay_argb() | 0x6000_0000);
                            }
                        }
                        Status::Extra if show_extra => {
                            let (lo, hi) = inflated(0.004);
                            push_box(&mut quads, white, lo, hi, Status::Extra.overlay_argb());
                        }
                        Status::WrongBlock | Status::WrongState if show_wrong => {
                            let (lo, hi) = inflated(0.004);
                            push_box(&mut quads, white, lo, hi, status.overlay_argb());
                        }
                        _ => {}
                    }
                    while blocks.len() < quads.len() / 4 {
                        blocks.push((w, cube));
                    }
                }
            }
        }
        quads.truncate(blocks.len() * 4);
        // Whether `w` shows a ghost cube (its faces towards another one are hidden).
        let mut ghost_cube = |w: [i32; 3]| {
            mode == Mode::Ghost
                && layer.contains(w[1])
                && loaded(game, w)
                && quick_status(game, s, w).is_some_and(|(st, _, res)| st == Status::Missing && res.cube && res.id.is_some())
        };
        let mut kept = Vec::with_capacity(quads.len());
        for (q, &(w, cube)) in quads.chunks_exact(4).zip(&blocks) {
            let block = glam::Vec3::new((w[0] - anchor[0]) as f32 + 0.5, (w[1] - anchor[1]) as f32 + 0.5, (w[2] - anchor[2]) as f32 + 0.5);
            let p = [0, 1, 2, 3].map(|i| glam::Vec3::from_array(q[i].pos));
            let centre = (p[0] + p[1] + p[2] + p[3]) * 0.25;
            let out = centre - block;
            let given = q[0].normal;
            let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            // The model's own normal where it gives one (two-sided faces, w = -127, never hide);
            // for the overlay's boxes, away from the block's centre (their faces are its sides).
            // A face through the centre (a plant's cross) gets none and is always drawn.
            let normal = if given[3] == -127 {
                glam::Vec3::ZERO
            } else if given[..3] != [0, 0, 0] {
                glam::Vec3::new(f32::from(given[0]), f32::from(given[1]), f32::from(given[2])).normalize_or_zero()
            } else if out.length_squared() < 1e-4 || normal.dot(out).abs() < 1e-3 {
                glam::Vec3::ZERO
            } else if normal.dot(out) < 0.0 {
                -normal
            } else {
                normal
            };
            // A ghost cube's face on its boundary, against another ghost cube: hidden.
            if cube && normal != glam::Vec3::ZERO {
                let axis = (0..3).find(|&i| normal[i].abs() > 0.99);
                if let Some(i) = axis
                    && (out[i].abs() - 0.5).abs() < 1e-3
                {
                    let mut n = w;
                    n[i] += normal[i].signum() as i32;
                    if ghost_cube(n) {
                        continue;
                    }
                }
            }
            // `normal.w = 127`: the render side drops the quad while it faces away.
            let packed = if normal == glam::Vec3::ZERO {
                [0; 4]
            } else {
                let n = normal * 127.0;
                [n.x.round() as i8, n.y.round() as i8, n.z.round() as i8, 127]
            };
            for v in q {
                let mut v = *v;
                v.normal = packed;
                kept.push(v);
            }
        }
        quads = kept;
    });
    quads
}

// ---------------------------------------------------------------- printer

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
        || ["_door", "_trapdoor", "_fence_gate", "_button", "_bed", "_sign", "_hanging_sign", "_shulker_box", "chest", "barrel", "furnace", "smoker", "hopper", "dispenser", "dropper", "potted_", "candle", "brewing_stand", "_cake"]
            .iter()
            .any(|p| name.contains(p))
}

fn face_dir(f: [i32; 3]) -> Direction {
    match f {
        [0, 1, 0] => Direction::Up,
        [0, -1, 0] => Direction::Down,
        [0, 0, -1] => Direction::North,
        [0, 0, 1] => Direction::South,
        [-1, 0, 0] => Direction::West,
        _ => Direction::East,
    }
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

/// A click the printer makes: the look the server is told, where it lands, and whether it is
/// made sneaking (placing against a chest or door, or a chest that mustn't join its neighbour).
struct Click {
    yaw: f32,
    pitch: f32,
    hit: super::Hit,
    sneak: bool,
}

fn printer_step(game: &mut Game<'_>) {
    let now = game.ticks;
    // On a server, the click went out last tick: give it the player's real look back.
    if with(|s| std::mem::take(&mut s.server_look)) {
        game.forced_hit = None;
        let (yaw, pitch) = (game.yaw, game.pitch);
        send_look(game, yaw, pitch);
    }
    let (on, settings) = with(|s| (s.settings.printer_on && s.placement.is_some() && !s.moving, s.settings.printer));
    if !on || !game.native_playing() || now < with(|s| s.next_print) {
        return;
    }
    let Some(eye) = game.eye_position() else { return };
    let reach = settings.reach.min(game.reach());
    let layer = with(|s| s.layer);
    let feet = [eye[0].floor() as i32, (eye[1] - 1.62).floor() as i32, eye[2].floor() as i32];

    // Everything there is to do in reach, bottom up and nearest first...
    let mut jobs: Vec<(u8, usize, [i32; 3], BlockState, Job)> = Vec::new();
    let mut waiting = None;
    for (order, p) in printer::positions_in_reach(eye, reach).into_iter().enumerate() {
        if jobs.len() >= 256 {
            break;
        }
        if !layer.contains(p[1]) || !loaded(game, p) || with(|s| s.recent.blocked(p, now, settings.retry_ticks)) {
            continue;
        }
        let Some((status, wanted)) = with(|s| status_at(game, s, p)) else { continue };
        if wanted.is_air() || matches!(wanted.name.as_str(), "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column") {
            continue;
        }
        let job = match status {
            Status::Missing => {
                // The other half of a door or bed comes with its first half.
                if materials::items_for_state(&wanted) == 0 || p == feet || p == [feet[0], feet[1] + 1, feet[2]] {
                    continue;
                }
                // Sand, gravel, anvils...: only onto something, or they fall.
                if printer::falls(&wanted) {
                    let below = [p[0], p[1] - 1, p[2]];
                    let id = game.world.block(below[0], below[1], below[2]);
                    let facts = game.res.facts();
                    let f = facts.get(id);
                    let free = facts.is_air(id) || f.replaceable || (f.fluid != horizon_client::blocks::Fluid::None && !f.waterlogged);
                    if free {
                        let comes = with(|s| status_at(game, s, below)).is_some_and(|(_, w)| !w.is_air());
                        waiting = Some(if comes {
                            format!("{} at {} {} {} waits for the block below", short(&wanted.name), p[0], p[1], p[2])
                        } else {
                            format!("{} at {} {} {} would fall: nothing below in the schematic", short(&wanted.name), p[0], p[1], p[2])
                        });
                        continue;
                    }
                }
                // An observer waits for the block it watches, or placing that would fire it;
                // not when that block couldn't be placed (it may need the observer to stand on,
                // like a repeater on an observer looking up).
                if let Some(d) = printer::observed(&wanted) {
                    let watched = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    let stuck = with(|s| s.recent.blocked(watched, now, settings.retry_ticks));
                    if !stuck
                        && with(|s| status_at(game, s, watched)).is_some_and(|(st, w)| st == Status::Missing && !w.is_air())
                    {
                        waiting = Some(format!("observer at {} {} {} waits for what it watches", p[0], p[1], p[2]));
                        continue;
                    }
                }
                Job::Place
            }
            Status::WrongState => {
                let actual_id = game.world.block(p[0], p[1], p[2]);
                let actual = with(|s| game_state(game, &mut s.states, actual_id));
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
        jobs.push((printer::place_class(&wanted), order, p, wanted, job));
    }
    // ...done in redstone's order: plain blocks, then what carries a signal, then what makes one.
    jobs.sort_by_key(|(class, order, ..)| (*class, *order));

    let mut note = waiting.unwrap_or_default();
    for (_, _, p, wanted, job) in jobs.into_iter().take(12) {
        let item = match &job {
            Job::Adjust(_) => None,
            _ => match with(|s| item_for(game, s, &wanted)) {
                Some(item) => Some(item),
                None => continue,
            },
        };
        if let Some(item) = &item {
            match select(game, item) {
                Err(e) => {
                    note = format!("{}: {e}", short(item));
                    with(|s| s.recent.mark(p, now));
                    continue;
                }
                // Through a server (also the integrated one of a single-player world) the click
                // would go out before the slot change and place what was in hand before: the
                // change goes out this tick, the click the next.
                Ok(true) if game.remote.is_some() => {
                    with(|s| s.printer_note = format!("taking {} in hand", short(item)));
                    return;
                }
                Ok(_) => {}
            }
        }
        let click = match (&job, &item) {
            (Job::Place, Some(item)) => aim_for(game, eye, reach, p, &wanted, item, settings.air_place),
            (Job::Stack, Some(item)) => aim_stack(game, eye, reach, p, &wanted, item, settings.air_place),
            (Job::Adjust(_), _) => aim_block(game, eye, reach, p),
            _ => None,
        };
        let Some(click) = click else {
            with(|s| s.recent.mark(p, now));
            note = format!("no way to place {} at {} {} {} as wanted", short(&wanted.name), p[0], p[1], p[2]);
            continue;
        };
        if let Err(e) = do_click(game, &click) {
            with(|s| s.printer_note = format!("click failed: {e}"));
            return;
        }
        let what = match job {
            Job::Place => format!("placed {}", short(&wanted.name)),
            Job::Stack => format!("stacked {}", short(&wanted.name)),
            Job::Adjust(n) => format!("set up {} ({n} click{} left)", short(&wanted.name), if n == 1 { "" } else { "s" }),
        };
        with(|s| {
            // An adjusted block gets its next click soon; on a server once its answer is in.
            if !matches!(job, Job::Adjust(_)) {
                s.placed += 1;
                s.recent.mark(p, now);
            } else if game.remote.is_some() {
                s.recent.mark(p, now);
            }
            s.next_print = now + u64::from(settings.interval.max(1));
            s.printer_note = format!("{what} at {} {} {}", p[0], p[1], p[2]);
        });
        return;
    }
    if !note.is_empty() {
        with(|s| s.printer_note = note);
    }
}

/// Makes `click`: the camera never moves (like litematica-printer, only the click carries the
/// look). Locally it is used within this tick, so the view is back before the next frame; on a
/// server the look is sent just before it and the real one the next tick.
fn do_click(game: &mut Game<'_>, click: &Click) -> Result<(), String> {
    use winit::keyboard::KeyCode;
    if game.remote.is_some() {
        send_look(game, click.yaw, click.pitch);
        game.forced_hit = Some(click.hit);
        let _ = game.play_mouse(winit::event::MouseButton::Right, true);
        let _ = game.play_mouse(winit::event::MouseButton::Right, false);
        with(|s| s.server_look = true);
        return Ok(());
    }
    let before = (game.yaw, game.pitch);
    // Sneaking is read from the held Shift key: held for a sneaking click, let go otherwise
    // (a player holding Shift would place against chests instead of setting a repeater).
    let shift_held = game.keys.contains(&KeyCode::ShiftLeft);
    if click.sneak {
        game.keys.insert(KeyCode::ShiftLeft);
    } else {
        game.keys.remove(&KeyCode::ShiftLeft);
    }
    game.yaw = click.yaw;
    game.pitch = click.pitch;
    game.forced_hit = Some(click.hit);
    let used = game.use_with_hand();
    game.forced_hit = None;
    game.use_delay = 4;
    game.yaw = before.0;
    game.pitch = before.1;
    if shift_held {
        game.keys.insert(KeyCode::ShiftLeft);
    } else {
        game.keys.remove(&KeyCode::ShiftLeft);
    }
    used.map_err(|e| e.to_string())
}

/// Tells the server the player looks along `yaw`/`pitch` (`ServerboundMovePlayerPacket.Rot`).
fn send_look(game: &Game<'_>, yaw: f32, pitch: f32) {
    use horizon_protocol::packets::play::{self, Serverbound as S};
    let (on_ground, collision) =
        game.player.as_ref().map_or((false, false), |p| (p.ground.on_ground, p.collision.horizontal));
    game.send(S::MovePlayerRot(play::MovePlayerRot {
        yaw,
        pitch,
        flags: u8::from(on_ground) | u8::from(collision) << 1,
    }));
}

/// Puts `item` in hand from the hotbar. In creative only the hotbar is used: an empty slot, or
/// one whose item the schematic doesn't need any more, or the one the printer used longest ago,
/// gets the item (the rest of the inventory stays as it is). Whether the hand changed.
fn select(game: &mut Game<'_>, item: &str) -> Result<bool, String> {
    let now = game.ticks;
    if let Some(i) = (0..9).find(|&i| game.inventory[i].as_ref().is_some_and(|s| &*s.item == item)) {
        let changed = game.selected != i;
        game.selected = i;
        with(|s| s.hotbar_used[i] = now);
        return Ok(changed);
    }
    if !game.creative {
        return Err(if game.inventory.iter().flatten().any(|s| &*s.item == item) {
            "in the inventory but not the hotbar".into()
        } else {
            "none carried".into()
        });
    }
    let needed: std::collections::HashSet<String> = with(|s| {
        let left = if s.tally.is_some() { &s.missing } else { &s.totals };
        left.iter().filter(|(_, n)| **n > 0).map(|(k, _)| k.clone()).collect()
    });
    let used = with(|s| s.hotbar_used);
    let slot = (0..9)
        .find(|&i| game.inventory[i].is_none())
        .or_else(|| {
            (0..9)
                .filter(|&i| game.inventory[i].as_ref().is_some_and(|s| !needed.contains(&*s.item)))
                .min_by_key(|&i| used[i])
        })
        .or_else(|| (0..9).min_by_key(|&i| used[i]))
        .unwrap_or(0);
    // A creative edit of the slot (a server is told by the inventory sync).
    game.inventory[slot] = Some(horizon_client::items::ItemStack::new(item, 1));
    game.selected = slot;
    with(|s| s.hotbar_used[slot] = now);
    Ok(true)
}

/// Asks the game's own placement what a click would place; whether that is (on the way to)
/// `wanted` at `p`.
#[allow(clippy::too_many_arguments)]
fn would_place(
    game: &mut Game<'_>,
    p: [i32; 3],
    wanted: &BlockState,
    wanted_id: BlockStateId,
    item: &str,
    hit: &super::Hit,
    yaw: f32,
    pitch: f32,
    sneak: bool,
) -> bool {
    let ctx = PlaceContext {
        clicked_pos: BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
        face: face_dir(hit.face),
        hit: Vec3::new(hit.point[0], hit.point[1], hit.point[2]),
        inside: false,
        yaw,
        pitch,
        sneaking: sneak,
        item: item.to_string(),
        can_use_game_master_blocks: game.can_use_game_master_blocks(),
    };
    let Some((pos, state)) = game.with_level(|b, level, ticks| b.placement_state(level, ticks, &ctx)) else {
        return false;
    };
    if [pos.x, pos.y, pos.z] != p {
        return false;
    }
    if state == wanted_id {
        return true;
    }
    let got = with(|s| game_state(game, &mut s.states, state));
    verify::placement_ok(wanted, &got)
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
/// the neighbours (sneaking too, when not on a server: then chests and doors can be clicked,
/// and a chest stays single), asking the game's own `placement_state` what each would place.
/// With `air`, a spot no neighbour can be clicked for is clicked itself ("air place": the
/// clicked block is replaceable, so `BlockPlaceContext` places into it), with any look.
fn aim_for(game: &mut Game<'_>, eye: [f64; 3], reach: f64, p: [i32; 3], wanted: &BlockState, item: &str, air: bool) -> Option<Click> {
    let wanted_id = with(|s| wanted_id(game, s, wanted))?;
    const OFFSETS: [f64; 5] = [0.0, -0.3, 0.3, -0.42, 0.42];
    let sneaks: &[bool] = if game.remote.is_some() { &[false] } else { &[false, true] };
    for &sneak in sneaks {
        for f in printer::FACES {
            let support = [p[0] + f[0], p[1] + f[1], p[2] + f[2]];
            let sid = game.world.block(support[0], support[1], support[2]);
            let facts = game.res.facts();
            let sf = facts.get(sid);
            if facts.is_air(sid)
                || sf.replaceable
                || (sf.fluid != horizon_client::blocks::Fluid::None && !sf.waterlogged)
                || sf.outline.is_empty()
                || (!sneak && interactive(&sf.name))
            {
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
                    let (yaw, pitch) = super::agent::yaw_pitch(eye, point);
                    let pitch = pitch.clamp(-90.0, 90.0);
                    let dir = super::view_vector(yaw, pitch);
                    let Some(hit) = game.clip_ray(eye, dir, reach) else { continue };
                    if hit.pos != support && hit.pos != p {
                        continue;
                    }
                    if would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, sneak) {
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
                let hit = super::Hit { pos: p, face: d, point };
                let (ny, np) = super::agent::yaw_pitch(eye, point);
                let natural = std::iter::once((ny, np.clamp(-90.0, 90.0)));
                let any = (0..16).flat_map(|i| {
                    [0.0f32, 50.0, -50.0, 90.0, -90.0].map(move |pitch| (i as f32 * 22.5 - 180.0, pitch))
                });
                for (yaw, pitch) in natural.chain(any) {
                    if would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, sneak) {
                        return Some(Click { yaw, pitch, hit, sneak });
                    }
                }
            }
        }
    }
    None
}

/// A click on the block at `p` itself with more of its item, that makes it `wanted` (the top
/// half onto a bottom slab, another candle...): looked at through its own shape, else (with
/// `air`) aimed straight at a face.
fn aim_stack(game: &mut Game<'_>, eye: [f64; 3], reach: f64, p: [i32; 3], wanted: &BlockState, item: &str, air: bool) -> Option<Click> {
    let wanted_id = with(|s| wanted_id(game, s, wanted))?;
    const OFFSETS: [f64; 3] = [0.0, -0.3, 0.3];
    for f in printer::FACES {
        let centre = [0, 1, 2].map(|i| f64::from(p[i]) + 0.5 + f64::from(f[i]) * 0.5);
        let (_, u, v) = face_axes(f);
        for du in OFFSETS {
            for dv in OFFSETS {
                let mut point = centre;
                point[u] += du;
                point[v] += dv;
                let (yaw, pitch) = super::agent::yaw_pitch(eye, point);
                let pitch = pitch.clamp(-90.0, 90.0);
                let dir = super::view_vector(yaw, pitch);
                if let Some(hit) = game.clip_ray(eye, dir, reach)
                    && hit.pos == p
                    && would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, false)
                {
                    return Some(Click { yaw, pitch, hit, sneak: false });
                }
                if air {
                    let hit = super::Hit { pos: p, face: f, point };
                    if would_place(game, p, wanted, wanted_id, item, &hit, yaw, pitch, false) {
                        return Some(Click { yaw, pitch, hit, sneak: false });
                    }
                }
            }
        }
    }
    None
}

/// A plain (not sneaking) click on the block at `p`, to use it: a repeater's delay, a
/// comparator's mode, a note block's note.
fn aim_block(game: &mut Game<'_>, eye: [f64; 3], reach: f64, p: [i32; 3]) -> Option<Click> {
    let centre = [f64::from(p[0]) + 0.5, f64::from(p[1]) + 0.1, f64::from(p[2]) + 0.5];
    let (yaw, pitch) = super::agent::yaw_pitch(eye, centre);
    let pitch = pitch.clamp(-90.0, 90.0);
    let dir = super::view_vector(yaw, pitch);
    let hit = match game.clip_ray(eye, dir, reach) {
        Some(hit) if hit.pos == p => hit,
        _ => super::Hit { pos: p, face: [0, 1, 0], point: centre },
    };
    Some(Click { yaw, pitch, hit, sneak: false })
}

// ---------------------------------------------------------------- HUD

fn inventory_counts(game: &Game<'_>) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for s in game.inventory.iter().flatten() {
        *m.entry(s.item.to_string()).or_insert(0) += u64::from(s.count);
    }
    m
}

fn material_list(game: &Game<'_>) -> Vec<Material> {
    let inv = inventory_counts(game);
    with(|s| {
        if s.placement.is_none() {
            return Vec::new();
        }
        let missing = if s.tally.is_some() { s.missing.clone() } else { s.totals.clone() };
        materials::build_list(&s.totals, &missing, &inv)
    })
}

pub(super) fn hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    if !game.native_on(ID) {
        return;
    }
    let lines = with(|s| {
        load_settings(game, s);
        let p = s.placement.as_ref()?;
        if !s.settings.hud && !s.moving {
            return Some(Vec::new());
        }
        let name = if p.schematic.metadata.name.is_empty() { p.file_name.clone() } else { p.schematic.metadata.name.clone() };
        let mut l = vec![(format!("Horizonmatica: {name}"), 0x55FFFFu32)];
        if s.moving {
            l.push(("MOVE MODE · wheel/arrows/PgUp/PgDn · R rotate · N done".into(), 0xFFFF55));
        }
        l.push((
            format!(
                "Origin {} {} {} · {} · mirror {}",
                p.origin[0],
                p.origin[1],
                p.origin[2],
                p.rotation.name(),
                p.mirror.name()
            ),
            0xE0E0E0,
        ));
        l.push((
            format!("Layers: {}{}", s.layer.describe(), if s.settings.render { "" } else { " (hidden)" }),
            0xE0E0E0,
        ));
        match &s.tally {
            Some(t) => {
                l.push((format!("{:.1}% done · {} missing", t.percent(), t.missing), 0x55FF55));
                if t.wrong_block + t.wrong_state + t.extra > 0 {
                    l.push((
                        format!("{} wrong block · {} wrong state · {} extra", t.wrong_block, t.wrong_state, t.extra),
                        0xFFAA00,
                    ));
                }
            }
            None => l.push(("Verifying…".into(), 0xAAAAAA)),
        }
        if s.settings.printer_on {
            l.push((format!("Printer ON · {} placed", s.placed), 0xFF55FF));
            if !s.printer_note.is_empty() {
                l.push((s.printer_note.clone(), 0xAAAAAA));
            }
        }
        Some(l)
    });
    let Some(lines) = lines else { return };
    if !lines.is_empty() && game.native_corner_shown(Corner::TopLeft) {
        let w = lines.iter().map(|(t, _)| game.native_text_width(t)).fold(0.0f32, f32::max) + 8.0;
        let h = lines.len() as f32 * 10.0 + 6.0;
        let (x, y) = corners.place(Corner::TopLeft, w, h);
        game.native_fill(batch, [x, y, w, h], 0x9000_0000);
        for (i, (text, rgb)) in lines.iter().enumerate() {
            game.native_text(batch, text, [x + 4.0, y + 4.0 + i as f32 * 10.0], *rgb, true);
        }
    }
    if with(|s| s.settings.materials_hud) && game.native_corner_shown(Corner::TopRight) {
        let list = material_list(game);
        let rows: Vec<_> = list.iter().filter(|m| m.missing > 0).take(14).collect();
        let title = if rows.is_empty() { "Materials: nothing missing".to_string() } else { "Materials (missing / carried)".to_string() };
        let text = |m: &Material| format!("{}  {} / {}", short(&m.item), m.missing, m.available);
        let w = rows.iter().map(|m| game.native_text_width(&text(m)) + 16.0).fold(game.native_text_width(&title), f32::max) + 10.0;
        let h = 16.0 + rows.len() as f32 * 13.0;
        let (x, y) = corners.place(Corner::TopRight, w, h);
        game.native_fill(batch, [x, y, w, h], 0x9000_0000);
        game.native_text(batch, &title, [x + 4.0, y + 4.0], 0x55FFFF, true);
        for (i, m) in rows.iter().enumerate() {
            let ry = y + 15.0 + i as f32 * 13.0;
            game.native_item(batch, &m.item, [x + 4.0, ry], 12.0);
            let rgb = if m.to_fetch() == 0 { 0x55FF55 } else { 0xFF5555 };
            game.native_text(batch, &text(m), [x + 20.0, ry + 2.0], rgb, true);
        }
    }
}

// ---------------------------------------------------------------- the menu screen (M)

/// What the Load tab shows of a file (read when it is picked).
#[derive(Clone, Debug)]
struct FileInfo {
    name: String,
    format: String,
    size: [i32; 3],
    blocks: i64,
    regions: usize,
    author: String,
}

fn file_info(name: &str) -> Result<FileInfo, String> {
    let dir = schematics_dir()?;
    let bytes = std::fs::read(dir.join(name)).map_err(|e| e.to_string())?;
    let (s, format) = horizonmatica_core::formats::load(&bytes)?;
    let (lo, hi) = s.bounds();
    Ok(FileInfo {
        name: if s.metadata.name.is_empty() { name.to_string() } else { s.metadata.name.clone() },
        format: format.name(),
        size: [0, 1, 2].map(|i| hi[i] - lo[i] + 1),
        blocks: if s.metadata.total_blocks > 0 { s.metadata.total_blocks } else { s.block_count() as i64 },
        regions: s.regions.len(),
        author: s.metadata.author.clone(),
    })
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

/// `minecraft:oak_planks` as "Oak Planks".
fn pretty_item(item: &str) -> String {
    short(item)
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs a command for the screen; its first line (or the error) shows at the bottom.
fn act(game: &mut Game<'_>, args: &[&str]) {
    let msg = match run(game, args) {
        Ok(lines) => lines.into_iter().next().unwrap_or_default(),
        Err(e) => format!("Error: {e}"),
    };
    with(|s| s.ui_message = msg);
}

/// After a setting changed through the screen.
fn settings_changed(game: &Game<'_>) {
    with(|s| {
        s.settings_loaded = false;
        load_settings(game, s);
        s.mesh_key = None;
    });
}

/// The screen M opens, drawn with horizon-menus' widgets (the game's own buttons, sliders,
/// edit boxes and lists). The game keeps running behind it.
pub(super) fn screen(game: &mut Game<'_>, ui: &mut menus::Ui<'_>) {
    use winit::keyboard::KeyCode;
    with(|s| load_settings(game, s));
    let w = (ui.width - 20.0).min(460.0).floor();
    let x0 = ((ui.width - w) / 2.0).floor();
    let loaded = with(|s| {
        s.placement.as_ref().map(|p| if p.schematic.metadata.name.is_empty() { p.file_name.clone() } else { p.schematic.metadata.name.clone() })
    });
    ui.text_centered(game, ui.width / 2.0, 7.0, "Horizonmatica", 0xFFFFFF);
    if let Some(name) = &loaded {
        let shown = ui.fit(game, name, w / 2.0 - 50.0);
        ui.text_right(game, x0 + w, 7.0, &shown, 0x55FFFF);
    }
    const TABS: [&str; 6] = ["Load", "Placement", "Printer", "Display", "Materials", "Credits"];
    let tab = with(|s| s.ui_tab).min(TABS.len() - 1);
    let mut next = ui.tabs(game, [x0, 20.0, w, 20.0], &TABS, tab);
    // 1-6 switch tabs, M closes (while no edit box has the keys).
    for (i, key) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6]
        .into_iter()
        .enumerate()
    {
        if ui.key(key) {
            next = Some(i);
        }
    }
    if ui.key(KeyCode::KeyM) {
        ui.close();
    }
    if let Some(t) = next {
        with(|s| s.ui_tab = t);
    }
    let body = [x0, 46.0, w, ui.height - 46.0 - 30.0];
    match tab {
        0 => tab_load(game, ui, body),
        1 => tab_placement(game, ui, body),
        2 => tab_printer(game, ui, body),
        3 => tab_display(game, ui, body),
        4 => tab_materials(game, ui, body),
        _ => tab_credits(game, ui, body),
    }
    let fy = ui.height - 25.0;
    let msg = with(|s| s.ui_message.clone());
    if !msg.is_empty() {
        ui.text_fit(game, x0, fy + 6.0, w - 110.0, &msg, 0xFFFF55);
    }
    if ui.button(game, [x0 + w - 100.0, fy, 100.0, 20.0], "Done", true) {
        ui.close();
    }
}

fn tab_load(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    // The folder is listed again every few seconds while the tab is open.
    with(|s| {
        if s.ui_files_at.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(3)) {
            s.ui_files_at = Some(std::time::Instant::now());
            s.ui_files = schematics_dir()
                .map(|dir| {
                    horizonmatica_core::list_schematics(&dir)
                        .into_iter()
                        .map(|n| {
                            let size = std::fs::metadata(dir.join(&n)).map_or(0, |m| m.len());
                            (n, size)
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
    });
    let lw = (r[2] * 0.55).floor();
    let query = ui.edit(game, "search", [r[0] + 1.0, r[1] + 1.0, lw - 2.0, 18.0], "Search...", 64).to_lowercase();
    let files: Vec<(String, u64)> =
        with(|s| s.ui_files.iter().filter(|(n, _)| n.to_lowercase().contains(&query)).cloned().collect());
    let total = with(|s| s.ui_files.len());
    let list = [r[0], r[1] + 24.0, lw, r[3] - 24.0];
    ui.fill(game, list, 0x9000_0000);
    ui.frame(game, list, 0xFF50_5050);
    let row_h = 14.0;
    let content = files.len() as f32 * row_h + 4.0;
    let (y0, mark) = ui.scroll_begin("files", list, content);
    let selected = with(|s| s.ui_file.clone());
    let mut picked = None;
    for (i, (name, size)) in files.iter().enumerate() {
        let y = y0 + 2.0 + i as f32 * row_h;
        if y + row_h < list[1] || y > list[1] + list[3] {
            continue;
        }
        let row = [list[0] + 2.0, y, list[2] - 10.0, row_h];
        let is_selected = selected.as_deref() == Some(name.as_str());
        if is_selected {
            ui.fill(game, row, 0x50FF_FFFF);
        } else if ui.hovered(row) {
            ui.fill(game, row, 0x25FF_FFFF);
        }
        let size = human_size(*size);
        let sw = ui.text_width(game, &size);
        ui.text_fit(game, row[0] + 3.0, y + 3.0, row[2] - sw - 12.0, name, if is_selected { 0xFFFFFF } else { 0xD0D0D0 });
        ui.text_right(game, row[0] + row[2] - 3.0, y + 3.0, &size, 0x909090);
        if ui.clicked(row) {
            picked = Some(name.clone());
        }
    }
    ui.scroll_end(game, "files", list, content, mark);
    if files.is_empty() {
        let text = if total == 0 { "No schematic files yet" } else { "Nothing matches" };
        ui.text(game, list[0] + 5.0, list[1] + 5.0, text, 0x909090);
    }
    if let Some(name) = picked {
        // A second click on the picked file loads it.
        if selected.as_deref() == Some(name.as_str()) {
            act(game, &["load", &name]);
            with(|s| s.ui_tab = 1);
            return;
        }
        with(|s| s.ui_file = Some(name));
    }

    // The picked file: what it is, and loading it.
    let rx = r[0] + lw + 8.0;
    let rw = r[2] - lw - 8.0;
    let selected = with(|s| s.ui_file.clone()).filter(|n| with(|s| s.ui_files.iter().any(|(f, _)| f == n)));
    let mut y = r[1] + 2.0;
    match &selected {
        Some(name) => {
            let size = with(|s| s.ui_files.iter().find(|(f, _)| f == name).map_or(0, |(_, b)| *b));
            let key = format!("{name}|{size}");
            let info = with(|s| s.ui_info.get(&key).cloned()).unwrap_or_else(|| {
                let info = file_info(name);
                with(|s| s.ui_info.insert(key.clone(), info.clone()));
                info
            });
            match info {
                Ok(i) => {
                    y += ui.text_wrapped(game, rx, y, rw, &i.name, 0xFFFFFF) + 2.0;
                    let lines = [
                        (format!("{} x {} x {}", i.size[0], i.size[1], i.size[2]), 0xE0E0E0),
                        (format!("{} blocks", i.blocks), 0xE0E0E0),
                        (format!("{} region{}", i.regions, if i.regions == 1 { "" } else { "s" }), 0xA0A0A0),
                        (i.format.clone(), 0xA0A0A0),
                        (if i.author.is_empty() { String::new() } else { format!("by {}", i.author) }, 0xA0A0A0),
                    ];
                    for (text, rgb) in lines {
                        if !text.is_empty() {
                            ui.text_fit(game, rx, y, rw, &text, rgb);
                            y += 11.0;
                        }
                    }
                }
                Err(e) => {
                    ui.text_wrapped(game, rx, y, rw, &format!("Can't read it: {e}"), 0xFF5555);
                }
            }
            let by = r[1] + r[3] - 68.0;
            if ui.button_tip(game, [rx, by, rw, 20.0], "Load here", true, &["Places it at your feet (or click the file twice)"]) {
                act(game, &["load", name]);
                with(|s| s.ui_tab = 1);
            }
        }
        None => {
            ui.text_wrapped(game, rx, y, rw, "Pick a file on the left. Put .litematic, .schem, .schematic and .nbt files into the schematics folder.", 0xA0A0A0);
        }
    }
    let by = r[1] + r[3] - 44.0;
    let half = ((rw - 4.0) / 2.0).floor();
    if ui.button_tip(game, [rx, by, half, 20.0], "Reload last", true, &["Loads the last schematic again where it was"]) {
        act(game, &["reload"]);
    }
    if ui.button(game, [rx + half + 4.0, by, rw - half - 4.0, 20.0], "Open folder", true) {
        menu_action(game, "folder");
    }
    if ui.button(game, [rx, by + 24.0, rw, 20.0], "Refresh", true) {
        with(|s| {
            s.ui_files_at = None;
            s.ui_info.clear();
        });
    }
}

fn tab_placement(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let info = with(|s| {
        s.placement.as_ref().map(|p| {
            let (lo, hi) = p.bounds();
            (
                p.origin,
                [0, 1, 2].map(|i| hi[i] - lo[i] + 1),
                p.rotation.name(),
                p.mirror.name(),
                s.layer.describe(),
                s.moving,
                s.tally.as_ref().map(|t| (t.percent(), t.missing, t.wrong_block + t.wrong_state, t.extra)),
            )
        })
    });
    let Some((origin, size, rotation, mirror, layers, moving, tally)) = info else {
        ui.text(game, r[0], r[1] + 4.0, "No schematic loaded.", 0xA0A0A0);
        if ui.button(game, [r[0], r[1] + 18.0, 150.0, 20.0], "Load one", true) {
            with(|s| s.ui_tab = 0);
        }
        return;
    };
    let col = ((r[2] - 8.0) / 2.0).floor();
    let (lx, rx) = (r[0], r[0] + col + 8.0);
    let mut y = r[1];

    // Left: where it is.
    ui.text(game, lx, y + 2.0, &format!("Size {} x {} x {}", size[0], size[1], size[2]), 0xE0E0E0);
    if let Some((pct, missing, wrong, extra)) = tally {
        ui.text(game, lx, y + 13.0, &format!("{pct:.1}% done, {missing} missing", ), 0x55FF55);
        ui.text(game, lx, y + 24.0, &format!("{wrong} wrong, {extra} extra"), 0xFFAA00);
    } else {
        ui.text(game, lx, y + 13.0, "Checking the world...", 0xA0A0A0);
    }
    y += 38.0;
    ui.text(game, lx, y + 6.0, "Origin", 0xFFFFFF);
    // The fields follow the origin unless one is being typed in.
    let typing = ui.focused("ox") || ui.focused("oy") || ui.focused("oz");
    if !typing && with(|s| s.ui_origin != Some(origin)) {
        ui.set_edit("ox", &origin[0].to_string());
        ui.set_edit("oy", &origin[1].to_string());
        ui.set_edit("oz", &origin[2].to_string());
        with(|s| s.ui_origin = Some(origin));
    }
    let fx = lx + 36.0;
    let fw = ((col - 36.0 - 40.0 - 6.0) / 3.0).floor();
    let ox = ui.edit(game, "ox", [fx, y + 1.0, fw - 2.0, 18.0], "x", 9);
    let oy = ui.edit(game, "oy", [fx + fw, y + 1.0, fw - 2.0, 18.0], "y", 9);
    let oz = ui.edit(game, "oz", [fx + 2.0 * fw, y + 1.0, fw - 2.0, 18.0], "z", 9);
    let set = ui.button(game, [fx + 3.0 * fw + 2.0, y, 40.0, 20.0], "Set", true) || ui.key(winit::keyboard::KeyCode::Enter);
    if set {
        match (ox.trim().parse::<i32>(), oy.trim().parse::<i32>(), oz.trim().parse::<i32>()) {
            (Ok(a), Ok(b), Ok(c)) => act(game, &["origin", &a.to_string(), &b.to_string(), &c.to_string()]),
            _ => with(|s| s.ui_message = "Origin: three whole numbers".into()),
        }
        with(|s| s.ui_origin = None);
    }
    y += 24.0;
    // Nudge buttons: 1 block, 10 with Shift.
    let step = if ui.shift { 10 } else { 1 };
    let bw = ((col - 10.0) / 6.0).floor();
    let nudges: [(&str, [i32; 3]); 6] =
        [("-X", [-1, 0, 0]), ("+X", [1, 0, 0]), ("-Y", [0, -1, 0]), ("+Y", [0, 1, 0]), ("-Z", [0, 0, -1]), ("+Z", [0, 0, 1])];
    for (i, (label, d)) in nudges.into_iter().enumerate() {
        let tip = format!("Move {step} block{} ({label}); Shift: 10", if step == 1 { "" } else { "s" });
        if ui.button_tip(game, [lx + i as f32 * (bw + 2.0), y, bw, 20.0], label, true, &[&tip]) {
            let d = d.map(|v| (v * step).to_string());
            act(game, &["move", &d[0], &d[1], &d[2]]);
        }
    }
    y += 24.0;
    if ui.button(game, [lx, y, col, 20.0], "Move to my position", true) {
        act(game, &["origin", "here"]);
    }
    y += 24.0;
    let label = format!("Move mode: {}", if moving { "ON" } else { "OFF" });
    if ui.button_tip(game, [lx, y, col, 20.0], &label, true, &["Wheel, arrows and PgUp/PgDn move it in the world (N)"]) {
        toggle_move_mode(game);
        if with(|s| s.moving) {
            ui.close();
        }
    }

    // Right: how it is turned, layers, unload.
    let mut y = r[1];
    let half = ((col - 4.0) / 2.0).floor();
    ui.text(game, rx, y + 2.0, &format!("Rotation: {rotation}"), 0xE0E0E0);
    y += 13.0;
    if ui.button(game, [rx, y, half, 20.0], "Rotate left", true) {
        act(game, &["rotate", "ccw"]);
    }
    if ui.button(game, [rx + half + 4.0, y, col - half - 4.0, 20.0], "Rotate right", true) {
        act(game, &["rotate", "cw"]);
    }
    y += 24.0;
    if ui.button_tip(game, [rx, y, col, 20.0], &format!("Mirror: {mirror}"), true, &["none, x (front-back), z (left-right)"]) {
        menu_action(game, "mirror_next");
    }
    y += 24.0;
    if ui.button(game, [rx, y, col, 20.0], &format!("Layers: {layers}"), true) {
        let next = next_layer_mode();
        act(game, &["layer", next]);
    }
    y += 24.0;
    if ui.button(game, [rx, y, half, 20.0], "Layer down", true) {
        act(game, &["layer", "down"]);
    }
    if ui.button(game, [rx + half + 4.0, y, col - half - 4.0, 20.0], "Layer up", true) {
        act(game, &["layer", "up"]);
    }
    y += 24.0;
    if ui.button(game, [rx, y, col, 20.0], "Show all layers", true) {
        act(game, &["layer", "all"]);
    }
    let uy = (r[1] + r[3] - 20.0).max(y + 28.0);
    if ui.button_tip(game, [rx, uy, col, 20.0], "Unload", true, &["Removes the schematic (the world stays as it is)"]) {
        act(game, &["unload"]);
    }
}

fn tab_printer(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let (on, placed, note, loaded) =
        with(|s| (s.settings.printer_on, s.placed, s.printer_note.clone(), s.placement.is_some()));
    let label = if on { "Printer: ON (building)" } else { "Printer: OFF" };
    if ui.button_tip(game, [r[0], r[1], r[2], 20.0], label, loaded, &["Places missing blocks within reach (K)"]) {
        act(game, &["printer"]);
    }
    let mut y = r[1] + 26.0;
    if !loaded {
        ui.text(game, r[0], y, "Load a schematic first.", 0xA0A0A0);
        y += 12.0;
    }
    ui.text(game, r[0], y, &format!("Placed this session: {placed}"), 0xE0E0E0);
    y += 11.0;
    if !note.is_empty() {
        ui.text_fit(game, r[0], y, r[2], &note, 0xA0A0A0);
    }
    y += 16.0;
    let event = {
        let mut store = menus::GameStore { game: &*game, id: ID };
        ui.settings(game, menu(), "printer", &mut store, [r[0], y, r[2], 0.0], 2).0
    };
    if event != menus::Event::None {
        settings_changed(game);
    }
    ui.text_wrapped(
        game,
        r[0],
        r[1] + r[3] - 20.0,
        r[2],
        "Survival: the blocks must be in your hotbar. On servers, automatic placing may break rules or trip anti-cheat.",
        0x808080,
    );
}

fn tab_display(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let mut events = Vec::new();
    let mut y = r[1];
    for (title, page) in [("World overlay", "display"), ("HUD", "hud")] {
        ui.text(game, r[0], y + 2.0, title, 0xFFFFFF);
        y += 13.0;
        let (event, used) = {
            let mut store = menus::GameStore { game: &*game, id: ID };
            ui.settings(game, menu(), page, &mut store, [r[0], y, r[2], 0.0], 2)
        };
        events.push(event);
        y += used + 4.0;
    }
    for event in events {
        match event {
            menus::Event::None => {}
            menus::Event::Changed(_) => settings_changed(game),
            menus::Event::Action(a) => menu_action(game, &a),
        }
    }
}

fn tab_materials(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let list = material_list(game);
    let hud = with(|s| s.settings.materials_hud);
    let bw = 150.0;
    if ui.button_tip(game, [r[0] + r[2] - bw, r[1], bw, 20.0], &format!("HUD list: {}", if hud { "ON" } else { "OFF" }), true, &["The list in the corner while playing (J)"]) {
        with(|s| {
            s.settings.materials_hud = !s.settings.materials_hud;
            save_settings(game, &s.settings);
        });
    }
    let fetch: u64 = list.iter().map(|m| m.to_fetch()).sum();
    let summary = if list.is_empty() {
        "No schematic loaded, or it needs no items.".to_string()
    } else {
        format!("{} kinds, {} items still to fetch", list.len(), fetch)
    };
    ui.text(game, r[0], r[1] + 6.0, &summary, 0xE0E0E0);
    let table = [r[0], r[1] + 26.0, r[2], r[3] - 26.0];
    let cols = [table[0] + table[2] - 150.0, table[0] + table[2] - 90.0, table[0] + table[2] - 12.0];
    ui.text(game, table[0] + 24.0, table[1], "Item", 0xFFFFFF);
    ui.text_right(game, cols[0], table[1], "Missing", 0xFFFFFF);
    ui.text_right(game, cols[1], table[1], "Total", 0xFFFFFF);
    ui.text_right(game, cols[2], table[1], "Carried", 0xFFFFFF);
    let area = [table[0], table[1] + 11.0, table[2], table[3] - 11.0];
    ui.fill(game, area, 0x9000_0000);
    let row_h = 18.0;
    let content = list.len() as f32 * row_h + 2.0;
    let (y0, mark) = ui.scroll_begin("materials", area, content);
    for (i, m) in list.iter().enumerate() {
        let y = y0 + 1.0 + i as f32 * row_h;
        if y + row_h < area[1] || y > area[1] + area[3] {
            continue;
        }
        let row = [area[0] + 1.0, y, area[2] - 8.0, row_h];
        if i % 2 == 1 {
            ui.fill(game, row, 0x18FF_FFFF);
        }
        let name = pretty_item(&m.item);
        if ui.hovered(row) {
            ui.fill(game, row, 0x25FF_FFFF);
            ui.tooltip(&[&name, &format!("{} still to fetch", m.to_fetch()), &m.item]);
        }
        ui.item(game, row[0] + 2.0, y + 1.0, &m.item, 16.0);
        ui.text_fit(game, row[0] + 23.0, y + 5.0, cols[0] - row[0] - 80.0, &name, 0xE0E0E0);
        let done = m.to_fetch() == 0;
        ui.text_right(game, cols[0], y + 5.0, &m.missing.to_string(), if m.missing == 0 { 0x55FF55 } else { 0xFF7070 });
        ui.text_right(game, cols[1], y + 5.0, &m.total.to_string(), 0xA0A0A0);
        ui.text_right(game, cols[2], y + 5.0, &m.available.to_string(), if done { 0x55FF55 } else { 0xFFAA00 });
    }
    ui.scroll_end(game, "materials", area, content, mark);
}

fn tab_credits(game: &mut Game<'_>, ui: &mut menus::Ui<'_>, r: menus::Rect) {
    let cx = r[0] + r[2] / 2.0;
    let mut y = r[1] + 4.0;
    ui.text_centered(game, cx, y, "Horizonmatica re-creates Litematica for Horizon.", 0xFFFFFF);
    y += 12.0;
    ui.text_centered(game, cx, y, "All credit for the idea and design goes to:", 0xA0A0A0);
    y += 20.0;
    let people: [(&str, &str, &str); 4] = [
        ("Litematica", "by masa (maruohon), maintained by Sakura-Ryoko", "github.com/maruohon/litematica"),
        ("Litematica Printer", "by aleksilassila, maintained by Sakura-Ryoko", "github.com/aleksilassila/litematica-printer"),
        ("Horizon", "by the horizon-rust team", "github.com/horizon-rust/horizon"),
        ("File formats", "Litematica, Sponge/WorldEdit, MCEdit, Mojang", ".litematic .schem .schematic .nbt"),
    ];
    for (what, who, link) in people {
        ui.text_centered(game, cx, y, what, 0x55FFFF);
        y += 11.0;
        ui.text_centered(game, cx, y, who, 0xE0E0E0);
        y += 11.0;
        ui.text_centered(game, cx, y, link, 0x808080);
        y += 17.0;
    }
    ui.text_centered(game, cx, y, "Not made or endorsed by them. No code copied: format and behaviour re-created.", 0xA0A0A0);
    if ui.button(game, [cx - 100.0, r[1] + r[3] - 20.0, 200.0, 20.0], "Show the credits in chat", true) {
        menu_action(game, "credits_chat");
    }
}
