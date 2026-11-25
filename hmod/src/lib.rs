//! Horizonmatica: the mod's crate. The launcher compiles it against the pinned game and calls its
//! `Hooks`. The game-independent logic lives in `src/core/` (crate `horizonmatica_core`); the game
//! is read through `mod_api` (`world`).

#[macro_use]
mod i18n;
mod completion;
pub mod fix;
pub mod hands;
pub mod memory;
mod menu;
pub mod overlay;
mod runtime;
mod settings;
mod store;
mod ui;
mod world;

use mod_api::{
    ClientHost, ClientTickPostEvent, CommandEvent, Flow, GuiRenderPostEvent, Hooks, InputKeyEvent, KeyAction, KeyRegistration,
    MouseButtonPreEvent, MouseScrollingEvent, RegisterClientCommandsEvent, Registrar, RegistrationContext, ScreenCharTypedPreEvent,
    ScreenClosingEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseButtonReleasedPreEvent,
    ScreenMouseDraggedPreEvent, ScreenMouseScrolledPreEvent, ScreenRenderPostEvent, SettingChangedEvent, Text,
};
use world::World;

pub const ID: &str = "horizonmatica";
pub const VERSION: &str = "0.5.4";
/// The data version of the game the mod is pinned to (26.3), written into saved files.
pub const DATA_VERSION: i32 = 5023;

/// Who Horizonmatica owes its design to (`/horizonmatica credits`).
pub fn credits() -> Vec<String> {
    (1..=6).map(|i| tr!(&format!("credits.{i}"))).collect()
}

#[derive(Default)]
struct Mod;

/// Sends the chat lines the mod queued.
fn flush(host: &mut dyn ClientHost) {
    for line in runtime::take_out() {
        let _ = host.chat_line(Text::literal(line));
    }
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        settings::register(r);
        completion::register(r);
        for (id, _, key) in runtime::BINDS {
            let mut k = KeyRegistration::new(id, key);
            k.label = format!("{ID}.key.{id}");
            r.register_key_mapping(k);
        }
    }

    fn register_client_commands(&self, _host: &mut dyn ClientHost, event: &mut RegisterClientCommandsEvent<'_>) {
        for name in ["horizonmatica", "hm"] {
            event.commands.push(mod_api::Command::new(name));
            for (sub, words, rest) in runtime::WORDS {
                event.commands.push(mod_api::Command::new(&format!("{name} {sub}")));
                for word in *words {
                    event.commands.push(mod_api::Command::new(&format!("{name} {sub} {word}")));
                }
                if rest.is_some() {
                    event.commands.push(mod_api::Command::new(&format!("{name} {sub} {}", completion::argument_of(sub))));
                }
            }
        }
    }

    fn command(&self, host: &mut dyn ClientHost, event: &mut CommandEvent<'_>) -> Flow {
        let ours = runtime::command(&World::new(&*host), event.command);
        flush(host);
        if ours { Flow::Cancel } else { Flow::Continue }
    }

    fn client_tick_post(&self, host: &mut dyn ClientHost, _event: &mut ClientTickPostEvent) {
        i18n::refresh(&*host);
        let pressed: Vec<&str> = if host.playing() {
            runtime::BINDS.iter().map(|(id, ..)| *id).filter(|id| host.consume_click(ID, id)).collect()
        } else {
            Vec::new()
        };
        {
            let w = World::new(&*host);
            runtime::tick(&w);
            for id in pressed {
                runtime::key_clicked(&w, id);
            }
            if w.in_world() {
                completion_step(&w);
            }
        }
        // The menu opens and closes here: that takes the game's mutable host.
        if menu::take_close() {
            host.mod_screen_close(ID);
        }
        if menu::take_open() && !host.mod_screen_open(ID) {
            runtime::say(&tr!("chat.menu_blocked"));
        }
        flush(host);
    }

    fn setting_changed(&self, _host: &mut dyn ClientHost, _event: &mut SettingChangedEvent<'_>) {
        runtime::settings_changed();
    }

    fn input_key(&self, host: &mut dyn ClientHost, event: &mut InputKeyEvent<'_>) {
        if event.action != KeyAction::Press || !host.playing() {
            return;
        }
        runtime::raw_key(&World::new(&*host), event.key, event.modifiers);
        flush(host);
    }

    fn mouse_scrolling(&self, host: &mut dyn ClientHost, event: &mut MouseScrollingEvent) -> Flow {
        if !host.playing() {
            return Flow::Continue;
        }
        let taken = runtime::wheel(&World::new(&*host), event.scroll[1] as f32, event.modifiers);
        flush(host);
        if taken { Flow::Cancel } else { Flow::Continue }
    }

    fn mouse_button_pre(&self, host: &mut dyn ClientHost, event: &mut MouseButtonPreEvent) -> Flow {
        if event.pressed && !host.playing() {
            return Flow::Continue;
        }
        let taken = runtime::mouse(&World::new(&*host), event.button, event.pressed, event.modifiers);
        flush(host);
        if taken { Flow::Cancel } else { Flow::Continue }
    }

    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        if !menu_open(host) {
            return;
        }
        i18n::refresh(host);
        let mouse = event.screen.mouse;
        let w = World::new(host);
        menu::draw(&w, ui::Ui::begin(&mut *event.draw, host, mouse));
    }

    fn screen_mouse_button_pressed_pre(&self, host: &dyn ClientHost, event: &mut ScreenMouseButtonPressedPreEvent<'_>) -> Flow {
        if !menu_open(host) {
            return Flow::Continue;
        }
        ui::on_press(host, event.screen.mouse, event.button, event.modifiers);
        Flow::Cancel
    }

    fn screen_mouse_button_released_pre(&self, host: &dyn ClientHost, event: &mut ScreenMouseButtonReleasedPreEvent<'_>) -> Flow {
        if !menu_open(host) {
            return Flow::Continue;
        }
        ui::on_release(event.button);
        Flow::Cancel
    }

    fn screen_mouse_dragged_pre(&self, host: &dyn ClientHost, event: &mut ScreenMouseDraggedPreEvent<'_>) -> Flow {
        if !menu_open(host) {
            return Flow::Continue;
        }
        ui::on_drag(host, event.screen.mouse, event.button);
        Flow::Cancel
    }

    fn screen_mouse_scrolled_pre(&self, host: &dyn ClientHost, event: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        if !menu_open(host) {
            return Flow::Continue;
        }
        ui::on_scroll(event.scroll[1]);
        Flow::Cancel
    }

    fn screen_key_pressed_pre(&self, host: &dyn ClientHost, event: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        // Escape is the game's: it closes the screen.
        if !menu_open(host) || event.key == "key.keyboard.escape" {
            return Flow::Continue;
        }
        ui::on_key(host, event.key, event.modifiers);
        Flow::Cancel
    }

    fn screen_char_typed_pre(&self, host: &dyn ClientHost, event: &mut ScreenCharTypedPreEvent<'_>) -> Flow {
        if !menu_open(host) {
            return Flow::Continue;
        }
        ui::on_char(host, event.c);
        Flow::Cancel
    }

    fn screen_closing(&self, host: &dyn ClientHost, _event: &mut ScreenClosingEvent<'_>) {
        if menu_open(host) {
            ui::on_close(host);
        }
    }

    fn gui_render_post(&self, host: &dyn ClientHost, event: &mut GuiRenderPostEvent<'_>) {
        // Hidden behind the menu (it would draw over it).
        if event.gui_hidden || menu_open(host) {
            return;
        }
        let w = World::new(host);
        if !w.in_world() {
            return;
        }
        runtime::ensure_settings(&w);
        hud(&w, event);
    }
}

/// What the chat's completions offer: the schematics folder (once) and the placements.
fn completion_step(w: &World<'_>) {
    static FOLDER_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !FOLDER_SET.load(std::sync::atomic::Ordering::Relaxed)
        && let Ok(dir) = store::schematics_dir(w)
    {
        completion::set_folder(dir);
        FOLDER_SET.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    completion::set_placements(runtime::with(|s| s.slots.iter().map(|x| runtime::display_name(&x.placement)).collect()));
}

/// The mod's own screen (the menu) is open.
fn menu_open(host: &dyn ClientHost) -> bool {
    host.mod_screen() == Some(ID)
}

/// The info box (top left) and the material list (top right).
fn hud(w: &World<'_>, event: &mut GuiRenderPostEvent<'_>) {
    let batch = &mut *event.batch;
    // The material list first: the info box keeps clear of it (long lines are cut short).
    let mut room = batch.size()[0] - 4.0;
    if runtime::with(|s| s.settings.materials_hud) {
        let list = runtime::material_list(w);
        let rows: Vec<_> = list.iter().filter(|m| m.missing > 0).take(14).collect();
        let title = if rows.is_empty() { tr!("hud.materials_none") } else { tr!("hud.materials") };
        let title = title.as_str();
        let text = |m: &horizonmatica_core::materials::Material| format!("{}  {} / {}", i18n::item_name(&m.item), m.missing, m.available);
        let width = rows.iter().map(|m| batch.text_width(&text(m)) + 16.0).fold(batch.text_width(title), f32::max) + 10.0;
        let height = 16.0 + rows.len() as f32 * 13.0;
        let x = batch.size()[0] - width - 2.0;
        let y = 2.0;
        room = x - 6.0;
        batch.fill([x, y, width, height], 0x9000_0000);
        batch.text(title, [x + 4.0, y + 4.0], 0xFF55_FFFF, true);
        for (i, m) in rows.iter().enumerate() {
            let ry = y + 15.0 + i as f32 * 13.0;
            if let Some(key) = mod_api::Identifier::parse(&m.item).ok().and_then(|id| w.host.item_key(&id)) {
                batch.item(key, [x + 4.0, ry], 12.0);
            }
            let rgb = if m.to_fetch() == 0 { 0xFF55_FF55 } else { 0xFFFF_5555 };
            batch.text(&text(m), [x + 20.0, ry + 2.0], rgb, true);
        }
    }
    let lines = runtime::hud_lines(w);
    if !lines.is_empty() {
        let max = (room - 8.0).max(60.0);
        let lines: Vec<(String, u32)> = lines
            .into_iter()
            .map(|(t, rgb)| {
                if batch.text_width(&t) <= max {
                    return (t, rgb);
                }
                let mut cut: String = t;
                while !cut.is_empty() && batch.text_width(&format!("{cut}...")) > max {
                    cut.pop();
                }
                (format!("{cut}..."), rgb)
            })
            .collect();
        let width = lines.iter().map(|(t, _)| batch.text_width(t)).fold(0.0f32, f32::max) + 8.0;
        let height = lines.len() as f32 * 10.0 + 6.0;
        let (x, y) = (2.0, 2.0);
        batch.fill([x, y, width, height], 0x9000_0000);
        for (i, (text, rgb)) in lines.iter().enumerate() {
            batch.text(text, [x + 4.0, y + 4.0 + i as f32 * 10.0], 0xFF00_0000 | rgb, true);
        }
    }
}