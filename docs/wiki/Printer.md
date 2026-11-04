# Printer tab

[Back to the overview](Home.md)

The printer places missing blocks of the shown placements within reach, from the bottom up and only in the layers
shown. It needs a loaded schematic, and it is off each time you start the game.

> **On other people's servers, automatic placing may break the rules or trip anti-cheat.**

| Control | Default | What it does | Command |
|---|---|---|---|
| **Printer: ON/OFF** | Off | Starts or stops building | `/hm printer [on\|off]`, key **K** |
| **One block every** | 3 ticks | Speed, from 1 to 20 ticks per block. Lower is faster | `/hm printer speed <ticks>` |
| **Reach** | 4.5 blocks | How far it places, from 1 to 6 blocks | `/hm printer reach <blocks>` |
| **Air place** | On | When no neighbouring block can be clicked, it clicks the empty spot itself. Anti-cheat plugins notice this | `/hm printer air [on\|off]` |

The tab also shows **Placed this session**, and a note on what the printer is doing or why it is waiting.

## Good to know

- **Survival:** the blocks must be in your hotbar. If an item is only in your inventory, the HUD says so.
- **Creative:** only the hotbar is used. The rest of the inventory stays as it is.
- **Order:** plain blocks first, then what carries or uses a signal, then what makes one, and observers last.
- **Falling blocks** are only placed onto something. If they float in the schematic, the printer leaves them out.
- **Fluids** are not placed. Blocks that can't be placed correctly from any reachable side are skipped, and the HUD
  says why.
- The printer pauses while [move mode](Keys.md#move-mode) is on. Hidden placements aren't printed.
