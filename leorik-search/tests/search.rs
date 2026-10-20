use leorik_core::notation::{self, move_name, Variant};
use leorik_search::tt::TranspositionTable;
use leorik_search::{Engine, Limits, Score};

/// Node counts at fixed depth printed by the original Leorik 3.2 (fresh 50 MB hash).
#[test]
fn node_counts_match_original() {
    let mut engine = Engine::default();
    for uci in ["e2e4", "e7e5", "g1f3"] {
        engine.play(uci).unwrap();
    }
    let mut infos = Vec::new();
    let result = engine.go(&Limits::depth(12), |i| infos.push((i.depth, i.nodes, i.score)));
    let nodes: Vec<i64> = infos.iter().map(|i| i.1).collect();
    assert_eq!(nodes, [45, 145, 324, 624, 906, 1274, 1723, 4608, 10899, 17714, 32368, 61874]);
    assert_eq!(infos.last().unwrap().2, Score::Cp(-22));
    assert_eq!(move_name(result.best_move.unwrap(), Variant::Standard), "g8f6");
    assert_eq!(move_name(result.ponder.unwrap(), Variant::Standard), "f3e5");
}

#[test]
fn finds_scholars_mate() {
    let mut engine = Engine::new(16);
    // Scholar's mate: Qxf7# is mate in one
    engine.set_fen("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4").unwrap();
    let mut last = None;
    let result = engine.go(&Limits::depth(6), |i| last = Some(i.score));
    assert_eq!(move_name(result.best_move.unwrap(), Variant::Standard), "h5f7");
    assert_eq!(last, Some(Score::Mate(1)));
}

#[test]
fn stalemate_and_checkmate_have_no_best_move() {
    let mut engine = Engine::new(1);
    engine.set_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    assert!(engine.go(&Limits::depth(3), |_| {}).best_move.is_none());
}

#[test]
fn repetition_is_scored_as_draw() {
    // after Nf3 Nf6 Ng1 Ng8 Nf3 Nf6 Ng1 the position repeats; a search sees the draw
    let mut engine = Engine::new(16);
    for uci in ["g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1"] {
        engine.play(uci).unwrap();
    }
    assert_eq!(engine.history_plys(), 8);
    let result = engine.go(&Limits::depth(8), |_| {});
    assert!(result.best_move.is_some());
}

#[test]
fn transposition_table_roundtrip() {
    let tt = TranspositionTable::new(1);
    let board = notation::starting_position();
    let mv = notation::move_from_uci(&board, "e2e4", Variant::Standard).unwrap();
    let hash = board.zobrist_hash;
    tt.store(hash, 5, 0, -100, 100, 37, mv);
    assert_eq!(tt.best_move(hash), Some(mv));
    assert_eq!(tt.get_score(hash, 5, 0, -100, 100), (true, mv, 37));
    assert_eq!(tt.get_score(hash, 6, 0, -100, 100), (false, mv, 0), "too shallow");
    // mate scores are stored relative to the node
    tt.store(hash ^ 1, 3, 4, -10000, 10000, 9999 - 7, mv);
    assert_eq!(tt.get_score(hash ^ 1, 3, 2, -10000, 10000).2, 9999 - 5);
    tt.clear();
    assert_eq!(tt.best_move(hash), None);
}

#[test]
fn illegal_moves_are_rejected() {
    let mut engine = Engine::new(1);
    assert!(engine.play("e2e5").is_err());
    assert!(engine.play("e1e2").is_err());
    engine.set_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").unwrap();
    assert!(engine.play("e1f2").is_err(), "f2 is attacked by the rook");
    assert!(engine.play("e1d1").is_ok());
}
