use leorik_core::notation::*;
use leorik_core::perft::Perft;

fn perft(fen: &str, depth: usize) -> u64 {
    Perft::new().compute(&board_from_fen(fen).unwrap(), depth)
}

#[test]
fn perft_standard_positions() {
    // https://www.chessprogramming.org/Perft_Results
    let cases: &[(&str, usize, u64)] = &[
        ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 5, 4_865_609),
        ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 4, 4_085_603),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 5, 674_624),
        ("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1", 4, 422_333),
        ("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8", 4, 2_103_487),
        ("r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10", 4, 3_894_594),
    ];
    for &(fen, depth, nodes) in cases {
        assert_eq!(perft(fen, depth), nodes, "{fen}");
    }
}

#[test]
fn zobrist_and_eval_match_leorik() {
    // values printed by the original Leorik 3.2 ("hash" and "eval" commands)
    let start = starting_position();
    assert_eq!(start.zobrist_hash, 11826965726486845583);
    assert_eq!(start.eval.score, 74);
    let mut board = start.clone();
    for uci in ["e2e4", "e7e5", "g1f3"] {
        let mv = move_from_uci(&board, uci, Variant::Standard).unwrap();
        assert!(board.play_move(mv));
    }
    assert_eq!(board.zobrist_hash, 8161745686678069281);
    assert_eq!(board.eval.score, -77);
}

#[test]
fn fen_roundtrip() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 1",
    ] {
        assert_eq!(fen_of(fen), fen);
    }
}

fn fen_of(fen: &str) -> String {
    leorik_core::notation::fen(&board_from_fen(fen).unwrap(), 1)
}

#[test]
fn rejects_broken_fens() {
    assert!(board_from_fen("8/8/8/8/8/8/8/8 w - - 0 1").is_err());
    assert!(board_from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1").is_err());
    assert!(board_from_fen("rnbqkbnr/pppppppp/9/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_err());
    assert!(board_from_fen("garbage").is_err());
}

/// Full perft suites from the Leorik repository (standard + Chess960). Run with `--ignored`.
#[test]
#[ignore]
fn perft_suites_full() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../testdata/");
    let mut checked = 0;
    for line in std::fs::read_to_string(format!("{root}perft.txt")).unwrap().lines() {
        let parts: Vec<&str> = line.split(';').map(str::trim).collect();
        if parts.len() < 3 {
            continue;
        }
        let depth: usize = parts[1].parse().unwrap();
        let nodes: u64 = parts[2].parse().unwrap();
        assert_eq!(perft(parts[0], depth), nodes, "{line}");
        checked += 1;
    }
    for line in std::fs::read_to_string(format!("{root}perft960.epd")).unwrap().lines() {
        let parts: Vec<&str> = line.split(';').map(str::trim).collect();
        if parts.len() < 2 {
            continue;
        }
        // check up to depth 5 to keep the runtime reasonable
        for p in &parts[1..] {
            let (d, n) = p.split_once(' ').unwrap();
            let depth: usize = d[1..].parse().unwrap();
            if depth <= 5 {
                assert_eq!(perft(parts[0], depth), n.parse::<u64>().unwrap(), "{line}");
            }
        }
        checked += 1;
    }
    println!("checked {checked} perft positions");
}
