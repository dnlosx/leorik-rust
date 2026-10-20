//! Piece encoding, identical to Leorik's `Piece` flags enum.
//!
//! Bit 0: piece or none, bit 1: white or black, bits 2..4: piece type,
//! bits 5..7: move flags (en passant, promotions, castling).

pub type Piece = u8;

pub const NONE: Piece = 0;

pub const BLACK: Piece = 1;
pub const WHITE: Piece = 3;

pub const PAWN: Piece = 4;
pub const KNIGHT: Piece = 8;
pub const BISHOP: Piece = 12;
pub const ROOK: Piece = 16;
pub const QUEEN: Piece = 20;
pub const KING: Piece = 24;

pub const WHITE_PAWN: Piece = WHITE + PAWN;
pub const WHITE_KNIGHT: Piece = WHITE + KNIGHT;
pub const WHITE_BISHOP: Piece = WHITE + BISHOP;
pub const WHITE_ROOK: Piece = WHITE + ROOK;
pub const WHITE_QUEEN: Piece = WHITE + QUEEN;
pub const WHITE_KING: Piece = WHITE + KING;

pub const BLACK_PAWN: Piece = BLACK + PAWN;
pub const BLACK_KNIGHT: Piece = BLACK + KNIGHT;
pub const BLACK_BISHOP: Piece = BLACK + BISHOP;
pub const BLACK_ROOK: Piece = BLACK + ROOK;
pub const BLACK_QUEEN: Piece = BLACK + QUEEN;
pub const BLACK_KING: Piece = BLACK + KING;

// Flags
pub const EN_PASSANT: Piece = 32 + PAWN;
pub const KNIGHT_PROMOTION: Piece = 64 + PAWN;
pub const BISHOP_PROMOTION: Piece = 96 + PAWN;
pub const ROOK_PROMOTION: Piece = 128 + PAWN;
pub const QUEEN_PROMOTION: Piece = 160 + PAWN;
pub const CASTLE_SHORT: Piece = 192 + KING;
pub const CASTLE_LONG: Piece = 224 + KING;

// Masks
pub const COLOR_MASK: Piece = 3;
pub const TYPE_MASK: Piece = 28;
pub const PIECE_MASK: Piece = 31;

/// Side to move. The numeric value (-1 / +1) is used as a sign in score computations.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(i8)]
pub enum Color {
    Black = -1,
    White = 1,
}

impl Color {
    #[inline(always)]
    pub fn sign(self) -> i32 {
        self as i8 as i32
    }

    #[inline(always)]
    pub fn flip(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

/// Recolors a piece: `(piece & TypeMask) | (color + 2)`.
#[inline(always)]
pub fn of_color(piece: Piece, color: Color) -> Piece {
    (piece & TYPE_MASK) | (color.sign() + 2) as u8
}
