//! Efficiently updatable NNUE evaluation (port of Leorik's `NeuralNetEval`).
//!
//! The accumulators use wrapping 16-bit arithmetic exactly like Leorik's `Vector256<short>`
//! code, so the results are bit-identical. On aarch64 the hot loops use NEON intrinsics,
//! everywhere else a scalar implementation (which compilers auto-vectorize) is used.

use crate::bitboard::{clear_lsb, file, lsb, pop_count};
use crate::board::PieceView as Board;
use crate::moves::Move;
use crate::network::{self, network, INPUT_BUCKETS, INPUT_BUCKET_MAP, INPUT_SIZE, LAYER1_SIZE, QA, QB, SCALE};
use crate::piece::*;
use core::cell::{Cell, RefCell};

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Bucket {
    pub index: u8,
    pub mirrored: bool,
}

#[derive(Clone)]
#[repr(C, align(64))]
pub struct Accumulator(pub [i16; LAYER1_SIZE]);

#[derive(Clone)]
#[repr(C)]
pub struct NeuralNetEval {
    pub black: Accumulator,
    pub white: Accumulator,
    pub black_bucket: Bucket,
    pub white_bucket: Bucket,
    /// Output of the network for the side to move, computed on first use (see [`NeuralNetEval::score`]).
    /// `NO_SCORE` means not computed yet. Many positions are never evaluated (TT cutoffs, draws, mate
    /// distance pruning), so the output layer is only run when the score is actually read.
    score: Cell<i32>,
}

const NO_SCORE: i32 = i32::MIN;

impl Default for NeuralNetEval {
    fn default() -> Self {
        NeuralNetEval::new()
    }
}

const COLOR_STRIDE: usize = 64 * 6;
const PIECE_STRIDE: usize = 64;
const BUCKET_STRIDE: usize = INPUT_SIZE;

#[inline(always)]
fn king_bucket(color: Color, board: &Board) -> Bucket {
    let square = if color == Color::Black {
        lsb(board.black & board.kings) ^ 56
    } else {
        lsb(board.white & board.kings)
    };
    Bucket { index: INPUT_BUCKET_MAP[square], mirrored: file(square) >= 4 }
}

/// Offset of the feature's weights in `Network::feature_weights` for the given perspective.
#[inline(always)]
fn feature_offset(color: Color, bucket: Bucket, piece: Piece, square: usize) -> usize {
    let piece_type = (((piece & TYPE_MASK) >> 2) - 1) as usize;
    let white = ((piece & COLOR_MASK) >> 1) as usize;
    let index = if color == Color::Black {
        let black_square = square ^ if bucket.mirrored { 63 } else { 56 };
        bucket.index as usize * BUCKET_STRIDE + white * COLOR_STRIDE + piece_type * PIECE_STRIDE + black_square
    } else {
        let white_square = square ^ if bucket.mirrored { 7 } else { 0 };
        bucket.index as usize * BUCKET_STRIDE + (white ^ 1) * COLOR_STRIDE + piece_type * PIECE_STRIDE + white_square
    };
    index * LAYER1_SIZE
}

#[inline(always)]
fn weights(offset: usize) -> &'static [i16; LAYER1_SIZE] {
    network().feature_weights[offset..offset + LAYER1_SIZE].try_into().unwrap()
}

/// Up to two added and two removed features: enough for any move incl. castling and en passant.
struct Delta {
    add: [usize; 2],
    sub: [usize; 2],
    adds: usize,
    subs: usize,
}

impl Delta {
    #[inline(always)]
    fn new() -> Delta {
        Delta { add: [0; 2], sub: [0; 2], adds: 0, subs: 0 }
    }

    #[inline(always)]
    fn activate(&mut self, color: Color, bucket: Bucket, piece: Piece, square: usize) {
        if piece != NONE {
            self.add[self.adds] = feature_offset(color, bucket, piece, square);
            self.adds += 1;
        }
    }

    #[inline(always)]
    fn deactivate(&mut self, color: Color, bucket: Bucket, piece: Piece, square: usize) {
        if piece != NONE {
            self.sub[self.subs] = feature_offset(color, bucket, piece, square);
            self.subs += 1;
        }
    }

    #[inline(always)]
    fn from_move(color: Color, bucket: Bucket, mv: Move) -> Delta {
        let mut d = Delta::new();
        d.deactivate(color, bucket, mv.moving_piece(), mv.from as usize);
        d.deactivate(color, bucket, mv.target, mv.to as usize);
        d.activate(color, bucket, mv.new_piece(), mv.to as usize);

        const BLACK_EP: Piece = EN_PASSANT | BLACK_PAWN;
        const WHITE_EP: Piece = EN_PASSANT | WHITE_PAWN;
        const BLACK_SHORT: Piece = CASTLE_SHORT | BLACK;
        const BLACK_LONG: Piece = CASTLE_LONG | BLACK;
        const WHITE_SHORT: Piece = CASTLE_SHORT | WHITE;
        const WHITE_LONG: Piece = CASTLE_LONG | WHITE;
        match mv.flags {
            BLACK_EP => d.deactivate(color, bucket, WHITE_PAWN, mv.to as usize + 8),
            WHITE_EP => d.deactivate(color, bucket, BLACK_PAWN, mv.to as usize - 8),
            BLACK_SHORT => {
                d.activate(color, bucket, BLACK_ROOK, 61);
                d.activate(color, bucket, BLACK_KING, 62);
            }
            BLACK_LONG => {
                d.activate(color, bucket, BLACK_ROOK, 59);
                d.activate(color, bucket, BLACK_KING, 58);
            }
            WHITE_SHORT => {
                d.activate(color, bucket, WHITE_ROOK, 5);
                d.activate(color, bucket, WHITE_KING, 6);
            }
            WHITE_LONG => {
                d.activate(color, bucket, WHITE_ROOK, 3);
                d.activate(color, bucket, WHITE_KING, 2);
            }
            _ => {}
        }
        d
    }
}

impl NeuralNetEval {
    pub fn new() -> NeuralNetEval {
        NeuralNetEval {
            black: Accumulator([0; LAYER1_SIZE]),
            white: Accumulator([0; LAYER1_SIZE]),
            black_bucket: Bucket::default(),
            white_bucket: Bucket::default(),
            score: Cell::new(NO_SCORE),
        }
    }

    #[inline(always)]
    pub fn copy_from(&mut self, other: &NeuralNetEval) {
        self.black.0.copy_from_slice(&other.black.0);
        self.white.0.copy_from_slice(&other.white.0);
        self.black_bucket = other.black_bucket;
        self.white_bucket = other.white_bucket;
        self.score.set(other.score.get());
    }

    /// Rebuilds both accumulators from scratch.
    pub fn update(&mut self, board: &Board) {
        self.rebuild(Color::White, board);
        self.rebuild(Color::Black, board);
        self.score.set(NO_SCORE);
    }

    /// Derives this evaluation from `from` (the evaluation before `mv` was played) and the new board.
    #[inline(always)]
    pub fn update_incremental(&mut self, from: &NeuralNetEval, mv: Move, new_board: &Board) {
        if king_bucket(Color::White, new_board) != from.white_bucket {
            self.black_bucket = from.black_bucket;
            apply_delta(&mut self.black, &from.black, &Delta::from_move(Color::Black, from.black_bucket, mv));
            self.refresh(Color::White, new_board);
        } else if king_bucket(Color::Black, new_board) != from.black_bucket {
            self.white_bucket = from.white_bucket;
            apply_delta(&mut self.white, &from.white, &Delta::from_move(Color::White, from.white_bucket, mv));
            self.refresh(Color::Black, new_board);
        } else {
            self.black_bucket = from.black_bucket;
            self.white_bucket = from.white_bucket;
            apply_delta(&mut self.black, &from.black, &Delta::from_move(Color::Black, from.black_bucket, mv));
            apply_delta(&mut self.white, &from.white, &Delta::from_move(Color::White, from.white_bucket, mv));
        }
        self.score.set(NO_SCORE);
    }

    fn rebuild(&mut self, color: Color, board: &Board) {
        let bucket = king_bucket(color, board);
        let accu = if color == Color::White {
            self.white_bucket = bucket;
            &mut self.white
        } else {
            self.black_bucket = bucket;
            &mut self.black
        };
        accu.0.copy_from_slice(&network().feature_biases);
        let mut bits = board.white | board.black;
        while bits != 0 {
            let square = lsb(bits);
            let piece = board.get_piece(square);
            add_weights(accu, weights(feature_offset(color, bucket, piece, square)));
            bits = clear_lsb(bits);
        }
    }

    /// Like `rebuild`, but starts from the accumulator this thread last built for the same perspective and
    /// king bucket and only applies the pieces that changed since then (a "Finny table"). The accumulator is
    /// the biases plus the sum of the active features' weights in wrapping 16-bit arithmetic, which doesn't
    /// depend on the order of additions and subtractions, so the result is identical to `rebuild`.
    fn refresh(&mut self, color: Color, board: &Board) {
        let bucket = king_bucket(color, board);
        let accu = if color == Color::White {
            self.white_bucket = bucket;
            &mut self.white
        } else {
            self.black_bucket = bucket;
            &mut self.black
        };
        REFRESH_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let perspective = (color == Color::White) as usize;
            let entry = &mut cache[(perspective * INPUT_BUCKETS + bucket.index as usize) * 2 + bucket.mirrored as usize];
            let types = [board.pawns, board.knights, board.bishops, board.rooks, board.queens, board.kings];
            for (c, (side, side_bb)) in [(BLACK, board.black), (WHITE, board.white)].into_iter().enumerate() {
                for (t, &type_bb) in types.iter().enumerate() {
                    let piece = side + PAWN * (t as u8 + 1);
                    let new = side_bb & type_bb;
                    let old = entry.pieces[c][t];
                    let mut added = new & !old;
                    let mut removed = old & !new;
                    while added != 0 {
                        add_weights(&mut entry.accu, weights(feature_offset(color, bucket, piece, lsb(added))));
                        added = clear_lsb(added);
                    }
                    while removed != 0 {
                        sub_weights(&mut entry.accu, weights(feature_offset(color, bucket, piece, lsb(removed))));
                        removed = clear_lsb(removed);
                    }
                    entry.pieces[c][t] = new;
                }
            }
            accu.0.copy_from_slice(&entry.accu.0);
        });
    }

    /// The network's output for `board` (which must be the position these accumulators belong to),
    /// from the side to move's perspective.
    #[inline(always)]
    pub fn score(&self, board: &Board) -> i16 {
        let score = self.score.get();
        if score != NO_SCORE {
            return score as i16;
        }
        let score = self.compute_score(board);
        self.score.set(score as i32);
        score
    }

    #[inline(never)]
    fn compute_score(&self, board: &Board) -> i16 {
        let piece_count = pop_count(board.white | board.black);
        let output_bucket = network::material_bucket(piece_count);
        self.evaluate(board.side_to_move, output_bucket) as i16
    }

    #[inline(always)]
    fn evaluate(&self, stm: Color, output_bucket: usize) -> i32 {
        let (us, them) = if stm == Color::Black { (&self.black, &self.white) } else { (&self.white, &self.black) };
        let net = network();
        let offset = output_bucket * 2 * LAYER1_SIZE;
        let w_us: &[i16; LAYER1_SIZE] = net.output_weights[offset..offset + LAYER1_SIZE].try_into().unwrap();
        let w_them: &[i16; LAYER1_SIZE] =
            net.output_weights[offset + LAYER1_SIZE..offset + 2 * LAYER1_SIZE].try_into().unwrap();
        let mut output = screlu_dot(us, w_us).wrapping_add(screlu_dot(them, w_them));
        // during SCReLU values end up multiplied with QA * QA * QB but OutputBias is quantized by only QA * QB
        output /= QA;
        output = output.wrapping_add(net.output_biases[output_bucket] as i32);
        output.wrapping_mul(SCALE) / (QA * QB)
    }
}

/// A cached accumulator and the pieces (by color and type) it was built from.
struct RefreshEntry {
    accu: Accumulator,
    pieces: [[u64; 6]; 2],
}

thread_local! {
    /// One entry per perspective, king bucket and mirroring; each starts as the empty board (just the biases).
    static REFRESH_CACHE: RefCell<Box<[RefreshEntry]>> = RefCell::new(
        (0..2 * INPUT_BUCKETS * 2)
            .map(|_| RefreshEntry { accu: Accumulator(network().feature_biases), pieces: [[0; 6]; 2] })
            .collect(),
    );
}

#[inline(always)]
fn apply_delta(dst: &mut Accumulator, src: &Accumulator, d: &Delta) {
    let w = |o: usize| weights(o);
    match (d.adds, d.subs) {
        (0, 0) => dst.0.copy_from_slice(&src.0),
        (1, 1) => simd::add_sub(&mut dst.0, &src.0, w(d.add[0]), w(d.sub[0])),
        (1, 2) => simd::add_sub_sub(&mut dst.0, &src.0, w(d.add[0]), w(d.sub[0]), w(d.sub[1])),
        (2, 2) => simd::add_add_sub_sub(&mut dst.0, &src.0, w(d.add[0]), w(d.add[1]), w(d.sub[0]), w(d.sub[1])),
        _ => {
            // Not reachable for legal moves, but handled generically for robustness.
            dst.0.copy_from_slice(&src.0);
            for &o in &d.add[..d.adds] {
                add_weights(dst, w(o));
            }
            for &o in &d.sub[..d.subs] {
                sub_weights(dst, w(o));
            }
        }
    }
}

#[inline(always)]
fn add_weights(accu: &mut Accumulator, w: &[i16; LAYER1_SIZE]) {
    for (a, &b) in accu.0.iter_mut().zip(w.iter()) {
        *a = a.wrapping_add(b);
    }
}

#[inline(always)]
fn sub_weights(accu: &mut Accumulator, w: &[i16; LAYER1_SIZE]) {
    for (a, &b) in accu.0.iter_mut().zip(w.iter()) {
        *a = a.wrapping_sub(b);
    }
}

#[inline(always)]
fn screlu_dot(accu: &Accumulator, w: &[i16; LAYER1_SIZE]) -> i32 {
    simd::screlu_dot(&accu.0, w)
}

/// Scalar reference implementation of the SCReLU dot product (exact, wrapping i32 sum).
pub fn screlu_dot_scalar(accu: &[i16; LAYER1_SIZE], w: &[i16; LAYER1_SIZE]) -> i32 {
    let mut sum = 0i32;
    for i in 0..LAYER1_SIZE {
        let a = (accu[i] as i32).clamp(0, QA);
        sum = sum.wrapping_add((a * a).wrapping_mul(w[i] as i32));
    }
    sum
}

#[cfg(target_arch = "aarch64")]
mod simd {
    use super::LAYER1_SIZE;
    use core::arch::aarch64::*;

    const LANES: usize = 8;

    #[inline(always)]
    pub fn add_sub(dst: &mut [i16; LAYER1_SIZE], src: &[i16; LAYER1_SIZE], a: &[i16; LAYER1_SIZE], s: &[i16; LAYER1_SIZE]) {
        // SAFETY: NEON is mandatory on aarch64; all arrays hold LAYER1_SIZE (a multiple of 8) elements.
        unsafe {
            for i in (0..LAYER1_SIZE).step_by(LANES) {
                let v = vld1q_s16(src.as_ptr().add(i));
                let v = vaddq_s16(v, vld1q_s16(a.as_ptr().add(i)));
                let v = vsubq_s16(v, vld1q_s16(s.as_ptr().add(i)));
                vst1q_s16(dst.as_mut_ptr().add(i), v);
            }
        }
    }

    #[inline(always)]
    pub fn add_sub_sub(
        dst: &mut [i16; LAYER1_SIZE],
        src: &[i16; LAYER1_SIZE],
        a: &[i16; LAYER1_SIZE],
        s1: &[i16; LAYER1_SIZE],
        s2: &[i16; LAYER1_SIZE],
    ) {
        // SAFETY: see add_sub
        unsafe {
            for i in (0..LAYER1_SIZE).step_by(LANES) {
                let v = vld1q_s16(src.as_ptr().add(i));
                let v = vaddq_s16(v, vld1q_s16(a.as_ptr().add(i)));
                let v = vsubq_s16(v, vld1q_s16(s1.as_ptr().add(i)));
                let v = vsubq_s16(v, vld1q_s16(s2.as_ptr().add(i)));
                vst1q_s16(dst.as_mut_ptr().add(i), v);
            }
        }
    }

    #[inline(always)]
    pub fn add_add_sub_sub(
        dst: &mut [i16; LAYER1_SIZE],
        src: &[i16; LAYER1_SIZE],
        a1: &[i16; LAYER1_SIZE],
        a2: &[i16; LAYER1_SIZE],
        s1: &[i16; LAYER1_SIZE],
        s2: &[i16; LAYER1_SIZE],
    ) {
        // SAFETY: see add_sub
        unsafe {
            for i in (0..LAYER1_SIZE).step_by(LANES) {
                let v = vld1q_s16(src.as_ptr().add(i));
                let v = vaddq_s16(v, vld1q_s16(a1.as_ptr().add(i)));
                let v = vaddq_s16(v, vld1q_s16(a2.as_ptr().add(i)));
                let v = vsubq_s16(v, vld1q_s16(s1.as_ptr().add(i)));
                let v = vsubq_s16(v, vld1q_s16(s2.as_ptr().add(i)));
                vst1q_s16(dst.as_mut_ptr().add(i), v);
            }
        }
    }

    /// sum(clamp(a, 0, 255)^2 * w), computed as ((a * w) as i16) * a like Leorik's AVX2 path.
    /// This is exact as long as |w| <= 128, which holds for the embedded network (checked by a test).
    #[inline(always)]
    pub fn screlu_dot(accu: &[i16; LAYER1_SIZE], w: &[i16; LAYER1_SIZE]) -> i32 {
        // SAFETY: see add_sub
        unsafe {
            let zero = vdupq_n_s16(0);
            let qa = vdupq_n_s16(super::QA as i16);
            let mut sum0 = vdupq_n_s32(0);
            let mut sum1 = vdupq_n_s32(0);
            for i in (0..LAYER1_SIZE).step_by(LANES) {
                let a = vminq_s16(vmaxq_s16(vld1q_s16(accu.as_ptr().add(i)), zero), qa);
                let aw = vmulq_s16(a, vld1q_s16(w.as_ptr().add(i)));
                sum0 = vmlal_s16(sum0, vget_low_s16(aw), vget_low_s16(a));
                sum1 = vmlal_high_s16(sum1, aw, a);
            }
            vaddvq_s32(vaddq_s32(sum0, sum1))
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
mod simd {
    use super::LAYER1_SIZE;

    #[inline(always)]
    pub fn add_sub(dst: &mut [i16; LAYER1_SIZE], src: &[i16; LAYER1_SIZE], a: &[i16; LAYER1_SIZE], s: &[i16; LAYER1_SIZE]) {
        for i in 0..LAYER1_SIZE {
            dst[i] = src[i].wrapping_add(a[i]).wrapping_sub(s[i]);
        }
    }

    #[inline(always)]
    pub fn add_sub_sub(
        dst: &mut [i16; LAYER1_SIZE],
        src: &[i16; LAYER1_SIZE],
        a: &[i16; LAYER1_SIZE],
        s1: &[i16; LAYER1_SIZE],
        s2: &[i16; LAYER1_SIZE],
    ) {
        for i in 0..LAYER1_SIZE {
            dst[i] = src[i].wrapping_add(a[i]).wrapping_sub(s1[i]).wrapping_sub(s2[i]);
        }
    }

    #[inline(always)]
    pub fn add_add_sub_sub(
        dst: &mut [i16; LAYER1_SIZE],
        src: &[i16; LAYER1_SIZE],
        a1: &[i16; LAYER1_SIZE],
        a2: &[i16; LAYER1_SIZE],
        s1: &[i16; LAYER1_SIZE],
        s2: &[i16; LAYER1_SIZE],
    ) {
        for i in 0..LAYER1_SIZE {
            dst[i] = src[i].wrapping_add(a1[i]).wrapping_add(a2[i]).wrapping_sub(s1[i]).wrapping_sub(s2[i]);
        }
    }

    #[inline(always)]
    pub fn screlu_dot(accu: &[i16; LAYER1_SIZE], w: &[i16; LAYER1_SIZE]) -> i32 {
        super::screlu_dot_scalar(accu, w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_weights_fit_the_simd_trick() {
        // ((a * w) as i16) * a == a * a * w requires |w| <= 128 for a in [0, 255].
        assert!(network().output_weights.iter().all(|&w| (-128..=128).contains(&w)));
    }

    #[test]
    fn simd_dot_matches_scalar() {
        let mut x: u32 = 12345;
        let mut accu = [0i16; LAYER1_SIZE];
        for round in 0..200 {
            for a in accu.iter_mut() {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                *a = ((x >> 16) as i16 % 400) - 60;
            }
            let bucket = round % 8;
            let w: &[i16; LAYER1_SIZE] =
                network().output_weights[bucket * 2 * LAYER1_SIZE..][..LAYER1_SIZE].try_into().unwrap();
            assert_eq!(simd::screlu_dot(&accu, w), screlu_dot_scalar(&accu, w));
        }
    }
}
