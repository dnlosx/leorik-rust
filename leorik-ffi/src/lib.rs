//! C API for the Leorik chess engine (Rust port). See `include/leorik.h` for the documentation.

use leorik_core::notation::{move_name, Variant};
use leorik_search::{Engine, Limits, Score, SearchInfo, StopHandle};
use std::ffi::{c_char, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

pub const LEORIK_OK: i32 = 0;
pub const LEORIK_ERR_ARGUMENT: i32 = -1;
pub const LEORIK_ERR_POSITION: i32 = -2;
pub const LEORIK_ERR_GAME_OVER: i32 = -3;
pub const LEORIK_ERR_INTERNAL: i32 = -4;

pub const LEORIK_MOVE_LEN: usize = 8;
pub const LEORIK_PV_LEN: usize = 1024;

pub struct LeorikEngine {
    engine: Mutex<Engine>,
    stop: StopHandle,
}

#[repr(C)]
#[derive(Clone)]
pub struct LeorikAnalysis {
    pub depth: i32,
    pub score_cp: i32,
    pub mate: i32,
    pub white_score_cp: i32,
    pub white_mate: i32,
    pub nodes: i64,
    pub time_ms: i32,
    pub best_move: [c_char; LEORIK_MOVE_LEN],
    pub pv: [c_char; LEORIK_PV_LEN],
}

impl Default for LeorikAnalysis {
    fn default() -> Self {
        LeorikAnalysis {
            depth: 0,
            score_cp: 0,
            mate: 0,
            white_score_cp: 0,
            white_mate: 0,
            nodes: 0,
            time_ms: 0,
            best_move: [0; LEORIK_MOVE_LEN],
            pv: [0; LEORIK_PV_LEN],
        }
    }
}

pub type LeorikInfoCallback = Option<extern "C" fn(info: *const LeorikAnalysis, user_data: *mut c_void)>;

/// Copies `s` into a fixed C buffer, truncating at a space boundary if needed, always NUL terminated.
fn copy_str(dst: &mut [c_char], s: &str) {
    let mut s = s;
    if s.len() >= dst.len() {
        s = &s[..dst.len() - 1];
        if let Some(i) = s.rfind(' ') {
            s = &s[..i];
        }
    }
    for (d, b) in dst.iter_mut().zip(s.bytes()) {
        *d = b as c_char;
    }
    dst[s.len()] = 0;
}

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

unsafe fn opt_str<'a>(p: *const c_char) -> Result<&'a str, i32> {
    if p.is_null() {
        return Ok("");
    }
    CStr::from_ptr(p).to_str().map_err(|_| LEORIK_ERR_ARGUMENT)
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct LeorikLimits {
    pub time_ms: i32,
    pub depth: i32,
    pub nodes: i64,
}

fn limits(time_ms: i32, depth: i32) -> Option<Limits> {
    limits_with_nodes(time_ms, depth, 0)
}

fn limits_with_nodes(time_ms: i32, depth: i32, nodes: i64) -> Option<Limits> {
    if time_ms <= 0 && depth <= 0 && nodes <= 0 {
        return None;
    }
    let mut limits = Limits::default();
    if time_ms > 0 {
        limits.move_time = time_ms;
    }
    if depth > 0 {
        limits.max_depth = depth.min(leorik_search::MAX_PLY);
    }
    if nodes > 0 {
        limits.max_nodes = nodes;
    }
    Some(limits)
}

fn fill(out: &mut LeorikAnalysis, info: &SearchInfo, white_to_move: bool, variant: Variant) {
    out.depth = info.depth;
    out.nodes = info.nodes;
    out.time_ms = info.time_ms;
    let (cp, mate) = match info.score {
        Score::Cp(cp) => (cp, 0),
        Score::Mate(m) => (0, m),
    };
    out.score_cp = cp;
    out.mate = mate;
    let sign = if white_to_move { 1 } else { -1 };
    out.white_score_cp = sign * cp;
    out.white_mate = sign * mate;
    let pv: Vec<String> = info.pv.iter().map(|&m| move_name(m, variant)).collect();
    copy_str(&mut out.best_move, pv.first().map(String::as_str).unwrap_or(""));
    copy_str(&mut out.pv, &pv.join(" "));
}

#[no_mangle]
pub extern "C" fn leorik_version() -> *const c_char {
    concat!("Leorik ", env!("CARGO_PKG_VERSION"), " (Rust port)\0").as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn leorik_new(hash_mb: i32, threads: i32) -> *mut LeorikEngine {
    guard(std::ptr::null_mut(), || {
        let hash = if hash_mb <= 0 { leorik_search::tt::DEFAULT_SIZE_MB } else { hash_mb as usize };
        let mut engine = Engine::new(hash);
        engine.options.threads = threads.clamp(1, 256);
        let stop = engine.stop_handle();
        Box::into_raw(Box::new(LeorikEngine { engine: Mutex::new(engine), stop }))
    })
}

/// # Safety
/// `engine` must be NULL or a pointer returned by `leorik_new` that wasn't freed yet.
#[no_mangle]
pub unsafe extern "C" fn leorik_free(engine: *mut LeorikEngine) {
    if !engine.is_null() {
        drop(Box::from_raw(engine));
    }
}

fn lock(engine: &LeorikEngine) -> std::sync::MutexGuard<'_, Engine> {
    // a panic while holding the lock is caught at the API boundary; the engine state is still usable
    engine.engine.lock().unwrap_or_else(|e| e.into_inner())
}

/// # Safety
/// `engine` must be a valid handle; `fen` and `moves` must be NULL or NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn leorik_set_position(engine: *mut LeorikEngine, fen: *const c_char, moves: *const c_char) -> i32 {
    let Some(engine) = engine.as_ref() else { return LEORIK_ERR_ARGUMENT };
    let (fen, moves) = match (opt_str(fen), opt_str(moves)) {
        (Ok(f), Ok(m)) => (f.trim(), m),
        _ => return LEORIK_ERR_ARGUMENT,
    };
    guard(LEORIK_ERR_INTERNAL, || {
        let mut e = lock(engine);
        let mut candidate = Engine::new(1);
        candidate.options = e.options;
        let result = if fen.is_empty() || fen == "startpos" {
            Ok(())
        } else {
            candidate.set_fen(fen)
        };
        if result.is_err() {
            return LEORIK_ERR_POSITION;
        }
        // validate the whole move list first so a bad list leaves the engine untouched
        for uci in moves.split_whitespace() {
            if candidate.play(uci).is_err() {
                return LEORIK_ERR_POSITION;
            }
        }
        if fen.is_empty() || fen == "startpos" {
            e.set_position(leorik_core::notation::starting_position());
        } else {
            e.set_fen(fen).expect("validated");
        }
        for uci in moves.split_whitespace() {
            e.play(uci).expect("validated");
        }
        LEORIK_OK
    })
}

/// # Safety
/// `engine` must be NULL or a valid handle.
#[no_mangle]
pub unsafe extern "C" fn leorik_new_game(engine: *mut LeorikEngine) {
    if let Some(engine) = engine.as_ref() {
        guard((), || lock(engine).reset());
    }
}

/// # Safety
/// `engine` must be NULL or a valid handle.
#[no_mangle]
pub unsafe extern "C" fn leorik_stop(engine: *mut LeorikEngine) {
    if let Some(engine) = engine.as_ref() {
        engine.stop.stop();
    }
}

/// # Safety
/// `engine` must be a valid handle and `out_move` must point to `LEORIK_MOVE_LEN` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn leorik_best_move(engine: *mut LeorikEngine, time_ms: i32, depth: i32, out_move: *mut c_char) -> i32 {
    if out_move.is_null() {
        return LEORIK_ERR_ARGUMENT;
    }
    let mut analysis = LeorikAnalysis::default();
    let rc = leorik_analyze(engine, time_ms, depth, &mut analysis, None, std::ptr::null_mut());
    let out = std::slice::from_raw_parts_mut(out_move, LEORIK_MOVE_LEN);
    if rc == LEORIK_OK {
        out.copy_from_slice(&analysis.best_move);
    } else {
        out[0] = 0;
    }
    rc
}

/// # Safety
/// `engine` must be a valid handle, `out` must point to a writable `LeorikAnalysis`.
#[no_mangle]
pub unsafe extern "C" fn leorik_analyze(
    engine: *mut LeorikEngine,
    time_ms: i32,
    depth: i32,
    out: *mut LeorikAnalysis,
    callback: LeorikInfoCallback,
    user_data: *mut c_void,
) -> i32 {
    let Some(limits) = limits(time_ms, depth) else { return LEORIK_ERR_ARGUMENT };
    run_search(engine, limits, out, callback, user_data)
}

/// # Safety
/// `engine` must be a valid handle, `limits` must point to a `LeorikLimits`, `out` to a writable `LeorikAnalysis`.
#[no_mangle]
pub unsafe extern "C" fn leorik_search(
    engine: *mut LeorikEngine,
    limits: *const LeorikLimits,
    out: *mut LeorikAnalysis,
    callback: LeorikInfoCallback,
    user_data: *mut c_void,
) -> i32 {
    let Some(l) = limits.as_ref() else { return LEORIK_ERR_ARGUMENT };
    let Some(limits) = limits_with_nodes(l.time_ms, l.depth, l.nodes) else { return LEORIK_ERR_ARGUMENT };
    run_search(engine, limits, out, callback, user_data)
}

/// # Safety
/// `engine` must be NULL or a valid handle.
#[no_mangle]
pub unsafe extern "C" fn leorik_set_temperature(engine: *mut LeorikEngine, temperature: i32, seed: i32) -> i32 {
    let Some(engine) = engine.as_ref() else { return LEORIK_ERR_ARGUMENT };
    guard(LEORIK_ERR_INTERNAL, || {
        let mut e = lock(engine);
        e.options.temperature = temperature.clamp(0, 10_000);
        e.options.seed = seed.max(-1);
        LEORIK_OK
    })
}

unsafe fn run_search(
    engine: *mut LeorikEngine,
    limits: Limits,
    out: *mut LeorikAnalysis,
    callback: LeorikInfoCallback,
    user_data: *mut c_void,
) -> i32 {
    let (Some(engine), Some(out)) = (engine.as_ref(), out.as_mut()) else { return LEORIK_ERR_ARGUMENT };
    guard(LEORIK_ERR_INTERNAL, || {
        let mut e = lock(engine);
        if e.legal_moves().is_empty() {
            *out = LeorikAnalysis::default();
            return LEORIK_ERR_GAME_OVER;
        }
        let white = e.side_to_move() == leorik_core::Color::White;
        let variant = e.variant();
        let mut last = LeorikAnalysis::default();
        let result = e.go(&limits, |info| {
            if info.multi_pv == 1 {
                fill(&mut last, info, white, variant);
                if let Some(cb) = callback {
                    cb(&last, user_data);
                }
            }
        });
        // the best move comes from the last completed iteration; make sure it's reported even if the PV is empty
        if let Some(mv) = result.best_move {
            copy_str(&mut last.best_move, &move_name(mv, variant));
        }
        *out = last;
        LEORIK_OK
    })
}
