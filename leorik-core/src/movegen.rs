//! Pseudo-legal move generation (port of Leorik's `MoveGen`).
//!
//! Moves are appended to a caller-provided buffer starting at `next`. Legality is
//! checked when a move is played (`Board::play` returns false for illegal moves).

use crate::bitboard::*;
use crate::board::Board;
use crate::moves::Move;
use crate::piece::*;

/// Maximum number of pseudo-legal moves in any position (https://www.stmintz.com/ccc/index.php?id=425058).
pub const MAX_MOVES: usize = 225;

pub struct MoveGen<'a> {
    pub moves: &'a mut [Move],
    pub next: usize,
}

/// All legal moves of the position, in generation order.
pub fn legal_moves(board: &Board) -> Vec<Move> {
    let mut buffer = [Move::NONE; 256];
    let mut gen = MoveGen::new(&mut buffer, 0);
    gen.collect_all(board);
    let end = gen.next;
    let mut temp = Board::new();
    buffer[..end].iter().copied().filter(|&mv| temp.play(board, mv)).collect()
}

impl<'a> MoveGen<'a> {
    pub fn new(moves: &'a mut [Move], next: usize) -> MoveGen<'a> {
        MoveGen { moves, next }
    }

    /// Adds `mv` unless it's the default (empty) move. Returns the index before adding.
    #[inline(always)]
    pub fn collect_move(&mut self, mv: Move) -> usize {
        let old = self.next;
        if mv != Move::NONE {
            self.moves[self.next] = mv;
            self.next += 1;
        }
        old
    }

    #[inline(always)]
    pub fn collect_all(&mut self, board: &Board) -> usize {
        let old = self.next;
        if board.side_to_move == Color::White {
            self.collect_white_captures(board);
            self.collect_white_quiets(board);
        } else {
            self.collect_black_captures(board);
            self.collect_black_quiets(board);
        }
        old
    }

    #[inline(always)]
    pub fn collect_captures(&mut self, board: &Board) -> usize {
        let old = self.next;
        if board.side_to_move == Color::White {
            self.collect_white_captures(board);
        } else {
            self.collect_black_captures(board);
        }
        old
    }

    #[inline(always)]
    pub fn collect_quiets(&mut self, board: &Board) -> usize {
        let old = self.next;
        if board.side_to_move == Color::White {
            self.collect_white_quiets(board);
        } else {
            self.collect_black_quiets(board);
        }
        old
    }

    #[inline(always)]
    fn add(&mut self, flags: Piece, from: usize, to: usize) {
        self.moves[self.next] = Move::new(flags, from, to, NONE);
        self.next += 1;
    }

    #[inline(always)]
    fn add_all(&mut self, piece: Piece, square: usize, mut targets: u64) {
        while targets != 0 {
            self.add(piece, square, lsb(targets));
            targets = clear_lsb(targets);
        }
    }

    #[inline(always)]
    fn pawn_move(&mut self, flags: Piece, move_targets: u64, offset: isize) {
        let to = lsb(move_targets);
        let from = (to as isize + offset) as usize;
        self.add(flags, from, to);
    }

    #[inline(always)]
    fn add_all_captures(&mut self, piece: Piece, square: usize, mut targets: u64, board: &Board) {
        while targets != 0 {
            self.add_capture(piece, square, lsb(targets), board);
            targets = clear_lsb(targets);
        }
    }

    #[inline(always)]
    fn add_capture(&mut self, flags: Piece, from: usize, to: usize, board: &Board) {
        self.moves[self.next] = Move::new(flags, from, to, board.get_piece(to));
        self.next += 1;
    }

    #[inline(always)]
    fn pawn_capture(&mut self, flags: Piece, move_targets: u64, offset: isize, board: &Board) {
        let to = lsb(move_targets);
        let from = (to as isize + offset) as usize;
        self.add_capture(flags, from, to, board);
    }

    #[inline(always)]
    fn pawn_capture_promotions(&mut self, flags: Piece, move_targets: u64, offset: isize, board: &Board) {
        let to = lsb(move_targets);
        let from = (to as isize + offset) as usize;
        let target = board.get_piece(to);
        for promo in [QUEEN_PROMOTION, ROOK_PROMOTION, BISHOP_PROMOTION, KNIGHT_PROMOTION] {
            self.moves[self.next] = Move::new(flags | promo, from, to, target);
            self.next += 1;
        }
    }

    fn collect_black_captures(&mut self, board: &Board) {
        let occupied = board.black | board.white;

        let square = lsb(board.kings & board.black);
        self.add_all_captures(BLACK_KING, square, KING_TARGETS[square] & board.white, board);

        let mut knights = board.knights & board.black;
        while knights != 0 {
            let square = lsb(knights);
            self.add_all_captures(BLACK_KNIGHT, square, KNIGHT_TARGETS[square] & board.white, board);
            knights = clear_lsb(knights);
        }
        let mut bishops = board.bishops & board.black;
        while bishops != 0 {
            let square = lsb(bishops);
            self.add_all_captures(BLACK_BISHOP, square, bishop_targets(occupied, square) & board.white, board);
            bishops = clear_lsb(bishops);
        }
        let mut rooks = board.rooks & board.black;
        while rooks != 0 {
            let square = lsb(rooks);
            self.add_all_captures(BLACK_ROOK, square, rook_targets(occupied, square) & board.white, board);
            rooks = clear_lsb(rooks);
        }
        let mut queens = board.queens & board.black;
        while queens != 0 {
            let square = lsb(queens);
            self.add_all_captures(BLACK_QUEEN, square, queen_targets(occupied, square) & board.white, board);
            queens = clear_lsb(queens);
        }

        let black_pawns = board.pawns & board.black;

        let capture_left = ((black_pawns & 0xFEFE_FEFE_FEFE_FEFE) >> 9) & board.white;
        let mut targets = capture_left & 0xFFFF_FFFF_FFFF_FF00;
        while targets != 0 {
            self.pawn_capture(BLACK_PAWN, targets, 9, board);
            targets = clear_lsb(targets);
        }
        let mut targets = capture_left & 0x0000_0000_0000_00FF;
        while targets != 0 {
            self.pawn_capture_promotions(BLACK_PAWN, targets, 9, board);
            targets = clear_lsb(targets);
        }

        let capture_right = ((black_pawns & 0x7F7F_7F7F_7F7F_7F7F) >> 7) & board.white;
        let mut targets = capture_right & 0xFFFF_FFFF_FFFF_FF00;
        while targets != 0 {
            self.pawn_capture(BLACK_PAWN, targets, 7, board);
            targets = clear_lsb(targets);
        }
        let mut targets = capture_right & 0x0000_0000_0000_00FF;
        while targets != 0 {
            self.pawn_capture_promotions(BLACK_PAWN, targets, 7, board);
            targets = clear_lsb(targets);
        }

        // en passant
        let capture_left = ((black_pawns & 0x0000_0000_FE00_0000) >> 9) & board.en_passant;
        if capture_left != 0 {
            self.pawn_move(BLACK_PAWN | EN_PASSANT, capture_left, 9);
        }
        let capture_right = ((black_pawns & 0x0000_0000_7F00_0000) >> 7) & board.en_passant;
        if capture_right != 0 {
            self.pawn_move(BLACK_PAWN | EN_PASSANT, capture_right, 7);
        }

        // move down and promote to queen
        let mut targets = (black_pawns >> 8) & !occupied & 0x0000_0000_0000_00FF;
        while targets != 0 {
            self.pawn_move(BLACK | QUEEN_PROMOTION, targets, 8);
            targets = clear_lsb(targets);
        }
    }

    fn collect_black_quiets(&mut self, board: &Board) {
        let occupied = board.black | board.white;

        let king = board.kings & board.black;
        let square = lsb(king);
        self.add_all(BLACK_KING, square, KING_TARGETS[square] & !occupied);

        let mut knights = board.knights & board.black;
        while knights != 0 {
            let square = lsb(knights);
            self.add_all(BLACK_KNIGHT, square, KNIGHT_TARGETS[square] & !occupied);
            knights = clear_lsb(knights);
        }
        let mut bishops = board.bishops & board.black;
        while bishops != 0 {
            let square = lsb(bishops);
            self.add_all(BLACK_BISHOP, square, bishop_targets(occupied, square) & !occupied);
            bishops = clear_lsb(bishops);
        }
        let mut rooks = board.rooks & board.black;
        while rooks != 0 {
            let square = lsb(rooks);
            self.add_all(BLACK_ROOK, square, rook_targets(occupied, square) & !occupied);
            rooks = clear_lsb(rooks);
        }
        let mut queens = board.queens & board.black;
        while queens != 0 {
            let square = lsb(queens);
            self.add_all(BLACK_QUEEN, square, queen_targets(occupied, square) & !occupied);
            queens = clear_lsb(queens);
        }

        let black_pawns = board.pawns & board.black;
        let one_step = (black_pawns >> 8) & !occupied;
        let mut targets = one_step & 0xFFFF_FFFF_FFFF_FF00;
        while targets != 0 {
            self.pawn_move(BLACK_PAWN, targets, 8);
            targets = clear_lsb(targets);
        }
        let mut targets = one_step & 0x0000_0000_0000_00FF;
        while targets != 0 {
            self.pawn_move(BLACK | ROOK_PROMOTION, targets, 8);
            self.pawn_move(BLACK | BISHOP_PROMOTION, targets, 8);
            self.pawn_move(BLACK | KNIGHT_PROMOTION, targets, 8);
            targets = clear_lsb(targets);
        }
        let two_step = (one_step >> 8) & !occupied;
        let mut targets = two_step & 0x0000_00FF_0000_0000;
        while targets != 0 {
            self.pawn_move(BLACK_PAWN, targets, 16);
            targets = clear_lsb(targets);
        }

        // Chess960 castling requirements:
        // 1.) Neither king nor rook has moved (castle flags are set)
        // 2.) All squares between the king's initial and final squares (inclusive), and all squares between
        //     the rook's initial and final squares (inclusive) must be vacant except for the king and rook
        // 3.) No square the king passes through, including start and final square, is under enemy attack.
        let square = lsb(king);
        let left_rook = lower_bits(square) & 0xFF00_0000_0000_0000 & board.castle_flags;
        if left_rook != 0 {
            let left_rook_square = lsb(left_rook);
            let vacancy = bits_between(left_rook_square, 59) | bits_between(square, 58);
            if ((occupied ^ king ^ left_rook) & vacancy) == 0 && !is_attacked_by_white(board, bits_between(square, 58)) {
                self.add_capture(BLACK_ROOK | CASTLE_LONG, square, left_rook_square, board);
            }
        }
        let right_rook = higher_bits(square) & board.castle_flags;
        if right_rook != 0 {
            let right_rook_square = lsb(right_rook);
            let vacancy = bits_between(right_rook_square, 61) | bits_between(square, 62);
            if ((occupied ^ king ^ right_rook) & vacancy) == 0 && !is_attacked_by_white(board, bits_between(square, 62)) {
                // castling moves are encoded as king "takes" own rook
                self.add_capture(BLACK_ROOK | CASTLE_SHORT, square, right_rook_square, board);
            }
        }
    }

    fn collect_white_captures(&mut self, board: &Board) {
        let occupied = board.black | board.white;

        let square = lsb(board.kings & board.white);
        self.add_all_captures(WHITE_KING, square, KING_TARGETS[square] & board.black, board);

        let mut knights = board.knights & board.white;
        while knights != 0 {
            let square = lsb(knights);
            self.add_all_captures(WHITE_KNIGHT, square, KNIGHT_TARGETS[square] & board.black, board);
            knights = clear_lsb(knights);
        }
        let mut bishops = board.bishops & board.white;
        while bishops != 0 {
            let square = lsb(bishops);
            self.add_all_captures(WHITE_BISHOP, square, bishop_targets(occupied, square) & board.black, board);
            bishops = clear_lsb(bishops);
        }
        let mut rooks = board.rooks & board.white;
        while rooks != 0 {
            let square = lsb(rooks);
            self.add_all_captures(WHITE_ROOK, square, rook_targets(occupied, square) & board.black, board);
            rooks = clear_lsb(rooks);
        }
        let mut queens = board.queens & board.white;
        while queens != 0 {
            let square = lsb(queens);
            self.add_all_captures(WHITE_QUEEN, square, queen_targets(occupied, square) & board.black, board);
            queens = clear_lsb(queens);
        }

        let white_pawns = board.pawns & board.white;

        let capture_left = ((white_pawns & 0xFEFE_FEFE_FEFE_FEFE) << 7) & board.black;
        let mut targets = capture_left & 0x00FF_FFFF_FFFF_FFFF;
        while targets != 0 {
            self.pawn_capture(WHITE_PAWN, targets, -7, board);
            targets = clear_lsb(targets);
        }
        let mut targets = capture_left & 0xFF00_0000_0000_0000;
        while targets != 0 {
            self.pawn_capture_promotions(WHITE_PAWN, targets, -7, board);
            targets = clear_lsb(targets);
        }

        let capture_right = ((white_pawns & 0x7F7F_7F7F_7F7F_7F7F) << 9) & board.black;
        let mut targets = capture_right & 0x00FF_FFFF_FFFF_FFFF;
        while targets != 0 {
            self.pawn_capture(WHITE_PAWN, targets, -9, board);
            targets = clear_lsb(targets);
        }
        let mut targets = capture_right & 0xFF00_0000_0000_0000;
        while targets != 0 {
            self.pawn_capture_promotions(WHITE_PAWN, targets, -9, board);
            targets = clear_lsb(targets);
        }

        // en passant
        let capture_left = ((white_pawns & 0x0000_00FE_0000_0000) << 7) & board.en_passant;
        if capture_left != 0 {
            self.pawn_move(WHITE_PAWN | EN_PASSANT, capture_left, -7);
        }
        let capture_right = ((white_pawns & 0x0000_007F_0000_0000) << 9) & board.en_passant;
        if capture_right != 0 {
            self.pawn_move(WHITE_PAWN | EN_PASSANT, capture_right, -9);
        }

        // move up and promote to queen
        let mut targets = (white_pawns << 8) & !occupied & 0xFF00_0000_0000_0000;
        while targets != 0 {
            self.pawn_move(WHITE | QUEEN_PROMOTION, targets, -8);
            targets = clear_lsb(targets);
        }
    }

    fn collect_white_quiets(&mut self, board: &Board) {
        let occupied = board.black | board.white;

        let king = board.kings & board.white;
        let square = lsb(king);
        self.add_all(WHITE_KING, square, KING_TARGETS[square] & !occupied);

        let mut knights = board.knights & board.white;
        while knights != 0 {
            let square = lsb(knights);
            self.add_all(WHITE_KNIGHT, square, KNIGHT_TARGETS[square] & !occupied);
            knights = clear_lsb(knights);
        }
        let mut bishops = board.bishops & board.white;
        while bishops != 0 {
            let square = lsb(bishops);
            self.add_all(WHITE_BISHOP, square, bishop_targets(occupied, square) & !occupied);
            bishops = clear_lsb(bishops);
        }
        let mut rooks = board.rooks & board.white;
        while rooks != 0 {
            let square = lsb(rooks);
            self.add_all(WHITE_ROOK, square, rook_targets(occupied, square) & !occupied);
            rooks = clear_lsb(rooks);
        }
        let mut queens = board.queens & board.white;
        while queens != 0 {
            let square = lsb(queens);
            self.add_all(WHITE_QUEEN, square, queen_targets(occupied, square) & !occupied);
            queens = clear_lsb(queens);
        }

        let white_pawns = board.pawns & board.white;
        let one_step = (white_pawns << 8) & !occupied;
        let mut targets = one_step & 0x00FF_FFFF_FFFF_FFFF;
        while targets != 0 {
            self.pawn_move(WHITE_PAWN, targets, -8);
            targets = clear_lsb(targets);
        }
        let mut targets = one_step & 0xFF00_0000_0000_0000;
        while targets != 0 {
            self.pawn_move(WHITE | ROOK_PROMOTION, targets, -8);
            self.pawn_move(WHITE | BISHOP_PROMOTION, targets, -8);
            self.pawn_move(WHITE | KNIGHT_PROMOTION, targets, -8);
            targets = clear_lsb(targets);
        }
        let two_step = (one_step << 8) & !occupied;
        let mut targets = two_step & 0x0000_0000_FF00_0000;
        while targets != 0 {
            self.pawn_move(WHITE_PAWN, targets, -16);
            targets = clear_lsb(targets);
        }

        let square = lsb(king);
        let left_rook = lower_bits(square) & board.castle_flags;
        if left_rook != 0 {
            let left_rook_square = lsb(left_rook);
            let vacancy = bits_between(left_rook_square, 3) | bits_between(square, 2);
            if ((occupied ^ king ^ left_rook) & vacancy) == 0 && !is_attacked_by_black(board, bits_between(square, 2)) {
                self.add_capture(WHITE_ROOK | CASTLE_LONG, square, left_rook_square, board);
            }
        }
        let right_rook = higher_bits(square) & 0x0000_0000_0000_00FF & board.castle_flags;
        if right_rook != 0 {
            let right_rook_square = lsb(right_rook);
            let vacancy = bits_between(right_rook_square, 5) | bits_between(square, 6);
            if ((occupied ^ king ^ right_rook) & vacancy) == 0 && !is_attacked_by_black(board, bits_between(square, 6)) {
                self.add_capture(WHITE_ROOK | CASTLE_SHORT, square, right_rook_square, board);
            }
        }
    }
}

#[inline(always)]
fn is_attacked_by_white(board: &Board, mut mask: u64) -> bool {
    while mask != 0 {
        if board.is_attacked_by_white(lsb(mask)) {
            return true;
        }
        mask = clear_lsb(mask);
    }
    false
}

#[inline(always)]
fn is_attacked_by_black(board: &Board, mut mask: u64) -> bool {
    while mask != 0 {
        if board.is_attacked_by_black(lsb(mask)) {
            return true;
        }
        mask = clear_lsb(mask);
    }
    false
}
