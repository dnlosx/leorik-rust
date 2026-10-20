//! FEN and UCI move notation (port of the relevant parts of Leorik's `Notation`).

use crate::bitboard::*;
use crate::board::Board;
use crate::movegen::MoveGen;
use crate::moves::Move;
use crate::piece::*;

pub const STARTING_POS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Variant {
    #[default]
    Standard,
    Chess960,
}

pub fn piece_char(piece: Piece) -> char {
    match piece {
        WHITE_PAWN => 'P',
        WHITE_KNIGHT => 'N',
        WHITE_BISHOP => 'B',
        WHITE_ROOK => 'R',
        WHITE_QUEEN => 'Q',
        WHITE_KING => 'K',
        BLACK_PAWN => 'p',
        BLACK_KNIGHT => 'n',
        BLACK_BISHOP => 'b',
        BLACK_ROOK => 'r',
        BLACK_QUEEN => 'q',
        BLACK_KING => 'k',
        _ => ' ',
    }
}

pub fn char_piece(ascii: char) -> Option<Piece> {
    Some(match ascii {
        'P' => WHITE_PAWN,
        'N' => WHITE_KNIGHT,
        'B' => WHITE_BISHOP,
        'R' => WHITE_ROOK,
        'Q' => WHITE_QUEEN,
        'K' => WHITE_KING,
        'p' => BLACK_PAWN,
        'n' => BLACK_KNIGHT,
        'b' => BLACK_BISHOP,
        'r' => BLACK_ROOK,
        'q' => BLACK_QUEEN,
        'k' => BLACK_KING,
        _ => return None,
    })
}

pub fn starting_position() -> Board {
    board_from_fen(STARTING_POS_FEN).expect("valid start position")
}

/// Parses a FEN (or Shredder-/X-FEN for Chess960 castling rights).
///
/// Besides what Leorik checks, this also rejects positions that would make the engine misbehave
/// (missing kings, pieces on the same square, wrong number of ranks) so it's safe to use with
/// untrusted input.
pub fn board_from_fen(fen: &str) -> Result<Board, String> {
    let fields: Vec<&str> = fen.split_whitespace().collect();
    if fields.len() < 4 {
        return Err(format!("FEN needs at least 4 fields. Has only {} fields.", fields.len()));
    }
    let mut result = Board::new();

    let rows: Vec<&str> = fields[0].split('/').collect();
    if rows.len() != 8 {
        return Err(format!("FEN piece placement needs 8 ranks, found {}", rows.len()));
    }
    for (i, row) in rows.iter().enumerate() {
        let rank = 7 - i;
        let mut file = 0usize;
        for c in row.chars() {
            if let Some(empty) = c.to_digit(10) {
                file += empty as usize;
            } else {
                let piece = char_piece(c).ok_or_else(|| format!("Piece character {c} not supported."))?;
                if file > 7 {
                    return Err(format!("Too many squares on rank {}", rank + 1));
                }
                result.set_bit(rank * 8 + file, piece);
                file += 1;
            }
        }
        if file > 8 {
            return Err(format!("Too many squares on rank {}", rank + 1));
        }
    }
    if pop_count(result.white & result.kings) != 1 || pop_count(result.black & result.kings) != 1 {
        return Err("Each side needs exactly one king.".to_string());
    }

    result.side_to_move = if fields[1].eq_ignore_ascii_case("w") { Color::White } else { Color::Black };

    result.castle_flags = parse_castling_rights(fields[2], result.rooks);
    if (result.rooks & result.castle_flags) != result.castle_flags {
        return Err("Invalid CastleFlags, Rooks not found!".to_string());
    }

    result.en_passant = if fields[3] == "-" { 0 } else { 1u64 << square_index(fields[3])? };

    if fields.len() >= 5 {
        if let Ok(halfmoves) = fields[4].parse::<u8>() {
            result.halfmove_clock = halfmoves;
        }
    }

    result.update_eval();
    result.update_hash();
    Ok(result)
}

fn white_kingside_rook_bit(rooks: u64) -> u64 {
    highest_bit(rooks & 0xFF)
}
fn white_queenside_rook_bit(rooks: u64) -> u64 {
    lowest_bit(rooks & 0xFF)
}
fn black_kingside_rook_bit(rooks: u64) -> u64 {
    highest_bit(rooks & 0xFF00_0000_0000_0000)
}
fn black_queenside_rook_bit(rooks: u64) -> u64 {
    lowest_bit(rooks & 0xFF00_0000_0000_0000)
}

fn parse_castling_rights(field: &str, rooks: u64) -> u64 {
    let mut flags = 0u64;
    if field == "-" {
        return flags;
    }
    for c in field.chars() {
        match c {
            'K' => flags |= white_kingside_rook_bit(rooks),
            'Q' => flags |= white_queenside_rook_bit(rooks),
            'k' => flags |= black_kingside_rook_bit(rooks),
            'q' => flags |= black_queenside_rook_bit(rooks),
            'a'..='h' => flags |= 0x0100_0000_0000_0000u64 << (c as u8 - b'a'),
            'A'..='H' => flags |= 0x0000_0000_0000_0001u64 << (c as u8 - b'A'),
            _ => {}
        }
    }
    flags
}

pub fn fen(board: &Board, full_move_number: u32) -> String {
    let mut fen = String::with_capacity(90);
    for rank in (0..8).rev() {
        let mut empty = 0;
        for file in 0..8 {
            let piece = board.get_piece(rank * 8 + file);
            if piece == NONE {
                empty += 1;
            } else {
                if empty > 0 {
                    fen.push(char::from(b'0' + empty));
                    empty = 0;
                }
                fen.push(piece_char(piece));
            }
        }
        if empty > 0 {
            fen.push(char::from(b'0' + empty));
        }
        if rank > 0 {
            fen.push('/');
        }
    }
    fen.push_str(if board.side_to_move == Color::White { " w " } else { " b " });
    if board.castle_flags == 0 {
        fen.push('-');
    } else {
        encode_castling_rights(&mut fen, board);
    }
    fen.push(' ');
    if board.en_passant == 0 {
        fen.push('-');
    } else {
        fen.push_str(&square_name(lsb(board.en_passant)));
    }
    fen.push_str(&format!(" {} {}", board.halfmove_clock, full_move_number));
    fen
}

fn encode_castling_rights(fen: &mut String, board: &Board) {
    let white_king = mask_low(board.kings & board.white);
    let black_king = mask_low(board.kings & board.black);
    let bit = board.castle_flags & board.white & !white_king;
    if bit > 0 {
        fen.push(if bit & white_kingside_rook_bit(board.rooks) > 0 { 'K' } else { (lsb(bit) as u8 % 8 + b'A') as char });
    }
    let bit = board.castle_flags & board.white & white_king;
    if bit > 0 {
        fen.push(if bit & white_queenside_rook_bit(board.rooks) > 0 { 'Q' } else { (lsb(bit) as u8 % 8 + b'A') as char });
    }
    let bit = board.castle_flags & board.black & !black_king;
    if bit > 0 {
        fen.push(if bit & black_kingside_rook_bit(board.rooks) > 0 { 'k' } else { (lsb(bit) as u8 % 8 + b'a') as char });
    }
    let bit = board.castle_flags & board.black & black_king;
    if bit > 0 {
        fen.push(if bit & black_queenside_rook_bit(board.rooks) > 0 { 'q' } else { (lsb(bit) as u8 % 8 + b'a') as char });
    }
}

pub fn square_name(square: usize) -> String {
    let rank = square / 8;
    let file = square % 8;
    format!("{}{}", (b'a' + file as u8) as char, rank + 1)
}

pub fn square_index(name: &str) -> Result<usize, String> {
    let b = name.as_bytes();
    if b.len() < 2 {
        return Err(format!("Invalid square {name}"));
    }
    let file = b[0].wrapping_sub(b'a') as usize;
    let rank = b[1].wrapping_sub(b'1') as usize;
    if file < 8 && rank < 8 {
        Ok(rank * 8 + file)
    } else {
        Err(format!("The given square notation {name} does not map to a valid index between 0 and 63"))
    }
}

/// Long algebraic (UCI) notation. In `Standard` mode castling is written as the king's move (e1g1),
/// in `Chess960` mode as king-takes-rook (e1h1).
pub fn move_name(mv: Move, variant: Variant) -> String {
    if variant == Variant::Standard && mv.flags >= CASTLE_SHORT {
        match mv.flags {
            f if f == CASTLE_SHORT | BLACK => return "e8g8".to_string(),
            f if f == CASTLE_LONG | BLACK => return "e8c8".to_string(),
            f if f == CASTLE_SHORT | WHITE => return "e1g1".to_string(),
            f if f == CASTLE_LONG | WHITE => return "e1c1".to_string(),
            _ => {}
        }
    }
    let mut result = square_name(mv.from as usize);
    result.push_str(&square_name(mv.to as usize));
    if mv.is_promotion() {
        result.push(piece_char(mv.new_piece()).to_ascii_lowercase());
    }
    result
}

/// Finds the pseudo-legal move with the given UCI name. Like Leorik, this does not check legality.
pub fn move_from_uci(board: &Board, uci: &str, variant: Variant) -> Result<Move, String> {
    if uci.len() < 4 {
        return Err(format!("Long algebraic notation expected. '{uci}' is too short!"));
    }
    if uci.len() > 5 {
        return Err(format!("Long algebraic notation expected. '{uci}' is too long!"));
    }
    let mut moves = [Move::NONE; 256];
    let mut gen = MoveGen::new(&mut moves, 0);
    gen.collect_all(board);
    let end = gen.next;
    moves[..end]
        .iter()
        .copied()
        .find(|&mv| move_name(mv, variant) == uci)
        .ok_or_else(|| format!("No move meeting all requirements could be found for '{uci}'!"))
}

/// Like [`move_from_uci`] but also verifies that the move is legal.
pub fn legal_move_from_uci(board: &Board, uci: &str, variant: Variant) -> Result<Move, String> {
    let mv = move_from_uci(board, uci, variant)?;
    let mut temp = Board::new();
    if temp.play_without_hash_and_eval(board, mv) {
        Ok(mv)
    } else {
        Err(format!("Move '{uci}' is illegal in this position!"))
    }
}
