//! Move representation, identical in layout to Leorik's `Move` struct.

use crate::piece::*;

#[derive(Copy, Clone, Default, PartialEq, Eq, Hash)]
#[repr(C, align(4))]
pub struct Move {
    pub flags: Piece,
    pub target: Piece,
    pub from: u8,
    pub to: u8,
}

impl Move {
    pub const NONE: Move = Move { flags: 0, target: 0, from: 0, to: 0 };

    #[inline(always)]
    pub const fn new(flags: Piece, from: usize, to: usize, target: Piece) -> Move {
        Move { flags, target, from: from as u8, to: to as u8 }
    }

    #[inline(always)]
    pub fn is_none(self) -> bool {
        self == Move::NONE
    }

    #[inline(always)]
    pub fn from_square(self) -> usize {
        self.from as usize
    }

    #[inline(always)]
    pub fn to_square(self) -> usize {
        self.to as usize
    }

    #[inline(always)]
    pub fn new_piece(self) -> Piece {
        if self.flags < KNIGHT_PROMOTION {
            self.flags & PIECE_MASK
        } else if self.flags >= CASTLE_SHORT {
            NONE
        } else {
            ((self.flags >> 3) & !COLOR_MASK) | (self.flags & COLOR_MASK)
        }
    }

    #[inline(always)]
    pub fn captured_piece(self) -> Piece {
        if self.flags >= CASTLE_SHORT {
            NONE
        } else {
            self.target
        }
    }

    #[inline(always)]
    pub fn moving_piece(self) -> Piece {
        self.flags & PIECE_MASK
    }

    #[inline(always)]
    pub fn moving_piece_type(self) -> Piece {
        self.flags & TYPE_MASK
    }

    #[inline(always)]
    pub fn is_promotion(self) -> bool {
        self.flags >= KNIGHT_PROMOTION && self.flags < CASTLE_SHORT
    }

    #[inline(always)]
    pub fn is_castling(self) -> bool {
        self.flags >= CASTLE_SHORT
    }

    #[inline(always)]
    pub fn is_en_passant(self) -> bool {
        (self.flags & !COLOR_MASK) == EN_PASSANT
    }

    /// Most valuable victim, least valuable attacker.
    #[inline(always)]
    pub fn mvv_lva_score(self) -> i32 {
        6 * Move::order(self.target) - Move::order(self.flags & PIECE_MASK)
    }

    /// Pawn = 1, Knight = 2, Bishop = 3, Rook = 4, Queen = 5, King = 6
    #[inline(always)]
    pub fn order(piece: Piece) -> i32 {
        (piece >> 2) as i32
    }

    /// Packs the move into 32 bits like Leorik's `(int)move` cast.
    #[inline(always)]
    pub fn to_bits(self) -> u32 {
        self.flags as u32 | (self.from as u32) << 8 | (self.to as u32) << 16 | (self.target as u32) << 24
    }

    #[inline(always)]
    pub fn from_bits(bits: u32) -> Move {
        Move { flags: bits as u8, from: (bits >> 8) as u8, to: (bits >> 16) as u8, target: (bits >> 24) as u8 }
    }
}

impl core::fmt::Debug for Move {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", crate::notation::move_name(*self, crate::notation::Variant::Chess960))
    }
}

impl core::fmt::Display for Move {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", crate::notation::move_name(*self, crate::notation::Variant::Standard))
    }
}
