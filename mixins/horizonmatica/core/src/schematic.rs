//! The `.litematic` format: gzip NBT with `Metadata` and `Regions`, each region a palette and a
//! packed `BlockStates` long array indexed `y * sizeX * sizeZ + z * sizeX + x` from the region's
//! minimum corner.

use crate::bitarray::BitArray;
use crate::nbt::{self, Tag};
use std::collections::BTreeMap;

/// A block state as the file names it: `minecraft:oak_stairs` and its properties.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

impl BlockState {
    pub fn air() -> Self {
        Self { name: "minecraft:air".into(), properties: BTreeMap::new() }
    }
    pub fn new(name: &str, props: &[(&str, &str)]) -> Self {
        Self {
            name: name.into(),
            properties: props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
    /// `structure_void` cells are skipped when placing, as Litematica does.
    pub fn is_void(&self) -> bool {
        self.name == "minecraft:structure_void"
    }
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }
    /// `minecraft:oak_stairs[facing=north,half=bottom]`.
    pub fn to_string_full(&self) -> String {
        if self.properties.is_empty() {
            return self.name.clone();
        }
        let props: Vec<String> = self.properties.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{}[{}]", self.name, props.join(","))
    }
    /// Parses `name[k=v,...]`, adding `minecraft:` when the namespace is missing.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (name, rest) = match text.find('[') {
            Some(i) => (&text[..i], Some(&text[i + 1..])),
            None => (text, None),
        };
        if name.is_empty() {
            return None;
        }
        let name = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        let mut properties = BTreeMap::new();
        if let Some(rest) = rest {
            let rest = rest.strip_suffix(']')?;
            for pair in rest.split(',').filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=')?;
                properties.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        Some(Self { name, properties })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub name: String,
    pub author: String,
    pub description: String,
    pub region_count: i64,
    pub total_volume: i64,
    pub total_blocks: i64,
    pub enclosing_size: [i32; 3],
    pub time_created: i64,
    pub time_modified: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub name: String,
    /// `Position` as written: one corner of the box, relative to the schematic's origin.
    pub position: [i32; 3],
    /// `Size` as written (components may be negative).
    pub raw_size: [i32; 3],
    /// The box's minimum corner relative to the schematic origin.
    pub min: [i32; 3],
    /// `abs(Size)`.
    pub size: [i32; 3],
    pub palette: Vec<BlockState>,
    blocks: BitArray,
    /// Block entity NBT by position relative to `min`.
    pub block_entities: Vec<([i32; 3], Tag)>,
    pub entity_count: usize,
}

impl Region {
    pub fn volume(&self) -> u64 {
        self.size.iter().map(|&v| v as u64).product()
    }
    fn index(&self, x: i32, y: i32, z: i32) -> u64 {
        let [sx, _, sz] = self.size.map(|v| v as u64);
        y as u64 * sx * sz + z as u64 * sx + x as u64
    }
    /// The palette index at `local` (relative to `min`), 0 (air) outside the box.
    pub fn palette_index(&self, local: [i32; 3]) -> u32 {
        let [x, y, z] = local;
        if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return 0;
        }
        let i = self.blocks.get(self.index(x, y, z));
        if (i as usize) < self.palette.len() { i } else { 0 }
    }
    pub fn state(&self, local: [i32; 3]) -> &BlockState {
        &self.palette[self.palette_index(local) as usize]
    }
    /// A region at the origin from a palette and one palette index per cell, in Litematica's
    /// order (x fastest, then z, then y). Used by the other file formats.
    pub fn from_indices(
        name: &str,
        size: [i32; 3],
        palette: Vec<BlockState>,
        indices: &[u32],
        block_entities: Vec<([i32; 3], Tag)>,
    ) -> Self {
        let palette = if palette.is_empty() { vec![BlockState::air()] } else { palette };
        let mut blocks = BitArray::new(BitArray::bits_for(palette.len()), indices.len() as u64);
        for (i, &v) in indices.iter().enumerate() {
            if v != 0 {
                blocks.set(i as u64, v);
            }
        }
        Self {
            name: name.into(),
            position: [0; 3],
            raw_size: size,
            min: [0; 3],
            size,
            palette,
            blocks,
            block_entities,
            entity_count: 0,
        }
    }

    /// Builds a region in memory (tests, and future saving).
    pub fn from_fn(
        name: &str,
        position: [i32; 3],
        size: [i32; 3],
        mut f: impl FnMut([i32; 3]) -> BlockState,
    ) -> Self {
        let abs = size.map(i32::abs);
        let end = [0, 1, 2].map(|i| position[i] + if size[i] >= 0 { size[i] - 1 } else { size[i] + 1 });
        let min = [0, 1, 2].map(|i| position[i].min(end[i]));
        let mut palette = vec![BlockState::air()];
        let mut indices = Vec::new();
        for y in 0..abs[1] {
            for z in 0..abs[2] {
                for x in 0..abs[0] {
                    let s = f([x, y, z]);
                    let i = palette.iter().position(|p| *p == s).unwrap_or_else(|| {
                        palette.push(s);
                        palette.len() - 1
                    });
                    indices.push(i as u32);
                }
            }
        }
        let mut blocks = BitArray::new(BitArray::bits_for(palette.len()), indices.len() as u64);
        for (i, v) in indices.into_iter().enumerate() {
            blocks.set(i as u64, v);
        }
        Self {
            name: name.into(),
            position,
            raw_size: size,
            min,
            size: abs,
            palette,
            blocks,
            block_entities: Vec::new(),
            entity_count: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Schematic {
    pub version: i64,
    pub data_version: i64,
    pub metadata: Metadata,
    pub regions: Vec<Region>,
}

/// Upper bound on a file's unpacked size.
pub const MAX_UNPACKED: usize = 512 << 20;
/// Upper bound on a region's volume (Litematica itself allows large areas; this keeps memory sane).
pub const MAX_VOLUME: u64 = 256 * 1024 * 1024;

fn vec3(tag: Option<&Tag>) -> Option<[i32; 3]> {
    let t = tag?;
    Some([t.get("x")?.int()? as i32, t.get("y")?.int()? as i32, t.get("z")?.int()? as i32])
}

pub(crate) fn read_state(tag: &Tag) -> Result<BlockState, String> {
    let name = tag.get("Name").and_then(Tag::str).ok_or("palette entry without Name")?;
    let mut properties = BTreeMap::new();
    if let Some(props) = tag.get("Properties").and_then(Tag::compound) {
        for (k, v) in props {
            if let Some(v) = v.str() {
                properties.insert(k.clone(), v.to_string());
            }
        }
    }
    Ok(BlockState { name: name.to_string(), properties })
}

impl Schematic {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let root = nbt::read_file_bytes(bytes, MAX_UNPACKED)?;
        Self::from_nbt(&root)
    }

    pub fn from_nbt(root: &Tag) -> Result<Self, String> {
        let version = root.get("Version").and_then(Tag::int).ok_or("not a .litematic file (no Version)")?;
        if !(1..=7).contains(&version) {
            return Err(format!("unsupported .litematic version {version}"));
        }
        let data_version = root.get("MinecraftDataVersion").and_then(Tag::int).unwrap_or(0);
        let m = root.get("Metadata");
        let s = |k: &str| m.and_then(|m| m.get(k)).and_then(Tag::str).unwrap_or("").to_string();
        let i = |k: &str| m.and_then(|m| m.get(k)).and_then(Tag::int).unwrap_or(0);
        let metadata = Metadata {
            name: s("Name"),
            author: s("Author"),
            description: s("Description"),
            region_count: i("RegionCount"),
            total_volume: i("TotalVolume"),
            total_blocks: i("TotalBlocks"),
            enclosing_size: vec3(m.and_then(|m| m.get("EnclosingSize"))).unwrap_or([0; 3]),
            time_created: i("TimeCreated"),
            time_modified: i("TimeModified"),
        };
        let regions_tag = root.get("Regions").and_then(Tag::compound).ok_or("no Regions")?;
        let mut regions = Vec::new();
        for (name, r) in regions_tag {
            let position = vec3(r.get("Position")).ok_or_else(|| format!("region {name}: no Position"))?;
            let raw_size = vec3(r.get("Size")).ok_or_else(|| format!("region {name}: no Size"))?;
            let size = raw_size.map(i32::abs);
            let end = [0, 1, 2].map(|i| {
                position[i] + if raw_size[i] >= 0 { raw_size[i] - 1 } else { raw_size[i] + 1 }
            });
            let min = [0, 1, 2].map(|i| position[i].min(end[i]));
            let volume: u64 = size.iter().map(|&v| v as u64).product();
            if volume > MAX_VOLUME {
                return Err(format!("region {name} is too large ({volume} blocks)"));
            }
            let palette: Vec<BlockState> = r
                .get("BlockStatePalette")
                .and_then(Tag::list)
                .ok_or_else(|| format!("region {name}: no BlockStatePalette"))?
                .iter()
                .map(read_state)
                .collect::<Result<_, _>>()?;
            let palette = if palette.is_empty() { vec![BlockState::air()] } else { palette };
            let longs = r.get("BlockStates").and_then(Tag::longs).unwrap_or(&[]);
            let bits = BitArray::bits_for(palette.len());
            let blocks = BitArray::from_longs(longs, bits, volume)
                .map_err(|e| format!("region {name}: {e}"))?;
            let block_entities = r
                .get("TileEntities")
                .and_then(Tag::list)
                .unwrap_or(&[])
                .iter()
                .filter_map(|t| {
                    let p = [t.get("x")?.int()? as i32, t.get("y")?.int()? as i32, t.get("z")?.int()? as i32];
                    Some((p, t.clone()))
                })
                .collect();
            let entity_count = r.get("Entities").and_then(Tag::list).map_or(0, <[Tag]>::len);
            regions.push(Region {
                name: name.clone(),
                position,
                raw_size,
                min,
                size,
                palette,
                blocks,
                block_entities,
                entity_count,
            });
        }
        Ok(Self { version, data_version, metadata, regions })
    }

    /// Writes the schematic as a `.litematic` (gzip NBT, version 6).
    pub fn to_bytes(&self) -> Vec<u8> {
        let v3 = |p: [i32; 3]| {
            Tag::Compound(BTreeMap::from([
                ("x".into(), Tag::Int(p[0])),
                ("y".into(), Tag::Int(p[1])),
                ("z".into(), Tag::Int(p[2])),
            ]))
        };
        let mut regions = BTreeMap::new();
        for r in &self.regions {
            let palette = r
                .palette
                .iter()
                .map(|s| {
                    let mut m = BTreeMap::from([("Name".to_string(), Tag::String(s.name.clone()))]);
                    if !s.properties.is_empty() {
                        m.insert(
                            "Properties".into(),
                            Tag::Compound(
                                s.properties.iter().map(|(k, v)| (k.clone(), Tag::String(v.clone()))).collect(),
                            ),
                        );
                    }
                    Tag::Compound(m)
                })
                .collect();
            regions.insert(
                r.name.clone(),
                Tag::Compound(BTreeMap::from([
                    ("Position".into(), v3(r.position)),
                    ("Size".into(), v3(r.raw_size)),
                    ("BlockStatePalette".into(), Tag::List(palette)),
                    (
                        "BlockStates".into(),
                        Tag::LongArray(r.blocks.longs().iter().map(|&v| v as i64).collect()),
                    ),
                    ("TileEntities".into(), Tag::List(vec![])),
                    ("Entities".into(), Tag::List(vec![])),
                    ("PendingBlockTicks".into(), Tag::List(vec![])),
                ])),
            );
        }
        let m = &self.metadata;
        let metadata = Tag::Compound(BTreeMap::from([
            ("Name".into(), Tag::String(m.name.clone())),
            ("Author".into(), Tag::String(m.author.clone())),
            ("Description".into(), Tag::String(m.description.clone())),
            ("RegionCount".into(), Tag::Int(self.regions.len() as i32)),
            ("TotalVolume".into(), Tag::Int(m.total_volume as i32)),
            ("TotalBlocks".into(), Tag::Int(m.total_blocks as i32)),
            ("TimeCreated".into(), Tag::Long(m.time_created)),
            ("TimeModified".into(), Tag::Long(m.time_modified)),
            ("EnclosingSize".into(), v3(m.enclosing_size)),
        ]));
        let root = Tag::Compound(BTreeMap::from([
            ("MinecraftDataVersion".into(), Tag::Int(self.data_version as i32)),
            ("Version".into(), Tag::Int(6)),
            ("Metadata".into(), metadata),
            ("Regions".into(), Tag::Compound(regions)),
        ]));
        nbt::write_gzip("", &root)
    }

    /// The bounding box of all regions, relative to the origin: (min, max inclusive).
    pub fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        for r in &self.regions {
            for i in 0..3 {
                lo[i] = lo[i].min(r.min[i]);
                hi[i] = hi[i].max(r.min[i] + r.size[i] - 1);
            }
        }
        if self.regions.is_empty() {
            return ([0; 3], [0; 3]);
        }
        (lo, hi)
    }

    /// Non-air, non-void blocks in all regions.
    pub fn block_count(&self) -> u64 {
        let mut n = 0;
        for r in &self.regions {
            let solid: Vec<bool> = r.palette.iter().map(|s| !s.is_air() && !s.is_void()).collect();
            for i in 0..r.volume() {
                if solid.get(r.blocks.get(i) as usize).copied().unwrap_or(false) {
                    n += 1;
                }
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Schematic {
        // A region whose Size is negative in x and z: it extends from Position towards -x/-z.
        let region = Region::from_fn("main", [2, 0, 3], [-3, 2, -4], |[x, y, z]| {
            if y == 0 {
                BlockState::new("minecraft:stone", &[])
            } else if x == 0 && z == 0 {
                BlockState::new("minecraft:oak_stairs", &[("facing", "north"), ("half", "bottom")])
            } else {
                BlockState::air()
            }
        });
        Schematic {
            version: 6,
            data_version: 4000,
            metadata: Metadata { name: "Test".into(), author: "me".into(), ..Default::default() },
            regions: vec![region],
        }
    }

    #[test]
    fn negative_size_gives_min_corner() {
        let s = sample();
        let r = &s.regions[0];
        assert_eq!(r.min, [0, 0, 0]);
        assert_eq!(r.size, [3, 2, 4]);
        assert_eq!(s.bounds(), ([0, 0, 0], [2, 1, 3]));
    }

    #[test]
    fn file_round_trip() {
        let s = sample();
        let back = Schematic::from_bytes(&s.to_bytes()).unwrap();
        let r = &back.regions[0];
        assert_eq!(back.metadata.name, "Test");
        assert_eq!(r.state([1, 0, 2]).name, "minecraft:stone");
        assert_eq!(r.state([0, 1, 0]).prop("facing"), Some("north"));
        assert!(r.state([1, 1, 1]).is_air());
        assert_eq!(back.block_count(), 3 * 4 + 1);
    }

    #[test]
    fn parse_state_strings() {
        let s = BlockState::parse("oak_stairs[facing=east,half=top]").unwrap();
        assert_eq!(s.name, "minecraft:oak_stairs");
        assert_eq!(s.prop("half"), Some("top"));
        assert_eq!(s.to_string_full(), "minecraft:oak_stairs[facing=east,half=top]");
        assert!(BlockState::parse("stone[facing").is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(Schematic::from_bytes(b"not nbt").is_err());
    }
}
