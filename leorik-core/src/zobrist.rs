//! Zobrist keys, generated exactly like Leorik does (`new Random(228126)`).

use crate::dotnet_random::DotNetRandom;
use crate::piece::Piece;

pub struct ZobristTables {
    pub board: [[u64; 14]; 64],
    pub flags: [u64; 64],
    pub side_to_move: u64,
}

const fn generate() -> ZobristTables {
    let mut rnd = DotNetRandom::new(228126);
    let mut board = [[0u64; 14]; 64];
    let mut flags = [0u64; 64];
    let mut square = 0;
    while square < 64 {
        let mut piece = 2;
        while piece < 14 {
            board[square][piece] = rnd.next_u64();
            piece += 1;
        }
        flags[square] = rnd.next_u64();
        square += 1;
    }
    let side_to_move = rnd.next_u64();
    ZobristTables { board, flags, side_to_move }
}

pub static ZOBRIST: ZobristTables = generate();

#[inline(always)]
pub fn piece_square(piece: Piece, square: usize) -> u64 {
    ZOBRIST.board[square][piece_index(piece)]
}

#[inline(always)]
pub fn piece_index(piece: Piece) -> usize {
    (piece >> 1) as usize
}

#[inline(always)]
pub fn flags(square: usize) -> u64 {
    ZOBRIST.flags[square]
}

#[inline(always)]
pub fn side_to_move() -> u64 {
    ZOBRIST.side_to_move
}
