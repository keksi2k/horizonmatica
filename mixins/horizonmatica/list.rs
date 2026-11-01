// The mod's entry and keys in Options > Mods.
fn horizonmatica_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    mods.push(crate::game::horizonmatica::listed());
}
