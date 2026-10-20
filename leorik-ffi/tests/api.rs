use leorik::*;
use std::ffi::{c_char, c_void, CStr, CString};
use std::time::{Duration, Instant};

fn s(buf: &[c_char]) -> String {
    unsafe { CStr::from_ptr(buf.as_ptr()) }.to_str().unwrap().to_string()
}

struct Handle(*mut LeorikEngine);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { leorik_free(self.0) }
    }
}

fn engine() -> Handle {
    let e = leorik_new(16, 1);
    assert!(!e.is_null());
    Handle(e)
}

fn set(e: &Handle, fen: Option<&str>, moves: Option<&str>) -> i32 {
    let fen = fen.map(|f| CString::new(f).unwrap());
    let moves = moves.map(|m| CString::new(m).unwrap());
    unsafe {
        leorik_set_position(
            e.0,
            fen.as_ref().map_or(std::ptr::null(), |f| f.as_ptr()),
            moves.as_ref().map_or(std::ptr::null(), |m| m.as_ptr()),
        )
    }
}

fn analyze(e: &Handle, time_ms: i32, depth: i32) -> (i32, LeorikAnalysis) {
    let mut out = LeorikAnalysis::default();
    let rc = unsafe { leorik_analyze(e.0, time_ms, depth, &mut out, None, std::ptr::null_mut()) };
    (rc, out)
}

#[test]
fn version_string() {
    let v = unsafe { CStr::from_ptr(leorik_version()) }.to_str().unwrap();
    assert!(v.starts_with("Leorik 3.2"), "{v}");
}

#[test]
fn null_handles_are_rejected() {
    let mut mv = [0 as c_char; LEORIK_MOVE_LEN];
    unsafe {
        assert_eq!(leorik_set_position(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()), LEORIK_ERR_ARGUMENT);
        assert_eq!(leorik_best_move(std::ptr::null_mut(), 0, 3, mv.as_mut_ptr()), LEORIK_ERR_ARGUMENT);
        leorik_stop(std::ptr::null_mut());
        leorik_new_game(std::ptr::null_mut());
        leorik_free(std::ptr::null_mut());
    }
    let e = engine();
    unsafe {
        assert_eq!(leorik_best_move(e.0, 0, 3, std::ptr::null_mut()), LEORIK_ERR_ARGUMENT);
        assert_eq!(leorik_best_move(e.0, 0, 0, mv.as_mut_ptr()), LEORIK_ERR_ARGUMENT, "no limit");
    }
}

#[test]
fn best_move_from_start_and_after_moves() {
    let e = engine();
    assert_eq!(set(&e, None, None), LEORIK_OK);
    let mut mv = [0 as c_char; LEORIK_MOVE_LEN];
    assert_eq!(unsafe { leorik_best_move(e.0, 0, 8, mv.as_mut_ptr()) }, LEORIK_OK);
    assert_eq!(s(&mv).len(), 4);

    // same line as the engine parity check: Leorik 3.2 (default 50 MB hash, fresh table) plays g8f6 at
    // depth 12 after exactly 61874 nodes
    let e = Handle(leorik_new(0, 1));
    assert_eq!(set(&e, Some("startpos"), Some("e2e4 e7e5 g1f3")), LEORIK_OK);
    let (rc, a) = analyze(&e, 0, 12);
    assert_eq!(rc, LEORIK_OK);
    assert_eq!(s(&a.best_move), "g8f6");
    assert_eq!(a.depth, 12);
    assert_eq!(a.nodes, 61874);
    assert_eq!(a.score_cp, -22);
    assert_eq!(a.white_score_cp, 22);
    assert!(s(&a.pv).starts_with("g8f6 f3e5"));
}

#[test]
fn invalid_input_keeps_previous_position() {
    let e = engine();
    let mate_in_one = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
    assert_eq!(set(&e, Some(mate_in_one), None), LEORIK_OK);
    assert_eq!(set(&e, Some("not a fen"), None), LEORIK_ERR_POSITION);
    assert_eq!(set(&e, Some("8/8/8/8/8/8/8/8 w - - 0 1"), None), LEORIK_ERR_POSITION);
    assert_eq!(set(&e, None, Some("e2e4 e7e5 e1e3")), LEORIK_ERR_POSITION, "illegal move");
    assert_eq!(set(&e, None, Some("e2e5")), LEORIK_ERR_POSITION);
    let (rc, a) = analyze(&e, 0, 4);
    assert_eq!(rc, LEORIK_OK);
    assert_eq!(s(&a.best_move), "a1a8", "still the mate-in-one position");
}

#[test]
fn mate_scores_are_reported_from_both_points_of_view() {
    let e = engine();
    assert_eq!(set(&e, Some("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"), None), LEORIK_OK);
    let (rc, a) = analyze(&e, 0, 6);
    assert_eq!(rc, LEORIK_OK);
    assert_eq!((a.mate, a.white_mate, a.score_cp), (1, 1, 0));

    // Black to move mates in one
    assert_eq!(set(&e, Some("r5k1/8/8/8/8/8/5PPP/6K1 b - - 0 1"), None), LEORIK_OK);
    let (_, a) = analyze(&e, 0, 6);
    assert_eq!(s(&a.best_move), "a8a1");
    assert_eq!((a.mate, a.white_mate), (1, -1));

    // White to move gets mated next move: 1.? Ra1#
    assert_eq!(set(&e, Some("r5k1/8/8/8/8/8/5PPP/6K1 b - - 0 1"), Some("g8h8")), LEORIK_OK);
    let (_, a) = analyze(&e, 0, 4);
    assert!(a.mate <= 0);
}

#[test]
fn game_over_positions() {
    let e = engine();
    // checkmated
    assert_eq!(set(&e, Some("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1"), None), LEORIK_OK);
    let (rc, _) = analyze(&e, 0, 4);
    assert_eq!(rc, LEORIK_ERR_GAME_OVER);
    // stalemate
    assert_eq!(set(&e, Some("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1"), None), LEORIK_OK);
    let mut mv = [1 as c_char; LEORIK_MOVE_LEN];
    assert_eq!(unsafe { leorik_best_move(e.0, 100, 0, mv.as_mut_ptr()) }, LEORIK_ERR_GAME_OVER);
    assert_eq!(mv[0], 0);
}

#[test]
fn time_limit_is_respected() {
    let e = engine();
    let t = Instant::now();
    let (rc, a) = analyze(&e, 300, 0);
    let elapsed = t.elapsed();
    assert_eq!(rc, LEORIK_OK);
    assert!(elapsed < Duration::from_millis(1000), "{elapsed:?}");
    assert!(a.depth >= 6);
}

#[test]
fn stop_from_another_thread() {
    let e = std::sync::Arc::new(engine());
    let e2 = e.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        unsafe { leorik_stop(e2.0) };
    });
    let t = Instant::now();
    let (rc, a) = analyze(&e, 60_000, 0);
    stopper.join().unwrap();
    assert_eq!(rc, LEORIK_OK);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!(s(&a.best_move).len(), 4);

    // the engine stays usable afterwards
    let (rc, _) = analyze(&e, 0, 5);
    assert_eq!(rc, LEORIK_OK);
}

extern "C" fn collect(info: *const LeorikAnalysis, user: *mut c_void) {
    let depths = unsafe { &mut *(user as *mut Vec<i32>) };
    depths.push(unsafe { (*info).depth });
}

#[test]
fn callback_reports_every_iteration() {
    let e = engine();
    let mut depths: Vec<i32> = Vec::new();
    let mut out = LeorikAnalysis::default();
    let rc = unsafe { leorik_analyze(e.0, 0, 7, &mut out, Some(collect), &mut depths as *mut _ as *mut c_void) };
    assert_eq!(rc, LEORIK_OK);
    assert_eq!(depths, (1..=7).collect::<Vec<_>>());
}

#[test]
fn multiple_threads() {
    let e = Handle(leorik_new(16, 4));
    let (rc, a) = analyze(&e, 0, 10);
    assert_eq!(rc, LEORIK_OK);
    assert_eq!(a.depth, 10);
    assert_eq!(s(&a.best_move).len(), 4);
}

fn search(e: &Handle, time_ms: i32, depth: i32, nodes: i64) -> (i32, LeorikAnalysis) {
    let limits = LeorikLimits { time_ms, depth, nodes };
    let mut out = LeorikAnalysis::default();
    let rc = unsafe { leorik_search(e.0, &limits, &mut out, None, std::ptr::null_mut()) };
    (rc, out)
}

#[test]
fn node_limit() {
    let e = Handle(leorik_new(0, 1));
    let (rc, a) = search(&e, 0, 0, 5000);
    assert_eq!(rc, LEORIK_OK);
    // the node limit is checked inside the search, so the last completed iteration stays below it
    assert!(a.nodes < 5000 && a.depth >= 3, "{} nodes, depth {}", a.nodes, a.depth);
    let (rc, _) = search(&e, 0, 0, 0);
    assert_eq!(rc, LEORIK_ERR_ARGUMENT);
    assert_eq!(unsafe { leorik_search(e.0, std::ptr::null(), &mut LeorikAnalysis::default(), None, std::ptr::null_mut()) }, LEORIK_ERR_ARGUMENT);
}

#[test]
fn temperature_varies_moves_and_seed_makes_them_reproducible() {
    let e = Handle(leorik_new(16, 1));
    let mut moves = std::collections::HashSet::new();
    for seed in 0..12 {
        assert_eq!(unsafe { leorik_set_temperature(e.0, 600, seed) }, LEORIK_OK);
        unsafe { leorik_new_game(e.0) };
        let (_, a) = search(&e, 0, 0, 2000);
        moves.insert(s(&a.best_move));
    }
    assert!(moves.len() >= 3, "temperature should vary the opening move: {moves:?}");

    let mut run = |seed| {
        unsafe { leorik_set_temperature(e.0, 600, seed) };
        unsafe { leorik_new_game(e.0) };
        s(&search(&e, 0, 0, 2000).1.best_move)
    };
    assert_eq!(run(7), run(7));

    // temperature 0 is the plain engine again
    unsafe { leorik_set_temperature(e.0, 0, -1) };
    unsafe { leorik_new_game(e.0) };
    let (_, a) = search(&e, 0, 6, 0);
    assert_eq!(s(&a.best_move).len(), 4);
}
