//! Generates the magic bitboard tables for slider attacks (see `src/bitboard.rs`).
//!
//! The magic numbers are found with a fixed-seed random search, so the output is deterministic.
//! Every table entry is computed with a plain ray walk, and a magic is only accepted if it maps
//! all blocker subsets of a square without a destructive collision, so the lookups return exactly
//! the attack sets of the ray walk (which the `slider_attacks_match_ray_walk` test also checks
//! against Leorik's "Classic" subset method).

use std::fmt::Write as _;
use std::path::Path;

const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (-1, 1), (1, -1), (-1, -1)];

fn ray_attacks(occupied: u64, square: usize, dirs: &[(i32, i32)]) -> u64 {
    let mut result = 0;
    for &(df, dr) in dirs {
        let (mut f, mut r) = ((square & 7) as i32, (square >> 3) as i32);
        loop {
            f += df;
            r += dr;
            if !(0..8).contains(&f) || !(0..8).contains(&r) {
                break;
            }
            let bit = 1u64 << (r * 8 + f);
            result |= bit;
            if occupied & bit != 0 {
                break;
            }
        }
    }
    result
}

/// Squares whose occupancy matters: the rays without their last square (the board edge).
fn relevant_mask(square: usize, dirs: &[(i32, i32)]) -> u64 {
    let mut result = 0;
    for &(df, dr) in dirs {
        let (mut f, mut r) = ((square & 7) as i32, (square >> 3) as i32);
        loop {
            f += df;
            r += dr;
            if !(0..8).contains(&(f + df)) || !(0..8).contains(&(r + dr)) {
                break;
            }
            result |= 1u64 << (r * 8 + f);
        }
    }
    result
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

struct Magic {
    mask: u64,
    magic: u64,
    shift: u32,
    offset: usize,
}

fn find_magics(dirs: &[(i32, i32)], rng: &mut Rng, table: &mut Vec<u64>) -> Vec<Magic> {
    let mut magics = Vec::new();
    for square in 0..64 {
        let mask = relevant_mask(square, dirs);
        let bits = mask.count_ones();
        let shift = 64 - bits;
        // enumerate all subsets of the mask (Carry-Rippler)
        let mut subsets = Vec::new();
        let mut subset = 0u64;
        loop {
            subsets.push((subset, ray_attacks(subset, square, dirs)));
            subset = subset.wrapping_sub(mask) & mask;
            if subset == 0 {
                break;
            }
        }
        let mut entries = vec![0u64; 1 << bits];
        let mut used = vec![0u32; 1 << bits];
        let mut epoch = 0u32;
        let magic = loop {
            let magic = rng.sparse();
            if (mask.wrapping_mul(magic) >> 56).count_ones() < 6 {
                continue;
            }
            epoch += 1;
            let ok = subsets.iter().all(|&(occ, attacks)| {
                let index = (occ.wrapping_mul(magic) >> shift) as usize;
                if used[index] != epoch {
                    used[index] = epoch;
                    entries[index] = attacks;
                    true
                } else {
                    entries[index] == attacks
                }
            });
            if ok {
                break magic;
            }
        };
        let offset = table.len();
        table.extend_from_slice(&entries);
        magics.push(Magic { mask, magic, shift, offset });
    }
    magics
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut table = Vec::new();
    let rook = find_magics(&ROOK_DIRS, &mut rng, &mut table);
    let bishop = find_magics(&BISHOP_DIRS, &mut rng, &mut table);

    let mut src = String::new();
    writeln!(src, "pub const SLIDER_TABLE_SIZE: usize = {};", table.len()).unwrap();
    for (name, magics) in [("ROOK_MAGICS", &rook), ("BISHOP_MAGICS", &bishop)] {
        writeln!(src, "pub static {name}: [Magic; 64] = [").unwrap();
        for m in magics.iter() {
            writeln!(
                src,
                "    Magic {{ mask: {:#018x}, magic: {:#018x}, shift: {}, offset: {} }},",
                m.mask, m.magic, m.shift, m.offset
            )
            .unwrap();
        }
        writeln!(src, "];").unwrap();
    }
    std::fs::write(Path::new(&out_dir).join("magics.rs"), src).unwrap();

    let bytes: Vec<u8> = table.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(Path::new(&out_dir).join("slider_table.bin"), bytes).unwrap();
}
