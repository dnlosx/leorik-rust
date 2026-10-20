//! Time management (port of Leorik's `TimeControl`). All state is atomic so `stop` and
//! `ponderhit` can be called from another thread while the search is running.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

const BASE_MARGIN: i32 = 5;
const MAX_TIME: i32 = i32::MAX / 3; // large but not too large to cause overflow issues
const PLAY_ON_INC: f32 = 0.5;
const PLAY_ON_RESERVE: f32 = 0.9;

pub struct TimeControl {
    epoch: Instant,
    pondering: AtomicBool,
    move_time: AtomicI32,
    move_time_limit: AtomicI32,
    max_depth: AtomicI32,
    t0_nanos: AtomicU64,
}

impl Default for TimeControl {
    fn default() -> Self {
        TimeControl::new()
    }
}

impl TimeControl {
    pub fn new() -> TimeControl {
        TimeControl {
            epoch: Instant::now(),
            pondering: AtomicBool::new(false),
            move_time: AtomicI32::new(0),
            move_time_limit: AtomicI32::new(0),
            max_depth: AtomicI32::new(0),
            t0_nanos: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    fn now_nanos(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Milliseconds since the clock started.
    #[inline(always)]
    pub fn elapsed(&self) -> i32 {
        let dt = self.now_nanos().saturating_sub(self.t0_nanos.load(Relaxed));
        (dt / 1_000_000) as i32
    }

    pub fn is_pondering(&self) -> bool {
        self.pondering.load(Relaxed)
    }

    /// Makes `can_search_deeper` and `check_time_budget` signal the search to stop.
    pub fn stop(&self) {
        self.pondering.store(false, Relaxed);
        self.move_time_limit.store(0, Relaxed);
    }

    pub fn ponderhit(&self) {
        // now the clock's running!
        self.t0_nanos.store(self.now_nanos(), Relaxed);
        self.move_time.store(self.move_time.load(Relaxed) / 2, Relaxed);
        self.pondering.store(false, Relaxed);
    }

    /// Fixed time per move (or infinite with `i32::MAX`).
    pub fn go_fixed(&self, max_depth: i32, time_per_move: i32, pondering: bool) {
        self.pondering.store(pondering, Relaxed);
        self.t0_nanos.store(self.now_nanos(), Relaxed);
        self.max_depth.store(max_depth, Relaxed);
        let t = time_per_move.min(MAX_TIME);
        self.move_time.store(t, Relaxed);
        self.move_time_limit.store(t, Relaxed);
    }

    /// Time budget derived from the remaining clock time.
    pub fn go_clock(&self, max_depth: i32, time: i32, increment: i32, moves_to_go: i32, pondering: bool) {
        let time = time.clamp(1, MAX_TIME);
        let moves_to_go = moves_to_go.max(1);

        self.pondering.store(pondering, Relaxed);
        self.t0_nanos.store(self.now_nanos(), Relaxed);
        self.max_depth.store(max_depth, Relaxed);

        let future_moves = moves_to_go - 1;
        let inc_time = increment.wrapping_mul(future_moves);
        let time_remaining = time.wrapping_add(inc_time);

        let move_time = time.min(time_remaining / moves_to_go);
        self.move_time.store(move_time, Relaxed);

        // abort as soon as the time left for each remaining move is less than 'reserve_ratio' of this one
        let inc_ratio = (inc_time as f32 / time as f32).clamp(0.0, 1.0);
        let reserve_ratio = PLAY_ON_RESERVE + (PLAY_ON_INC - PLAY_ON_RESERVE) * inc_ratio;
        let reserve = (reserve_ratio * future_moves as f32 * move_time as f32) as i32;
        self.move_time_limit.store(time.min(time_remaining - reserve), Relaxed);
    }

    pub fn can_search_deeper(&self, depth: i32, multi_pv: i32, stability: f32) -> bool {
        self.move_time_limit.fetch_sub(BASE_MARGIN * multi_pv, Relaxed);

        if depth >= self.max_depth.load(Relaxed) {
            return false;
        }
        if self.is_pondering() {
            return true; // clock's not running
        }
        // high stability means we're confident we already have the best move
        let stop_ratio = 1.0 / (1.0 + 2.0 * stability);
        if self.elapsed() as f32 > stop_ratio * self.move_time.load(Relaxed) as f32 {
            return false;
        }
        true
    }

    #[inline(always)]
    pub fn check_time_budget(&self) -> bool {
        if self.is_pondering() {
            false // no need to ever abort during pondering!
        } else {
            self.elapsed() > self.move_time_limit.load(Relaxed)
        }
    }
}
