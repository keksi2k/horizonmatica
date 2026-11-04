# Placement tab

[Back to the overview](Home.md)

With nothing loaded, this tab only offers **Load one**, which opens the [Load tab](Load.md).

## Left side: all placements

| Control | What it does | Command |
|---|---|---|
| List row | Selects that placement. A row shows the number, name and origin, and whether it is hidden or locked | `/hm select <n>` or `<name>`, `next`, `prev`; `/hm placements` lists them in chat |
| **Load more** | Opens the [Load tab](Load.md) to add another placement | — |
| **Unload all** | Removes every placement and forgets them for this world and dimension | `/hm unload all` |

## Right side: the selected placement

The top lines show its size, rotation, mirror and progress: percent done, and how many blocks are missing, wrong or
extra. `/hm info` prints the same in chat.

### Show, lock, unload

| Button | What it does | Command |
|---|---|---|
| **Hide** / **Show** | A hidden placement stays loaded and remembered, but isn't drawn or printed | `/hm show [on\|off]`, `/hm hide` |
| **Lock** / **Unlock** | A locked placement can't be moved, turned or mirrored, by any means. The lock is remembered | `/hm lock [on\|off]`, `/hm unlock` |
| **Unload** | Removes this placement. The blocks in the world stay as they are | `/hm unload [<n>]` |

### Position

The buttons in this section and the next are greyed out while the placement is locked.

| Control | What it does | Command |
|---|---|---|
| **Origin** x, y, z and **Set** | Moves the origin to the typed position. Enter works too while typing | `/hm origin <x> <y> <z>` |
| **-X +X -Y +Y -Z +Z** | Moves it by 1 block, or by 10 with Shift held | `/hm move <dx> <dy> <dz>` |
| **Move to me** | Moves the origin to where you stand | `/hm origin here` |
| **Move mode: ON/OFF** | Closes the menu and lets the wheel and keys move the placement in the world. See [Keys](Keys.md#move-mode) | Key **N** |

### Rotation and mirror

| Button | What it does | Command |
|---|---|---|
| **Rotate left** | 90° counter-clockwise | `/hm rotate ccw` |
| **Rotate right** | 90° clockwise | `/hm rotate cw` |
| **Mirror: ...** | Steps through none, x (front-back) and z (left-right) | `/hm mirror none\|x\|z` |

`/hm rotate 180` and `/hm rotate none` set a rotation directly.

### Layers

Layers limit what is drawn and printed. They apply to all placements, and locking doesn't affect them.

| Button | What it does | Command |
|---|---|---|
| **Layers: ...** | Steps through all, this layer, above, and below | Key **Home** |
| **Layer down** | Moves the shown layer down by one | `/hm layer down`, key **Page Down** |
| **Layer up** | Moves the shown layer up by one | `/hm layer up`, key **Page Up** |
| **All layers** | Shows everything again | `/hm layer all` |

Only with a command: a certain height (`/hm layer <y>`), the height you stand at (`/hm layer here`), everything above
or below a height (`/hm layer above <y>`, `/hm layer below <y>`), and a range (`/hm layer range <a> <b>`).

### Selection tool

| Button | What it does | Command |
|---|---|---|
| **Selection tool: area/placement mode** | Switches what the [selection tool](Tool.md) works on | `/hm tool mode area\|placement` |

## Good to know

- Where two placements overlap, the later one's blocks win. Its air doesn't erase the other's blocks.
- Placements are remembered per world and dimension, and come back when you join again.
- Every shown placement has a frame: faint normally, bright when selected. A selected frame is cyan, or orange when
  locked. **Boxes around placements** in the [Display tab](Display.md) turns the frames off.
