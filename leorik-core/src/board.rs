//! Board state with copy-make move execution (port of Leorik's `BoardState`).

use crate::bitboard::*;
use crate::moves::Move;
use crate::nnue::NeuralNetEval;
use crate::piece::*;
use crate::zobrist;

#[derive(Clone)]
#[repr(C)]
pub struct Board {
    pub white: u64,
    pub black: u64,
    pub pawns: u64,
    pub knights: u64,
    pub bishops: u64,
    pub rooks: u64,
    pub queens: u64,
    pub kings: u64,
    pub castle_flags: u64,
    pub en_passant: u64,

    pub zobrist_hash: u64,
    pub side_to_move: Color,
    pub halfmove_clock: u8,
    pub eval: NeuralNetEval,
}

impl Default for Board {
    fn default() -> Self {
        Board::new()
    }
}

impl Board {
    /// An empty board (no pieces). Only useful as a buffer for copy-make.
    pub fn new() -> Board {
        Board {
            white: 0,
            black: 0,
            pawns: 0,
            knights: 0,
            bishops: 0,
            rooks: 0,
            queens: 0,
            kings: 0,
            castle_flags: 0,
            en_passant: 0,
            zobrist_hash: 0,
            side_to_move: Color::White,
            halfmove_clock: 0,
            eval: NeuralNetEval::new(),
        }
    }

    #[inline(always)]
    pub fn set_bit(&mut self, square: usize, piece: Piece) {
        let bb = 1u64 << square;
        match piece & COLOR_MASK {
            BLACK => self.black |= bb,
            WHITE => self.white |= bb,
            _ => {}
        }
        match piece & TYPE_MASK {
            PAWN => self.pawns |= bb,
            KNIGHT => self.knights |= bb,
            BISHOP => self.bishops |= bb,
            ROOK => self.rooks |= bb,
            QUEEN => self.queens |= bb,
            KING => self.kings |= bb,
            _ => {}
        }
    }

    #[inline(always)]
    pub fn copy_from(&mut self, other: &Board) {
        self.copy_unmasked(other);
        self.en_passant = other.en_passant;
        self.zobrist_hash = other.zobrist_hash;
        self.side_to_move = other.side_to_move;
        self.eval.copy_from(&other.eval);
        self.halfmove_clock = other.halfmove_clock;
    }

    /// Replaces each piece by the same piece of the opposite color, mirrored to the other side of the board.
    pub fn flip(&mut self) {
        let (w, b) = (byte_swap(self.black), byte_swap(self.white));
        self.white = w;
        self.black = b;
        self.pawns = byte_swap(self.pawns);
        self.knights = byte_swap(self.knights);
        self.bishops = byte_swap(self.bishops);
        self.rooks = byte_swap(self.rooks);
        self.queens = byte_swap(self.queens);
        self.kings = byte_swap(self.kings);
        self.en_passant = byte_swap(self.en_passant);
        self.castle_flags = byte_swap(self.castle_flags);
        self.side_to_move = self.side_to_move.flip();
        self.update_eval();
        self.update_hash();
    }

    /// Recomputes the NNUE accumulators from scratch.
    #[inline]
    pub fn update_eval(&mut self) {
        let view = self.view();
        self.eval.update(&view);
    }

    /// NNUE score from the side to move's perspective (internal units).
    #[inline(always)]
    pub fn side_to_move_score(&self) -> i32 {
        self.eval.score as i32
    }

    /// NNUE score from White's perspective (internal units).
    #[inline(always)]
    pub fn score(&self) -> i32 {
        self.side_to_move.sign() * self.eval.score as i32
    }

    /// Plays the move and updates the evaluation but not the hash or halfmove clock.
    /// Returns false if the move leaves the own king in check.
    #[inline(always)]
    pub fn quick_play(&mut self, from: &Board, mv: Move) -> bool {
        if from.side_to_move == Color::White {
            self.play_white(from, mv);
            if self.is_attacked_by_black(lsb(self.kings & self.white)) {
                return false;
            }
        } else {
            self.play_black(from, mv);
            if self.is_attacked_by_white(lsb(self.kings & self.black)) {
                return false;
            }
        }
        let view = self.view();
        self.eval.update_incremental(&from.eval, mv, &view);
        true
    }

    /// Plays the move, updating evaluation, hash and halfmove clock.
    /// Returns false if the move leaves the own king in check.
    #[inline(always)]
    pub fn play(&mut self, from: &Board, mv: Move) -> bool {
        if from.side_to_move == Color::White {
            self.play_white(from, mv);
            if self.is_attacked_by_black(lsb(self.kings & self.white)) {
                return false;
            }
        } else {
            self.play_black(from, mv);
            if self.is_attacked_by_white(lsb(self.kings & self.black)) {
                return false;
            }
        }
        let view = self.view();
        self.eval.update_incremental(&from.eval, mv, &view);
        self.update_hash_incremental(from, mv);
        self.update_halfmove_clock(from, mv);
        true
    }

    /// Plays the move in place. Returns false (and leaves the board in an undefined state) if it's illegal.
    pub fn play_move(&mut self, mv: Move) -> bool {
        let from = self.clone();
        self.play(&from, mv)
    }

    #[inline(always)]
    pub fn play_null_move(&mut self, from: &Board) {
        self.copy_unmasked(from);
        self.halfmove_clock = from.halfmove_clock.wrapping_add(1);
        self.side_to_move = from.side_to_move.flip();
        self.en_passant = 0;
        let view = self.view();
        self.eval.update_incremental(&from.eval, Move::NONE, &view);
        self.zobrist_hash = from.zobrist_hash;

        self.zobrist_hash ^= zobrist::side_to_move();
        if from.en_passant > 0 {
            self.zobrist_hash ^= zobrist::flags(lsb(from.en_passant));
        }
    }

    #[inline(always)]
    pub fn play_without_hash_and_eval(&mut self, from: &Board, mv: Move) -> bool {
        if from.side_to_move == Color::White {
            self.play_white(from, mv);
            !self.is_attacked_by_black(lsb(self.kings & self.white))
        } else {
            self.play_black(from, mv);
            !self.is_attacked_by_white(lsb(self.kings & self.black))
        }
    }

    #[inline(always)]
    fn play_black(&mut self, from: &Board, mv: Move) {
        let bb_to = 1u64 << mv.to;
        let bb_from = 1u64 << mv.from;
        self.copy_bitboards(from, from.white & bb_to);
        self.en_passant = 0;
        self.side_to_move = Color::White;
        self.black ^= bb_from | bb_to;

        match mv.flags & !COLOR_MASK {
            PAWN => {
                self.pawns ^= bb_from | bb_to;
                self.en_passant = (bb_to << 8) & (bb_from >> 8);
            }
            QUEEN_PROMOTION => {
                self.pawns &= !bb_from;
                self.queens |= bb_to;
            }
            ROOK_PROMOTION => {
                self.pawns &= !bb_from;
                self.rooks |= bb_to;
            }
            BISHOP_PROMOTION => {
                self.pawns &= !bb_from;
                self.bishops |= bb_to;
            }
            KNIGHT_PROMOTION => {
                self.pawns &= !bb_from;
                self.knights |= bb_to;
            }
            EN_PASSANT => {
                self.pawns ^= bb_from | bb_to | bb_to << 8;
                self.white &= !(bb_to << 8);
            }
            KNIGHT => self.knights ^= bb_from | bb_to,
            BISHOP => self.bishops ^= bb_from | bb_to,
            ROOK => {
                self.rooks ^= bb_from | bb_to;
                self.castle_flags &= !bb_from;
            }
            QUEEN => self.queens ^= bb_from | bb_to,
            KING => {
                self.kings ^= bb_from | bb_to;
                self.castle_flags &= 0x0000_0000_0000_00FF;
            }
            CASTLE_SHORT => {
                self.kings ^= bb_from ^ 0x4000_0000_0000_0000;
                self.rooks ^= bb_to ^ 0x2000_0000_0000_0000;
                self.black |= 0x6000_0000_0000_0000;
                self.castle_flags &= 0x0000_0000_0000_00FF;
            }
            CASTLE_LONG => {
                self.kings ^= bb_from ^ 0x0400_0000_0000_0000;
                self.rooks ^= bb_to ^ 0x0800_0000_0000_0000;
                self.black |= 0x0C00_0000_0000_0000;
                self.castle_flags &= 0x0000_0000_0000_00FF;
            }
            _ => {}
        }
    }

    #[inline(always)]
    fn play_white(&mut self, from: &Board, mv: Move) {
        let bb_to = 1u64 << mv.to;
        let bb_from = 1u64 << mv.from;
        self.copy_bitboards(from, from.black & bb_to);
        self.en_passant = 0;
        self.side_to_move = Color::Black;
        self.white ^= bb_from | bb_to;

        match mv.flags & !COLOR_MASK {
            PAWN => {
                self.pawns ^= bb_from | bb_to;
                self.en_passant = (bb_to >> 8) & (bb_from << 8);
            }
            QUEEN_PROMOTION => {
                self.pawns &= !bb_from;
                self.queens |= bb_to;
            }
            ROOK_PROMOTION => {
                self.pawns &= !bb_from;
                self.rooks |= bb_to;
            }
            BISHOP_PROMOTION => {
                self.pawns &= !bb_from;
                self.bishops |= bb_to;
            }
            KNIGHT_PROMOTION => {
                self.pawns &= !bb_from;
                self.knights |= bb_to;
            }
            EN_PASSANT => {
                self.pawns ^= bb_from | bb_to | bb_to >> 8;
                self.black &= !(bb_to >> 8);
            }
            KNIGHT => self.knights ^= bb_from | bb_to,
            BISHOP => self.bishops ^= bb_from | bb_to,
            ROOK => {
                self.rooks ^= bb_from | bb_to;
                self.castle_flags &= !bb_from;
            }
            QUEEN => self.queens ^= bb_from | bb_to,
            KING => {
                self.kings ^= bb_from | bb_to;
                self.castle_flags &= 0xFF00_0000_0000_0000;
            }
            CASTLE_SHORT => {
                self.kings ^= bb_from ^ 0x40;
                self.rooks ^= bb_to ^ 0x20;
                self.white |= 0x60;
                self.castle_flags &= 0xFF00_0000_0000_0000;
            }
            CASTLE_LONG => {
                self.kings ^= bb_from ^ 0x4;
                self.rooks ^= bb_to ^ 0x8;
                self.white |= 0xC;
                self.castle_flags &= 0xFF00_0000_0000_0000;
            }
            _ => {}
        }
    }

    /// True if the side to move only has king and pawns.
    #[inline(always)]
    pub fn is_endgame(&self) -> bool {
        if self.side_to_move == Color::White {
            self.white == (self.white & (self.kings | self.pawns))
        } else {
            self.black == (self.black & (self.kings | self.pawns))
        }
    }

    #[inline(always)]
    fn copy_bitboards(&mut self, from: &Board, mask: u64) {
        if mask == 0 {
            self.copy_unmasked(from);
        } else {
            self.copy_masked(from, !mask);
        }
    }

    #[inline(always)]
    fn copy_unmasked(&mut self, from: &Board) {
        self.white = from.white;
        self.black = from.black;
        self.pawns = from.pawns;
        self.knights = from.knights;
        self.bishops = from.bishops;
        self.rooks = from.rooks;
        self.queens = from.queens;
        self.kings = from.kings;
        self.castle_flags = from.castle_flags;
    }

    #[inline(always)]
    fn copy_masked(&mut self, from: &Board, mask: u64) {
        self.white = from.white & mask;
        self.black = from.black & mask;
        self.pawns = from.pawns & mask;
        self.knights = from.knights & mask;
        self.bishops = from.bishops & mask;
        self.rooks = from.rooks & mask;
        self.queens = from.queens & mask;
        self.kings = from.kings & mask;
        self.castle_flags = from.castle_flags & mask;
    }

    #[inline(always)]
    pub fn in_check(&self) -> bool {
        self.in_check_color(self.side_to_move)
    }

    #[inline(always)]
    pub fn in_check_color(&self, color: Color) -> bool {
        if color == Color::White {
            self.is_attacked_by_black(lsb(self.kings & self.white))
        } else {
            self.is_attacked_by_white(lsb(self.kings & self.black))
        }
    }

    #[inline(always)]
    pub fn is_attacked_by_white(&self, square: usize) -> bool {
        let occupied = self.black | self.white;
        let mut pieces = self.white & self.knights;
        if pieces > 0 && (pieces & KNIGHT_TARGETS[square]) > 0 {
            return true;
        }
        pieces = self.white & self.kings;
        if pieces > 0 && (pieces & KING_TARGETS[square]) > 0 {
            return true;
        }
        pieces = self.white & (self.queens | self.bishops);
        if pieces > 0 && (pieces & DIAGONAL_MASK[square]) > 0 && (pieces & bishop_targets(occupied, square)) > 0 {
            return true;
        }
        pieces = self.white & (self.queens | self.rooks);
        if pieces > 0 && (pieces & ORTHOGONAL_MASK[square]) > 0 && (pieces & rook_targets(occupied, square)) > 0 {
            return true;
        }
        // Warning: pawn attacks do not consider en passant!
        pieces = self.white & self.pawns;
        let left = (pieces & 0xFEFE_FEFE_FEFE_FEFE) << 7;
        let right = (pieces & 0x7F7F_7F7F_7F7F_7F7F) << 9;
        ((left | right) & (1u64 << square)) > 0
    }

    #[inline(always)]
    pub fn is_attacked_by_black(&self, square: usize) -> bool {
        let occupied = self.black | self.white;
        let mut pieces = self.black & self.knights;
        if pieces > 0 && (pieces & KNIGHT_TARGETS[square]) > 0 {
            return true;
        }
        pieces = self.black & self.kings;
        if pieces > 0 && (pieces & KING_TARGETS[square]) > 0 {
            return true;
        }
        pieces = self.black & (self.queens | self.bishops);
        if pieces > 0 && (pieces & DIAGONAL_MASK[square]) > 0 && (pieces & bishop_targets(occupied, square)) > 0 {
            return true;
        }
        pieces = self.black & (self.queens | self.rooks);
        if pieces > 0 && (pieces & ORTHOGONAL_MASK[square]) > 0 && (pieces & rook_targets(occupied, square)) > 0 {
            return true;
        }
        pieces = self.black & self.pawns;
        let left = (pieces & 0xFEFE_FEFE_FEFE_FEFE) >> 9;
        let right = (pieces & 0x7F7F_7F7F_7F7F_7F7F) >> 7;
        ((left | right) & (1u64 << square)) > 0
    }

    #[inline(always)]
    pub fn get_piece(&self, square: usize) -> Piece {
        get_piece(self.white, self.black, self.pawns, self.knights, self.bishops, self.rooks, self.queens, self.kings, square)
    }

    /// A copy of the piece placement, used by the NNUE to update itself without borrowing the board.
    #[inline(always)]
    pub fn view(&self) -> PieceView {
        PieceView {
            white: self.white,
            black: self.black,
            pawns: self.pawns,
            knights: self.knights,
            bishops: self.bishops,
            rooks: self.rooks,
            queens: self.queens,
            kings: self.kings,
            side_to_move: self.side_to_move,
        }
    }

    /// Recomputes the Zobrist hash from scratch.
    pub fn update_hash(&mut self) {
        self.zobrist_hash = if self.side_to_move == Color::Black { zobrist::side_to_move() } else { 0 };
        let mut bits = self.white | self.black;
        while bits != 0 {
            let square = lsb(bits);
            self.zobrist_hash ^= zobrist::piece_square(self.get_piece(square), square);
            bits = clear_lsb(bits);
        }
        let mut bits = self.castle_flags | self.en_passant;
        while bits != 0 {
            self.zobrist_hash ^= zobrist::flags(lsb(bits));
            bits = clear_lsb(bits);
        }
    }

    #[inline(always)]
    fn update_hash_incremental(&mut self, from: &Board, mv: Move) {
        let mut hash = from.zobrist_hash;
        hash ^= zobrist::side_to_move();
        hash ^= zobrist::piece_square(mv.moving_piece(), mv.from as usize);
        hash ^= zobrist::piece_square(mv.target, mv.to as usize);
        hash ^= zobrist::piece_square(mv.new_piece(), mv.to as usize);

        const BLACK_EP: Piece = EN_PASSANT | BLACK_PAWN;
        const WHITE_EP: Piece = EN_PASSANT | WHITE_PAWN;
        const BLACK_SHORT: Piece = CASTLE_SHORT | BLACK;
        const BLACK_LONG: Piece = CASTLE_LONG | BLACK;
        const WHITE_SHORT: Piece = CASTLE_SHORT | WHITE;
        const WHITE_LONG: Piece = CASTLE_LONG | WHITE;
        match mv.flags {
            BLACK_EP => hash ^= zobrist::piece_square(WHITE_PAWN, mv.to as usize + 8),
            WHITE_EP => hash ^= zobrist::piece_square(BLACK_PAWN, mv.to as usize - 8),
            BLACK_SHORT => {
                hash ^= zobrist::piece_square(BLACK_KING, 62);
                hash ^= zobrist::piece_square(BLACK_ROOK, 61);
            }
            BLACK_LONG => {
                hash ^= zobrist::piece_square(BLACK_KING, 58);
                hash ^= zobrist::piece_square(BLACK_ROOK, 59);
            }
            WHITE_SHORT => {
                hash ^= zobrist::piece_square(WHITE_KING, 6);
                hash ^= zobrist::piece_square(WHITE_ROOK, 5);
            }
            WHITE_LONG => {
                hash ^= zobrist::piece_square(WHITE_KING, 2);
                hash ^= zobrist::piece_square(WHITE_ROOK, 3);
            }
            _ => {}
        }

        let mut bits = (self.castle_flags ^ from.castle_flags) | (self.en_passant ^ from.en_passant);
        while bits != 0 {
            hash ^= zobrist::flags(lsb(bits));
            bits = clear_lsb(bits);
        }
        self.zobrist_hash = hash;
    }

    #[inline(always)]
    fn update_halfmove_clock(&mut self, from: &Board, mv: Move) {
        if mv.moving_piece_type() == PAWN || mv.captured_piece() != NONE {
            self.halfmove_clock = 0;
        } else {
            self.halfmove_clock = from.halfmove_clock.wrapping_add(1);
        }
    }
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn get_piece(
    white: u64,
    black: u64,
    pawns: u64,
    knights: u64,
    bishops: u64,
    rooks: u64,
    queens: u64,
    kings: u64,
    square: usize,
) -> Piece {
    #[inline(always)]
    fn bit(bb: u64, square: usize, shift: u32) -> u8 {
        (((bb >> square) & 1) as u8) << shift
    }
    bit(black | white, square, 0)
        | bit(white, square, 1)
        | bit(pawns | bishops | queens, square, 2)
        | bit(knights | bishops | kings, square, 3)
        | bit(kings | rooks | queens, square, 4)
}

/// Piece placement and side to move of a [`Board`].
#[derive(Copy, Clone)]
pub struct PieceView {
    pub white: u64,
    pub black: u64,
    pub pawns: u64,
    pub knights: u64,
    pub bishops: u64,
    pub rooks: u64,
    pub queens: u64,
    pub kings: u64,
    pub side_to_move: Color,
}

impl PieceView {
    #[inline(always)]
    pub fn get_piece(&self, square: usize) -> Piece {
        get_piece(self.white, self.black, self.pawns, self.knights, self.bishops, self.rooks, self.queens, self.kings, square)
    }
}
