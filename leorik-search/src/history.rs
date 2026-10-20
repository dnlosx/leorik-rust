//! Move history, killers, continuations, correction history and search statistics
//! (port of Leorik's `History`).

use leorik_core::eval::{CHECKMATE_BASE, NORMALIZE_TO_PAWN_VALUE};
use leorik_core::piece::{Color, COLOR_MASK, NONE};
use leorik_core::{Board, Move};

const MAX_PLY: usize = 99;
const SQUARES: usize = 64;
const PIECES: usize = 14; // including colored 'none'
pub const CONT_DEPTH: usize = 4;
const CORR_TABLE: u64 = 19997; // prime!

#[derive(Copy, Clone, Default)]
struct CorrEntry {
    numerator: i64,
    denominator: i64,
}

impl CorrEntry {
    #[inline(always)]
    fn add(&mut self, corr: i64, inc: i64) {
        self.numerator += corr;
        self.denominator += inc;
    }

    #[inline(always)]
    fn get(&self) -> i32 {
        (self.numerator / (self.denominator + 100)) as i32
    }
}

/// Welford's online mean / variance.
#[derive(Copy, Clone)]
struct Accumulator {
    n: i32,
    mean: f64,
    m2: f64,
}

impl Accumulator {
    fn new(std_dev: f64, virtual_samples: i32) -> Accumulator {
        let n = virtual_samples.max(2);
        Accumulator { n, mean: 0.0, m2: std_dev * std_dev * n as f64 }
    }

    #[inline(always)]
    fn std_dev(&self) -> f64 {
        (self.m2 / self.n as f64).sqrt()
    }

    #[inline(always)]
    fn push(&mut self, x: f64) {
        self.n += 1;
        let delta = x - self.mean;
        self.mean += delta / self.n as f64;
        let delta2 = x - self.mean;
        self.m2 += delta * delta2;
    }
}

pub struct History {
    total_positive: u64,
    total_played: u64,
    null_move_passes_sum: i64,
    null_move_passes_count: i64,
    positive: Box<[[u64; SQUARES]; SQUARES]>,
    all: Box<[[u64; SQUARES]; SQUARES]>,
    moves: [Move; MAX_PLY],
    killers: [Move; MAX_PLY],
    continuation: Box<[[[Move; PIECES]; SQUARES]; CONT_DEPTH]>,
    corrections: Vec<CorrEntry>,
    black: Accumulator,
    white: Accumulator,
}

impl Default for History {
    fn default() -> Self {
        History::new()
    }
}

#[inline(always)]
fn piece_index(mv: Move) -> usize {
    // WhiteCastling = 0, BlackCastling = 1, BlackPawn = 2, WhitePawn = 3 ... BlackKing = 12, WhiteKing = 13
    if mv.is_castling() {
        ((mv.flags & COLOR_MASK) >> 1) as usize
    } else {
        (mv.moving_piece() >> 1) as usize
    }
}

impl History {
    pub fn new() -> History {
        History {
            total_positive: 0,
            total_played: 0,
            null_move_passes_sum: 0,
            null_move_passes_count: 1,
            positive: Box::new([[0; SQUARES]; SQUARES]),
            all: Box::new([[0; SQUARES]; SQUARES]),
            moves: [Move::NONE; MAX_PLY],
            killers: [Move::NONE; MAX_PLY],
            continuation: Box::new([[[Move::NONE; PIECES]; SQUARES]; CONT_DEPTH]),
            corrections: vec![CorrEntry::default(); 8 * CORR_TABLE as usize],
            black: Accumulator::new(NORMALIZE_TO_PAWN_VALUE as f64, 20),
            white: Accumulator::new(NORMALIZE_TO_PAWN_VALUE as f64, 20),
        }
    }

    #[inline(always)]
    pub fn good(&mut self, ply: usize, depth: i32, mv: Move) {
        // no killer, followup, counter tracking for captures
        if mv.captured_piece() != NONE {
            return;
        }
        let inc = (depth * depth) as u64;
        self.total_positive = self.total_positive.wrapping_add(inc);
        self.positive[mv.to as usize][mv.from as usize] = self.positive[mv.to as usize][mv.from as usize].wrapping_add(inc);
        self.killers[ply] = mv;

        for i in 0..ply.min(CONT_DEPTH) {
            let prev = self.moves[ply - i - 1];
            self.continuation[i][prev.to as usize][piece_index(prev)] = mv;
        }
    }

    #[inline(always)]
    pub fn played(&mut self, ply: usize, depth: i32, mv: Move) {
        self.moves[ply] = mv;
        if mv.captured_piece() != NONE {
            return;
        }
        let inc = (depth * depth) as u64;
        self.total_played = self.total_played.wrapping_add(inc);
        self.all[mv.to as usize][mv.from as usize] = self.all[mv.to as usize][mv.from as usize].wrapping_add(inc);
    }

    /// local-ratio / average-ratio, computed in single precision exactly like the original.
    #[inline(always)]
    pub fn value(&self, mv: Move) -> f32 {
        let a = self.positive[mv.to as usize][mv.from as usize] as f32;
        let b = self.all[mv.to as usize][mv.from as usize] as f32;
        self.total_played as f32 * a / (b * self.total_positive as f32 + 1.0)
    }

    #[inline(always)]
    pub fn killer(&self, ply: usize) -> Move {
        self.killers[ply]
    }

    #[inline(always)]
    pub fn continuation(&self, ply: usize, i: usize) -> Move {
        if i >= ply {
            return Move::NONE;
        }
        let prev = self.moves[ply - i - 1];
        self.continuation[i][prev.to as usize][piece_index(prev)]
    }

    #[inline(always)]
    pub fn null_move_pass(&mut self, eval: i32, beta: i32) {
        self.null_move_passes_count += 1;
        self.null_move_passes_sum += (eval - beta) as i64;
    }

    #[inline(always)]
    pub fn is_expected_fail_high(&self, eval: i32, beta: i32) -> bool {
        let avg_null_move_pass = (self.null_move_passes_sum / self.null_move_passes_count) as i32;
        eval > beta + avg_null_move_pass
    }

    #[inline(always)]
    pub fn adjusted_static_eval(&self, board: &Board) -> i32 {
        let stm = if board.side_to_move == Color::Black { 1 } else { 0 };
        let eval = board.side_to_move_score();
        let corr = self.correction(board, stm);
        const RANGE: i32 = CHECKMATE_BASE - 1;
        (eval + corr).clamp(-RANGE, RANGE)
    }

    #[inline(always)]
    fn correction(&self, board: &Board, stm: usize) -> i32 {
        let mut result = self.get_correction(stm, board.pawns);
        result += self.get_correction(stm + 2, board.knights | board.bishops);
        result += self.get_correction(stm + 4, board.queens | board.rooks);
        result += self.get_correction(stm + 6, board.kings);
        result
    }

    #[inline(always)]
    pub fn update_correction(&mut self, board: &Board, depth: i32, delta: i32) {
        let inc = (depth * depth) as i64;
        let corr = inc * delta.clamp(-100, 100) as i64;
        let stm = if board.side_to_move == Color::Black { 1 } else { 0 };
        self.add_correction(stm, corr, inc, board.pawns);
        self.add_correction(stm + 2, corr, inc, board.knights | board.bishops);
        self.add_correction(stm + 4, corr, inc, board.queens | board.rooks);
        self.add_correction(stm + 6, corr, inc, board.kings);
    }

    #[inline(always)]
    fn add_correction(&mut self, offset: usize, corr: i64, inc: i64, bits: u64) {
        let index = (bits % CORR_TABLE) as usize + offset * CORR_TABLE as usize;
        self.corrections[index].add(corr, inc);
    }

    #[inline(always)]
    fn get_correction(&self, offset: usize, bits: u64) -> i32 {
        let index = (bits % CORR_TABLE) as usize + offset * CORR_TABLE as usize;
        self.corrections[index].get()
    }

    #[inline(always)]
    pub fn estimate_quiet_improvement_upper_bound(&self, side_to_move: Color, std_devs: f32) -> i32 {
        let acc = if side_to_move == Color::White { &self.white } else { &self.black };
        (acc.mean + std_devs as f64 * acc.std_dev()) as i32
    }

    #[inline(always)]
    pub fn record_quiet_improvement(&mut self, side_to_move: Color, improvement: i32) {
        if side_to_move == Color::White {
            self.white.push(improvement as f64);
        } else {
            self.black.push(improvement as f64);
        }
    }
}
