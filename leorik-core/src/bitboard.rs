//! Bitboard helpers and slider attack generation (Leorik's "Classic" subset method).

pub use crate::tables::{DIAGONAL_MASK, KING_TARGETS, KNIGHT_TARGETS, ORTHOGONAL_MASK};
use crate::tables::{ANTI_DIAGONAL, DIAGONAL};

#[inline(always)]
pub fn lsb(bb: u64) -> usize {
    bb.trailing_zeros() as usize
}

#[inline(always)]
pub fn clear_lsb(bb: u64) -> u64 {
    bb & bb.wrapping_sub(1)
}

#[inline(always)]
pub fn pop_count(bb: u64) -> u32 {
    bb.count_ones()
}

#[inline(always)]
pub fn rank(square: usize) -> usize {
    square >> 3
}

#[inline(always)]
pub fn file(square: usize) -> usize {
    square & 7
}

/// Mirrors the ranks: all bits keep their file but their rank is mirrored.
#[inline(always)]
pub fn byte_swap(bb: u64) -> u64 {
    bb.swap_bytes()
}

/// Identify the highest set bit and set all bits below it, zeroing the rest.
#[inline(always)]
pub fn mask_high(bb: u64) -> u64 {
    0x7FFF_FFFF_FFFF_FFFFu64 >> (bb | 1).leading_zeros()
}

/// Identify the lowest set bit and set all bits below (inclusive), zeroing the rest.
#[inline(always)]
pub fn mask_low(bb: u64) -> u64 {
    bb ^ bb.wrapping_sub(1)
}

#[inline(always)]
pub fn lowest_bit(bb: u64) -> u64 {
    bb & !bb.wrapping_sub(1)
}

#[inline(always)]
pub fn highest_bit(bb: u64) -> u64 {
    if bb == 0 {
        0
    } else {
        1u64 << (63 - bb.leading_zeros())
    }
}

/// All bits strictly below `square`.
#[inline(always)]
pub fn lower_bits(square: usize) -> u64 {
    (1u64 << square).wrapping_sub(1)
}

/// All bits strictly above `square`.
#[inline(always)]
pub fn higher_bits(square: usize) -> u64 {
    // (1 << 64) wraps to 1 in C# (shift count is masked), so LowerBits(64) == 0 and HigherBits(63) == !0.
    // That case never matters for castling, but we mirror the C# semantics anyway.
    !lower_bits_wrapping(square + 1)
}

#[inline(always)]
fn lower_bits_wrapping(square: usize) -> u64 {
    (1u64.wrapping_shl(square as u32)).wrapping_sub(1)
}

/// Bits between two squares (inclusive).
#[inline(always)]
pub fn bits_between(square1: usize, square2: usize) -> u64 {
    let m1 = 1u64 << square1;
    let m2 = 1u64 << square2;
    (m1.wrapping_sub(1) ^ m2.wrapping_sub(1)) | m1 | m2
}

const HORIZONTAL: u64 = 0x0000_0000_0000_00FF;
const VERTICAL: u64 = 0x0101_0101_0101_0101;

#[inline(always)]
fn subset(line: u64, blocker: u64, below: u64) -> u64 {
    (mask_low(blocker & !below) ^ mask_high(blocker & below)) & line
}

#[inline(always)]
pub fn bishop_attacks(occupation: u64, square: usize) -> u64 {
    let piece = 1u64 << square;
    let blocker = occupation & !piece;
    let below = piece - 1;
    let diagonal = DIAGONAL[square];
    let anti_diagonal = ANTI_DIAGONAL[square];
    (subset(diagonal, blocker & diagonal, below) | subset(anti_diagonal, blocker & anti_diagonal, below)) & !piece
}

#[inline(always)]
pub fn rook_attacks(occupation: u64, square: usize) -> u64 {
    let piece = 1u64 << square;
    let blocker = occupation & !piece;
    let below = piece - 1;
    let horizontal = HORIZONTAL << (square & 56);
    let vertical = VERTICAL << (square & 7);
    (subset(horizontal, blocker & horizontal, below) | subset(vertical, blocker & vertical, below)) & !piece
}

#[inline(always)]
pub fn bishop_targets(occupation: u64, square: usize) -> u64 {
    bishop_attacks(occupation, square)
}

#[inline(always)]
pub fn rook_targets(occupation: u64, square: usize) -> u64 {
    rook_attacks(occupation, square)
}

#[inline(always)]
pub fn queen_targets(occupation: u64, square: usize) -> u64 {
    bishop_attacks(occupation, square) | rook_attacks(occupation, square)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slow_ray(occ: u64, sq: usize, dirs: &[(i32, i32)]) -> u64 {
        let mut result = 0;
        for &(df, dr) in dirs {
            let (mut f, mut r) = (file(sq) as i32, rank(sq) as i32);
            loop {
                f += df;
                r += dr;
                if !(0..8).contains(&f) || !(0..8).contains(&r) {
                    break;
                }
                let bit = 1u64 << (r * 8 + f);
                result |= bit;
                if occ & bit != 0 {
                    break;
                }
            }
        }
        result
    }

    #[test]
    fn slider_attacks_match_ray_walk() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..20000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let occ = x & (x >> 3);
            let sq = (x >> 58) as usize;
            assert_eq!(bishop_attacks(occ, sq), slow_ray(occ, sq, &[(1, 1), (-1, 1), (1, -1), (-1, -1)]));
            assert_eq!(rook_attacks(occ, sq), slow_ray(occ, sq, &[(1, 0), (-1, 0), (0, 1), (0, -1)]));
        }
    }
}
