//! Static exchange evaluation (port of Leorik's `StaticExchange`).

use crate::bitboard::*;
use crate::board::Board;
use crate::eval::CHECKMATE_SCORE;
use crate::moves::Move;
use crate::piece::*;

static PIECE_VALUES: [i32; 14] = [
    0, 0, //
    -100, 100, //
    -300, 300, //
    -300, 300, //
    -500, 500, //
    -900, 900, //
    -9999, 9999,
];

#[inline(always)]
fn piece_value(sign: i32, piece: Piece) -> i32 {
    sign * PIECE_VALUES[(piece >> 1) as usize]
}

#[derive(Default, Clone)]
pub struct StaticExchange {
    white: u64,
    black: u64,
    pawns: u64,
    knights: u64,
    bishops: u64,
    rooks: u64,
    queens: u64,
    kings: u64,
    to_bit: u64,
    flags: Piece,
    target: Piece,
    from_square: usize,
    to_square: usize,
}

impl StaticExchange {
    pub fn new() -> StaticExchange {
        StaticExchange::default()
    }

    #[inline(always)]
    fn new_piece(&self) -> Piece {
        if self.flags < KNIGHT_PROMOTION || self.flags >= CASTLE_SHORT {
            self.flags & PIECE_MASK
        } else {
            ((self.flags >> 3) & !COLOR_MASK) | (self.flags & COLOR_MASK)
        }
    }

    #[inline(always)]
    fn is_promotion(&self) -> bool {
        self.flags >= KNIGHT_PROMOTION && self.flags < CASTLE_SHORT
    }

    #[inline(always)]
    fn captured_piece_type(&self) -> Piece {
        self.target & TYPE_MASK
    }

    #[inline(always)]
    fn moving_piece(&self) -> Piece {
        self.flags & PIECE_MASK
    }

    /// True if the capture is expected to lose material.
    #[inline(always)]
    pub fn is_bad(&mut self, position: &Board, mv: Move) -> bool {
        if Move::order(mv.captured_piece()) >= Move::order(mv.moving_piece()) {
            return false;
        }
        let see = self.evaluate_window(position, mv, -1, 1);
        if position.side_to_move == Color::White {
            see < 0
        } else {
            see > 0
        }
    }

    pub fn evaluate_sign(position: &Board, mv: Move) -> i32 {
        StaticExchange::new().evaluate_window(position, mv, -1, 1).signum()
    }

    pub fn evaluate(position: &Board, mv: Move) -> i32 {
        StaticExchange::new().evaluate_window(position, mv, -CHECKMATE_SCORE, CHECKMATE_SCORE)
    }

    pub fn evaluate_window(&mut self, position: &Board, mv: Move, mut alpha: i32, mut beta: i32) -> i32 {
        if mv.flags >= CASTLE_SHORT {
            return 0;
        }
        self.flags = mv.flags;
        self.target = mv.target;
        self.from_square = mv.from as usize;
        self.to_square = mv.to as usize;
        let to = self.to_square;
        self.to_bit = 1u64 << to;
        let not_to = !self.to_bit;
        self.white = position.white & not_to;
        self.black = position.black & not_to;
        self.pawns = position.pawns & not_to;
        self.knights = position.knights & not_to & KNIGHT_TARGETS[to];
        self.bishops = position.bishops & not_to & DIAGONAL_MASK[to];
        self.rooks = position.rooks & not_to & ORTHOGONAL_MASK[to];
        self.queens = position.queens & not_to & (ORTHOGONAL_MASK[to] | DIAGONAL_MASK[to]);
        self.kings = position.kings & not_to & KING_TARGETS[to];

        let mut sign = position.side_to_move.sign();
        if sign > 0 {
            self.play_white();
        } else {
            self.play_black();
        }

        let mut see = if mv.is_en_passant() { piece_value(sign, mv.moving_piece()) } else { 0 };

        loop {
            if self.captured_piece_type() == KING {
                return sign * beta;
            }

            see -= piece_value(sign, self.target);

            if self.is_promotion() {
                see -= piece_value(sign, self.moving_piece());
                see += piece_value(sign, self.new_piece());
            }

            if see < alpha {
                return sign * alpha; // move would be too bad, stm uses stand-pat
            }

            beta = beta.min(see); // new stand-pat option for opponent

            if sign > 0 {
                if !self.play_blacks_least_valuable_attack() {
                    return beta;
                }
                self.black ^= 1u64 << self.from_square;
            } else {
                if !self.play_whites_least_valuable_attack() {
                    return -beta;
                }
                self.white ^= 1u64 << self.from_square;
            }

            // swap the side like a negamax without recursion
            sign = -sign;
            see = -see;
            let (a, b) = (-beta, -alpha);
            alpha = a;
            beta = b;
        }
    }

    #[inline(always)]
    fn play_blacks_least_valuable_attack(&mut self) -> bool {
        self.target = self.new_piece();
        let to = self.to_square;
        let occupied = self.black | self.white;

        let black_pawns = self.black & self.pawns;
        let left = (black_pawns & 0xFEFE_FEFE_FEFE_FEFE) >> 9;
        if (left & self.to_bit & 0xFFFF_FFFF_FFFF_FF00) > 0 {
            self.flags = BLACK_PAWN;
            self.from_square = to + 9;
            return true;
        }
        let right = (black_pawns & 0x7F7F_7F7F_7F7F_7F7F) >> 7;
        if (right & self.to_bit & 0xFFFF_FFFF_FFFF_FF00) > 0 {
            self.flags = BLACK_PAWN;
            self.from_square = to + 7;
            return true;
        }
        let pieces = self.black & self.knights;
        if pieces > 0 {
            self.flags = BLACK_KNIGHT;
            self.from_square = lsb(pieces);
            return true;
        }
        let mut pieces = self.black & self.bishops;
        if pieces > 0 {
            pieces &= bishop_targets(occupied, to);
            if pieces > 0 {
                self.flags = BLACK_BISHOP;
                self.from_square = lsb(pieces);
                return true;
            }
        }
        let mut pieces = self.black & self.rooks;
        if pieces > 0 {
            pieces &= rook_targets(occupied, to);
            if pieces > 0 {
                self.flags = BLACK_ROOK;
                self.from_square = lsb(pieces);
                return true;
            }
        }
        if (left & self.to_bit & 0x0000_0000_0000_00FF) > 0 {
            self.flags = BLACK_PAWN | QUEEN_PROMOTION;
            self.from_square = to + 9;
            return true;
        }
        if (right & self.to_bit & 0x0000_0000_0000_00FF) > 0 {
            self.flags = BLACK_PAWN | QUEEN_PROMOTION;
            self.from_square = to + 7;
            return true;
        }
        let mut pieces = self.black & self.queens;
        if pieces > 0 {
            pieces &= queen_targets(occupied, to);
            if pieces > 0 {
                self.flags = BLACK_QUEEN;
                self.from_square = lsb(pieces);
                return true;
            }
        }
        let pieces = self.black & self.kings;
        if pieces > 0 {
            self.flags = BLACK_KING;
            self.from_square = lsb(pieces);
            return true;
        }
        false
    }

    #[inline(always)]
    fn play_whites_least_valuable_attack(&mut self) -> bool {
        self.target = self.new_piece();
        let to = self.to_square;
        let occupied = self.black | self.white;

        let white_pawns = self.pawns & self.white;
        let left = (white_pawns & 0xFEFE_FEFE_FEFE_FEFE) << 7;
        if (left & self.to_bit & 0x00FF_FFFF_FFFF_FFFF) > 0 {
            self.flags = WHITE_PAWN;
            self.from_square = to - 7;
            return true;
        }
        let right = (white_pawns & 0x7F7F_7F7F_7F7F_7F7F) << 9;
        if (right & self.to_bit & 0x00FF_FFFF_FFFF_FFFF) > 0 {
            self.flags = WHITE_PAWN;
            self.from_square = to - 9;
            return true;
        }
        let pieces = self.white & self.knights;
        if pieces > 0 {
            self.flags = WHITE_KNIGHT;
            self.from_square = lsb(pieces);
            return true;
        }
        let mut pieces = self.white & self.bishops;
        if pieces > 0 {
            pieces &= bishop_targets(occupied, to);
            if pieces > 0 {
                self.flags = WHITE_BISHOP;
                self.from_square = lsb(pieces);
                return true;
            }
        }
        let mut pieces = self.white & self.rooks;
        if pieces > 0 {
            pieces &= rook_targets(occupied, to);
            if pieces > 0 {
                self.flags = WHITE_ROOK;
                self.from_square = lsb(pieces);
                return true;
            }
        }
        if (left & self.to_bit & 0xFF00_0000_0000_0000) > 0 {
            self.flags = WHITE_PAWN | QUEEN_PROMOTION;
            self.from_square = to - 7;
            return true;
        }
        if (right & self.to_bit & 0xFF00_0000_0000_0000) > 0 {
            self.flags = WHITE_PAWN | QUEEN_PROMOTION;
            self.from_square = to - 9;
            return true;
        }
        let mut pieces = self.white & self.queens;
        if pieces > 0 {
            pieces &= queen_targets(occupied, to);
            if pieces > 0 {
                self.flags = WHITE_QUEEN;
                self.from_square = lsb(pieces);
                return true;
            }
        }
        let pieces = self.white & self.kings;
        if pieces > 0 {
            self.flags = WHITE_KING;
            self.from_square = lsb(pieces);
            return true;
        }
        false
    }

    #[inline(always)]
    fn play_black(&mut self) {
        let bb_from = 1u64 << self.from_square;
        self.black ^= bb_from;
        match self.flags & !COLOR_MASK {
            CASTLE_SHORT => {
                self.kings ^= bb_from | self.to_bit;
                self.rooks ^= 0xA000_0000_0000_0000;
                self.black ^= 0xA000_0000_0000_0000;
            }
            CASTLE_LONG => {
                self.kings ^= bb_from | self.to_bit;
                self.rooks ^= 0x0900_0000_0000_0000;
                self.black ^= 0x0900_0000_0000_0000;
            }
            _ => {}
        }
    }

    #[inline(always)]
    fn play_white(&mut self) {
        let bb_from = 1u64 << self.from_square;
        self.white ^= bb_from;
        match self.flags & !COLOR_MASK {
            CASTLE_SHORT => {
                self.rooks ^= 0x0000_0000_0000_00A0;
                self.white ^= 0x0000_0000_0000_00A0;
            }
            CASTLE_LONG => {
                self.rooks ^= 0x0000_0000_0000_0009;
                self.white ^= 0x0000_0000_0000_0009;
            }
            _ => {}
        }
    }
}
