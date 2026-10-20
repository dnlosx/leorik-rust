//! Score constants and helpers (port of Leorik's `Evaluation`).

pub mod see;

use crate::bitboard::pop_count;
use crate::board::Board;
use crate::piece::Color;

/// Internal score units per pawn; UCI centipawns are `score * 100 / 306`.
pub const NORMALIZE_TO_PAWN_VALUE: i32 = 306;

pub const CHECKMATE_BASE: i32 = 9000;
pub const CHECKMATE_SCORE: i32 = 9999;
pub const SCORE_BOUNDS: i32 = 10000;

/// Number of full moves until mate for a mate score.
#[inline(always)]
pub fn mate_distance(score: i32) -> i32 {
    let plies = CHECKMATE_SCORE - score.abs();
    (plies + 1) / 2
}

#[inline(always)]
pub fn is_checkmate(score: i32) -> bool {
    score.abs() > CHECKMATE_BASE
}

#[inline(always)]
pub fn checkmate(color: Color, ply: i32) -> i32 {
    color.sign() * (ply - CHECKMATE_SCORE)
}

#[inline(always)]
pub fn mated_score(ply: i32) -> i32 {
    ply - CHECKMATE_SCORE
}

#[inline(always)]
pub fn mate_score(ply: i32) -> i32 {
    CHECKMATE_SCORE - ply
}

/// https://support.chess.com/article/128-what-does-insufficient-mating-material-mean
#[inline(always)]
pub fn is_insufficient_mating_material(board: &Board) -> bool {
    if pop_count(board.black | board.white) > 4 {
        return false;
    }
    let black = board.black & !board.kings;
    let white = board.white & !board.kings;

    // lone king vs two knights
    if white == 0 && (board.knights & black) == black && pop_count(black) == 2 {
        return true;
    }
    if black == 0 && (board.knights & white) == white && pop_count(white) == 2 {
        return true;
    }

    // if both sides have any one of: lone king, king and bishop, king and knight (and no pawns)
    let norb = board.knights | board.bishops;
    (black == 0 || ((norb & black) == black && pop_count(black) == 1))
        && (white == 0 || ((norb & white) == white && pop_count(white) == 1))
}
