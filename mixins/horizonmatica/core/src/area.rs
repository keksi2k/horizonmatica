//! The area selection's corners: the block positions they may take, and the box's size and
//! volume, computed wide and checked so no corner pair can overflow.

/// The block positions Minecraft can address: a packed `BlockPos` holds x and z in 26 bits and
/// y in 12 (the world border, at most 29,999,984 out, lies well inside).
pub const MIN_XZ: i32 = -(1 << 25);
pub const MAX_XZ: i32 = (1 << 25) - 1;
pub const MIN_Y: i32 = -(1 << 11);
pub const MAX_Y: i32 = (1 << 11) - 1;

const MIN: [i32; 3] = [MIN_XZ, MIN_Y, MIN_XZ];
const MAX: [i32; 3] = [MAX_XZ, MAX_Y, MAX_XZ];

/// `Err` names the first coordinate of `c` outside the positions a world can have.
pub fn check_corner(c: [i32; 3]) -> Result<[i32; 3], String> {
    for k in 0..3 {
        if !(MIN[k]..=MAX[k]).contains(&c[k]) {
            return Err(format!("{} must be {} to {}, not {}", ["x", "y", "z"][k], MIN[k], MAX[k], c[k]));
        }
    }
    Ok(c)
}

/// `c` moved into the positions a world can have (the selection tool's wheel stops at the edge).
pub fn clamp_corner(c: [i32; 3]) -> [i32; 3] {
    [0, 1, 2].map(|k| c[k].clamp(MIN[k], MAX[k]))
}

/// `c` plus `d` along each axis, stopped at the edge of the positions a world can have.
pub fn offset_corner(c: [i32; 3], d: [i32; 3]) -> [i32; 3] {
    clamp_corner([0, 1, 2].map(|k| c[k].saturating_add(d[k])))
}

/// The box from `a` to `b` (both included, either order) in blocks along each axis, or `None`
/// when a side doesn't fit an `i32`.
pub fn size(a: [i32; 3], b: [i32; 3]) -> Option<[i32; 3]> {
    let mut out = [0; 3];
    for k in 0..3 {
        let d = (i64::from(a[k]) - i64::from(b[k])).abs() + 1;
        out[k] = i32::try_from(d).ok()?;
    }
    Some(out)
}

/// The number of blocks in a box of `size`, or `None` when a side isn't positive or the
/// product doesn't fit a `u64`.
pub fn volume(size: [i32; 3]) -> Option<u64> {
    size.iter().try_fold(1u64, |v, &s| v.checked_mul(u64::try_from(s).ok().filter(|&s| s > 0)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_inside_the_world_pass() {
        assert_eq!(check_corner([0, 64, 0]), Ok([0, 64, 0]));
        assert!(check_corner([MIN_XZ, MIN_Y, MIN_XZ]).is_ok());
        assert!(check_corner([MAX_XZ, MAX_Y, MAX_XZ]).is_ok());
        assert!(check_corner([29_999_984, -64, -29_999_984]).is_ok());
    }

    #[test]
    fn corners_outside_the_world_fail() {
        assert!(check_corner([i32::MIN, 0, 0]).unwrap_err().starts_with("x "));
        assert!(check_corner([i32::MAX, 0, 0]).is_err());
        assert!(check_corner([MAX_XZ + 1, 0, 0]).is_err());
        assert!(check_corner([MIN_XZ - 1, 0, 0]).is_err());
        assert!(check_corner([0, MAX_Y + 1, 0]).unwrap_err().starts_with("y "));
        assert!(check_corner([0, MIN_Y - 1, 0]).is_err());
        assert!(check_corner([0, 0, i32::MIN]).unwrap_err().starts_with("z "));
    }

    #[test]
    fn size_of_ordinary_boxes() {
        assert_eq!(size([0, 0, 0], [0, 0, 0]), Some([1, 1, 1]));
        assert_eq!(size([5, 70, -3], [-4, 60, 2]), Some([10, 11, 6]));
        assert_eq!(size([-4, 60, 2], [5, 70, -3]), Some([10, 11, 6]));
    }

    #[test]
    fn size_at_the_i32_extremes_does_not_overflow() {
        // The reported case: corner 1 at i32::MIN, corner 2 at i32::MAX.
        assert_eq!(size([i32::MIN, 0, 0], [i32::MAX, 0, 0]), None);
        assert_eq!(size([0, i32::MIN, 0], [0, i32::MAX, 0]), None);
        assert_eq!(size([0, 0, i32::MAX], [0, 0, i32::MIN]), None);
        // The widest that still fits: 2^31 - 1 blocks.
        assert_eq!(size([0, 0, 0], [i32::MAX - 1, 0, 0]), Some([i32::MAX, 1, 1]));
        assert_eq!(size([-1, 0, 0], [i32::MAX - 1, 0, 0]), None);
        assert_eq!(size([i32::MIN, 0, 0], [i32::MIN, 0, 0]), Some([1, 1, 1]));
    }

    #[test]
    fn size_across_the_whole_world_fits() {
        let s = size([MIN_XZ, MIN_Y, MIN_XZ], [MAX_XZ, MAX_Y, MAX_XZ]).unwrap();
        assert_eq!(s, [1 << 26, 1 << 12, 1 << 26]);
    }

    #[test]
    fn volume_is_checked() {
        assert_eq!(volume([1, 1, 1]), Some(1));
        assert_eq!(volume([10, 11, 6]), Some(660));
        assert_eq!(volume([i32::MAX, i32::MAX, 2]), Some(2 * (i32::MAX as u64).pow(2)));
        // 2^26 * 2^12 * 2^26 = 2^64: the whole world is one past u64.
        assert_eq!(volume([1 << 26, 1 << 12, 1 << 26]), None);
        assert_eq!(volume([i32::MAX, i32::MAX, i32::MAX]), None);
        assert_eq!(volume([0, 1, 1]), None);
        assert_eq!(volume([-1, 1, 1]), None);
    }

    #[test]
    fn offsets_stop_at_the_edge() {
        assert_eq!(offset_corner([1, 2, 3], [1, -1, 0]), [2, 1, 3]);
        assert_eq!(offset_corner([MAX_XZ, 0, 0], [5, 0, 0]), [MAX_XZ, 0, 0]);
        assert_eq!(offset_corner([0, MIN_Y, 0], [0, i32::MIN, 0]), [0, MIN_Y, 0]);
        assert_eq!(offset_corner([0, 0, i32::MAX], [0, 0, i32::MAX]), [0, 0, MAX_XZ]);
        assert_eq!(clamp_corner([i32::MIN, i32::MAX, 7]), [MIN_XZ, MAX_Y, 7]);
    }
}
