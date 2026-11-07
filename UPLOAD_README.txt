Upload fork (this repository)
===============================
This is Horizonmatica (keksi2k/horizonmatica) with one addition:

- In-game Upload tab (press M, tab 8): paste the full path to a
  .litematic / .schem / .schematic / .nbt file on your computer and press
  Upload. The file is copied into the schematics folder and loaded as a
  placement. Schematics folder path is shown on the tab.
- /hm upload <path> in chat does the same.

Install:
  horizon-launcher mods add-github https://github.com/NotDiscxrd23/horizonmatica
  horizon-launcher native-build --instance horizon --trust-native-code --allow-version-mismatch
Release v0.2.0-upload ships horizonmatica-0.2.0.hnmod pinned to Horizon
67dad0d1a5adca139b426c8114e74a33b376ba64 (sha256 094398546873593abe368bc8218df5bbfafa725bb0fe93df566ea5ee5e6810f7).
Build from source:
  horizon-launcher native-build ./mixins --instance horizon --trust-native-code --profile dev --allow-version-mismatch
