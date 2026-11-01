# Horizonmatica

A native mod for [Horizon](https://github.com/horizon-rust/horizon) (Minecraft Java Edition 26.3, rewritten in Rust):
show schematics as ghost blocks, check your build against them, get a material list, and let a **printer**
(auto-build) place the missing blocks. Horizonmatica is a re-creation of **Litematica** for Horizon.

> ## ❤️ Credits
>
> Horizonmatica wouldn't exist without these projects. The idea, the way it is used, the file format and the
> behaviour all come from them:
>
> - **[Litematica](https://github.com/maruohon/litematica)** by **masa** ([maruohon](https://github.com/maruohon)),
>   maintained for newer Minecraft versions by **[Sakura-Ryoko](https://github.com/sakura-ryoko/litematica)**.
>   The `.litematic` format, the overlay colours, the layer modes and the verifier follow Litematica.
> - **[Litematica Printer](https://github.com/aleksilassila/litematica-printer)** by **aleksilassila**
>   (Aleksi Lassila), maintained by Sakura-Ryoko. The printer follows its approach: the look is sent to the server
>   only with the click, the camera never moves.
> - **[Horizon](https://github.com/horizon-rust/horizon)** by the horizon-rust team: the game, the mod hooks, the
>   placement logic and the data fixer Horizonmatica uses. The table that maps old `.schematic` block ids
>   (`core/src/legacy_blocks.rs`) comes from Horizon's `block_state_data.txt`, which is generated from vanilla's
>   `BlockStateData` (Horizon is MIT licensed).
> - File formats: [Sponge Schematic](https://github.com/SpongePowered/Schematic-Specification) (`.schem`, WorldEdit),
>   MCEdit (`.schematic`), Mojang's structure files (`.nbt`).
>
> Horizonmatica is **not an official project** of these authors and is neither made nor endorsed by them. It contains
> no code from Litematica or the printer (licensed LGPL-3.0 and AGPL-3.0); only the file format and the behaviour
> are re-created. In game: `/horizonmatica credits` and the mod's menu.
>
> If you like Horizonmatica, go and support the originals.

| Feature | Status |
|---|---|
| Load `.litematic` (versions 1–7, several regions, negative sizes) | ✅ |
| Load `.schem` (Sponge v1–v3: WorldEdit, FAWE, Amulet, Axiom) | ✅ |
| Load `.nbt` (vanilla structure blocks, data packs) | ✅ |
| Load `.schematic` (MCEdit / WorldEdit up to 1.12, numeric block ids) | ✅ |
| Upgrade older files to the current version (Horizon's data fixer) | ✅ |
| Place, move, rotate (90/180/270), mirror (x/z) | ✅ |
| Remembered per world and dimension, restored when you join again | ✅ |
| Ghost overlay: missing blocks as translucent real block models (chests and other block entities included) | ✅ |
| Overlay built per 16³ section: only what changed is rebuilt, within a time budget per tick; faces between ghost blocks are left out | ✅ |
| Verifier overlay: wrong block (red), wrong state (orange), extra block (pink) | ✅ |
| Layer modes: all / one layer / above / below / range | ✅ |
| Info HUD (progress in %, missing, wrong) | ✅ |
| Material list (HUD with item icons, chat, menu), missing vs. carried | ✅ |
| Own menu screen (M) drawn with the game's widgets | ✅ |
| **Printer**: places missing blocks within reach automatically, facing the right way | ✅ |
| Saving your own schematics, area selection, Easy Place protocol, entities | ❌ (not yet) |

## Installation

Native mods are compiled into the game by the Horizon launcher. They run unsandboxed, so only you can approve
building them.

1. Install the **Horizon launcher** and start the game once.
2. Build the mod into your instance, either from this folder:

   ```
   horizon-launcher native-build <path to this repository>/mixins --trust-native-code
   ```

   or in the launcher: **Mods** → install a native mod from a folder (or a `.hnmod` file) → **Build**.
   The first build downloads Rust and takes 20–30 minutes; later builds are faster.
3. To get a package: `python tools/package.py` writes `dist/horizonmatica-0.1.0.hnmod`.

**Version:** the mod uses game internals (block models, placement prediction, the data fixer, the Debug Options
screen for its menu), so it is pinned to one Horizon commit: `horizon_revision` in
`mixins/horizonmatica/horizon-mixin.toml`, currently **Nightly 2026-11-01 (`4aa6854`, hook API 80)**, which it is
checked against. It needs the render hooks `world_drawn`/`render_needs` (API 58+). For another Nightly, set its
commit (`python tools/package.py --horizon <commit>`) and build again.

### Verifying downloads

Every release `.hnmod` is signed with [minisign](https://jedisct1.github.io/minisign/); the signature is the
`.minisig` asset next to it. The public key is [`minisign.pub`](minisign.pub):

```
RWS+BRoCJslNCFapck3cbG2VFQcjlr1MUH5m3+CWA8zhJecoBoja7X94
```

Check a download with:

```
minisign -Vm horizonmatica-0.1.0.hnmod -P RWS+BRoCJslNCFapck3cbG2VFQcjlr1MUH5m3+CWA8zhJecoBoja7X94
```

## Usage

Schematics go into the `schematics/` folder next to `options.txt` of your Horizon instance
(`/horizonmatica folder` shows the path). `.litematic`, `.schem`, `.schematic` and `.nbt` files are read; the mod
tells the format by the contents. When two files share a name, loading without an extension picks the `.litematic`.

- **Older files** (from 1.16 or 1.12, say) go through Horizon's data fixer when loaded, like old worlds do:
  `grass` becomes `short_grass`, `grass_path` becomes `dirt_path`, `sign` becomes `oak_sign`, and so on.
- **`.nbt`:** cells the structure file doesn't list count as structure void and are neither checked nor built.
- **`.schematic` (old):** numeric ids are mapped with vanilla's flattening table. What the old format kept elsewhere
  is worked out again: door halves, tall plants, skull and bed colours, banner colours, double chests, snowy grass.
  Flower pot contents are lost (an empty pot).
- Fence, pane, wall and redstone connections and stair shapes follow from the neighbours while building, so they
  don't count as wrong unless **strict states** are on.

| Command | Effect |
|---|---|
| `/horizonmatica list` | List the schematics in the folder |
| `/horizonmatica load <name>` | Load one and place it at your position (the name without extension, or its start, is enough) |
| `/horizonmatica reload` | Load the last schematic again where it was |
| `/horizonmatica unload` | Unload it (and forget it for this world) |
| `/horizonmatica origin here` · `origin <x> <y> <z>` · `move <dx> <dy> <dz>` | Move it |
| `/horizonmatica rotate cw\|ccw\|180\|none` · `mirror none\|x\|z` | Rotate / mirror it |
| `/horizonmatica layer all\|<y>\|here\|up\|down\|above <y>\|below <y>\|range <a> <b>` | Layers |
| `/horizonmatica render on\|off` · `through on\|off` · `mode ghost\|boxes` | Display (`through`: see through blocks) |
| `/horizonmatica info` | Position, progress, errors |
| `/horizonmatica materials` | Material list to chat, and the HUD list on or off |
| `/horizonmatica printer on\|off` · `air on\|off` · `speed <ticks>` · `reach <blocks>` | Printer |
| `/horizonmatica credits` | Who made the originals |

`/hm` is the short form.

**Remembered per world:** for every world (single player: its folder; servers: their address) and every dimension,
Horizonmatica remembers which schematic is placed where, rotated and mirrored how. When you join the world again it
is loaded at the same spot (with a line in chat). Switching to another world or dimension unloads it and loads the
one remembered there. `/hm unload` forgets it for the world.

Keys (rebind them in **Options › Mods › Horizonmatica**):

| Key | Action |
|---|---|
| M | **Open the menu** (as in Litematica) |
| V | Rendering on/off |
| Page Up / Page Down | Layer up / down |
| Home | Next layer mode (all → this layer → above → below) |
| K | Printer on/off |
| J | Material list in the HUD on/off |
| N | Move mode on/off |

**Move mode (N):** only in this mode do the mouse and keys move the schematic; while it is off, only commands and
the menu do, so nothing gets moved by accident. In move mode:

| Input | Effect |
|---|---|
| Mouse wheel | Along your view (the closest axis, up and down too) |
| Arrow keys | Forward / back / left / right relative to your view |
| Page Up / Page Down | Up / down (instead of changing layers) |
| R | Rotate clockwise |
| Home | Move it to your position |
| Shift held | 5 blocks instead of 1 |
| N or Enter | Done, the schematic is locked again |

The printer pauses while move mode is on.

### The menu (M)

**M** opens Horizonmatica's own screen, drawn with the game's widgets (buttons, sliders, edit boxes, scroll lists,
tooltips, item icons) by [horizon-menus](libs/horizon-menus/README.md). The game keeps running behind it. **Esc**,
**M** or **Done** close it, **1–6** switch tabs.

| Tab | Contents |
|---|---|
| Load | Search box, scrolling file list with sizes, details of the picked file (size, blocks, format, author), load (or click the file twice), reload the last one, open the folder |
| Placement | Progress, the origin as edit boxes, nudge buttons ±X/±Y/±Z (Shift: 10 blocks), move to my position, rotate, mirror, layers, move mode, unload |
| Printer | On/off, blocks placed, speed and reach as sliders, air place |
| Display | Overlay and HUD settings (sliders for opacity and radius) |
| Materials | Material list with item icons: missing, total, carried; the HUD list on/off |
| Credits | The originals and their authors |

When the screen can't open (over another screen, say), M opens the mod's page in **Options › Mods** instead, with the
same settings as buttons (Shift + click steps backwards).

Under the hood the screen borrows the place of the Debug Options screen (F3+F6): its draw and input functions are
hooked and do nothing of their own while the game hasn't opened that screen itself, and the game doesn't pause for it.

### Printer (auto-build)

While the printer is on, it places one missing block within reach every few ticks, from the bottom up and only in
the layers shown. For each block it tries click points on the neighbouring faces and asks Horizon's own placement
logic (`placement_state`, vanilla's `getStateForPlacement`) what that click would place. It only clicks when exactly
the wanted state comes out (stairs the right way round, top/bottom slabs, furnaces, logs...).

- **Survival:** the item has to be in the hotbar. If it is only in the inventory, the HUD says so.
- **Creative:** only the hotbar is used: a free slot, else one whose item the schematic doesn't need any more, else
  the one used longest ago. The rest of the inventory stays as it is.
- **Redstone order:** plain blocks first, then what carries or uses a signal (dust, repeaters, comparators, pistons,
  droppers, lamps, doors...), then what makes one (torches, levers, buttons, redstone blocks...), observers last and
  only once the block they watch is there, so they don't fire while you build.
- **Set up after placing:** repeater delay, comparator mode, daylight detector inversion and note block pitch are
  clicked to the right value after placing.
- **On/off states** (powered, lit, power...) don't stop it: the block is placed, and if its state is wrong the
  overlay shows it as a wrong state (can be turned off: Display › "Redstone on/off counts as wrong").
- **Falling blocks** (sand, gravel, concrete powder, anvils, the dragon egg) are only placed onto something: if the
  block below is still missing it waits; if they float in the schematic it leaves them out (the HUD says why).
- **Item changes:** through a server (also the integrated server of a single-player world) it clicks only in the
  tick after a slot or item change, so the server doesn't place the item held before.
- **Double slabs and stacks:** where half a slab is already there, it clicks it into a double slab; likewise more
  candles, sea pickles, snow layers, turtle eggs, petals.
- **Chests:** a double chest forms when its second half is placed; a single chest next to one facing the same way is
  placed sneaking so that it stays single (in your own world). Ghost chests (and shulker boxes, banners, heads,
  bells) show their real model.
- The camera never moves (as with litematica-printer). In your own world the click is used within the same tick; on
  servers only the server is told the right look for the click.
- **Air place** (`/hm printer air on|off`, on by default): when no neighbour can be clicked, it clicks the empty spot
  itself, which vanilla servers allow. It checks first that the result faces the right way. Anti-cheat plugins
  (Grim, say) notice this.
- Fluids are not placed. Blocks that can't be placed right from any reachable side are skipped; the HUD says why.
- ⚠️ On other people's servers, automatic placing may break the rules or trip anti-cheat.

## Layout

```
mixins/horizonmatica/          the bundle Horizon builds in
  horizon-mixin.toml           hooks (tick, frame, hud, key, wheel, command(s), mods, settings, world_drawn,
                               render_needs) and the menu screen's injections
  *.rs                         thin helpers, one per hook
  runtime.rs                   game side: commands, HUD, verifier scan, overlay geometry, printer, menu screen
  bridge.rs                    render side (in horizon-client-render): the overlay's own GPU pass
  core/                        a crate of its own without the game, with unit tests: NBT, file formats, bit
                               arrays, rotation/mirroring, verifier, material list, printer planning
  menus/, menus_*.rs           copy of horizon-menus with its screen hooks (don't edit: vendor_menus.py)
libs/horizon-menus/            the menu library (git submodule: github.com/keksi2k/horizon-menus)
tools/package.py               packs the .hnmod (and checks the horizon-menus copy is current);
                               --sign also signs it with minisign (asks for the key's password)
minisign.pub                   the public key release files are signed with
```

Clone with `git clone --recursive`, or run `git submodule update --init` afterwards. After updating the library:
`python libs/horizon-menus/tools/vendor_menus.py mixins/horizonmatica horizonmatica`.

Tests: `cargo test -p horizonmatica-core -p horizon-menus`. A test schematic:
`cargo run -p horizonmatica-core --example sample -- sample_hut.litematic`.

License: MIT. No code taken from Litematica; the file format and the behaviour are re-created.
