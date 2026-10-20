//! Game state and search driver (port of the non-I/O parts of Leorik's `Engine`).
//!
//! `Engine::go` runs the search on the calling thread and blocks until it's done.
//! Use [`Engine::stop_handle`] to stop it (or signal `ponderhit`) from another thread.

use crate::search::{KillSwitchFn, Search, SearchOptions, MAX_PLY};
use crate::time::TimeControl;
use crate::tt::{TranspositionTable, DEFAULT_SIZE_MB};
use leorik_core::eval::{is_checkmate, mate_distance};
use leorik_core::movegen::legal_moves;
use leorik_core::notation::{self, Variant};
use leorik_core::perft::Perft;
use leorik_core::{Board, Color, Move};
use std::sync::Arc;

/// A search score from the side to move's point of view.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Score {
    /// Centipawns (normalized so that 100 = one pawn).
    Cp(i32),
    /// Mate in N moves; negative if the side to move gets mated.
    Mate(i32),
}

impl Score {
    /// `raw` is Leorik's score (normalized cp or a raw mate score) from the side to move's perspective.
    pub fn from_raw(raw: i32) -> Score {
        if is_checkmate(raw) {
            Score::Mate(raw.signum() * mate_distance(raw))
        } else {
            Score::Cp(raw)
        }
    }

    pub fn negate(self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(-cp),
            Score::Mate(m) => Score::Mate(-m),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SearchInfo {
    pub depth: i32,
    pub multi_pv: i32,
    /// Side-to-move relative score.
    pub score: Score,
    pub nodes: i64,
    pub time_ms: i32,
    pub pv: Vec<Move>,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct SearchResult {
    /// `None` if there is no legal move.
    pub best_move: Option<Move>,
    pub ponder: Option<Move>,
}

/// Search limits for `go`.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_depth: i32,
    pub max_nodes: i64,
    /// Fixed time per move in ms (`i32::MAX` = infinite). Ignored if `clock` is set.
    pub move_time: i32,
    /// Remaining time, increment and moves to go for the side to move.
    pub clock: Option<Clock>,
    pub search_moves: Option<Vec<Move>>,
    pub ponder: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct Clock {
    pub time: i32,
    pub increment: i32,
    /// `None` = guess like Leorik does.
    pub moves_to_go: Option<i32>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_depth: MAX_PLY, max_nodes: i64::MAX, move_time: i32::MAX, clock: None, search_moves: None, ponder: false }
    }
}

impl Limits {
    pub fn depth(depth: i32) -> Limits {
        Limits { max_depth: depth.clamp(1, MAX_PLY), ..Limits::default() }
    }
    pub fn move_time(ms: i32) -> Limits {
        Limits { move_time: ms.max(0), ..Limits::default() }
    }
    pub fn nodes(nodes: i64) -> Limits {
        Limits { max_nodes: nodes.max(1), ..Limits::default() }
    }
}

/// Lets another thread stop a running search or signal `ponderhit`.
#[derive(Clone)]
pub struct StopHandle(Arc<TimeControl>);

impl StopHandle {
    pub fn stop(&self) {
        self.0.stop();
    }
    pub fn ponderhit(&self) {
        self.0.ponderhit();
    }
}

pub struct Engine {
    board: Board,
    /// (Zobrist hash, halfmove clock) of every position since the last `set_position`, incl. the current one.
    history: Vec<(u64, u8)>,
    time: Arc<TimeControl>,
    tt: Arc<TranspositionTable>,
    pub options: SearchOptions,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new(DEFAULT_SIZE_MB)
    }
}

impl Engine {
    pub fn new(hash_mb: usize) -> Engine {
        let board = notation::starting_position();
        let history = vec![(board.zobrist_hash, board.halfmove_clock)];
        Engine {
            board,
            history,
            time: Arc::new(TimeControl::new()),
            tt: Arc::new(TranspositionTable::new(hash_mb)),
            options: SearchOptions::default(),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn side_to_move(&self) -> Color {
        self.board.side_to_move
    }

    pub fn history_plys(&self) -> usize {
        self.history.len()
    }

    pub fn fen(&self) -> String {
        notation::fen(&self.board, 1)
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        legal_moves(&self.board)
    }

    pub fn stop_handle(&self) -> StopHandle {
        StopHandle(self.time.clone())
    }

    /// Clears the transposition table (UCI `ucinewgame`).
    pub fn reset(&mut self) {
        self.tt.clear();
    }

    pub fn resize_hash(&mut self, size_mb: usize) {
        self.tt = Arc::new(TranspositionTable::new(size_mb));
    }

    pub fn set_position(&mut self, board: Board) {
        self.board = board;
        self.history.clear();
        self.history.push((self.board.zobrist_hash, self.board.halfmove_clock));
    }

    pub fn set_fen(&mut self, fen: &str) -> Result<(), String> {
        self.set_position(notation::board_from_fen(fen)?);
        Ok(())
    }

    /// Plays a move given in UCI notation. Unlike Leorik this rejects illegal moves.
    pub fn play(&mut self, uci: &str) -> Result<(), String> {
        let mv = notation::legal_move_from_uci(&self.board, uci, self.options.variant)?;
        self.play_move(mv);
        Ok(())
    }

    pub fn play_move(&mut self, mv: Move) -> bool {
        let from = self.board.clone();
        if !self.board.play(&from, mv) {
            self.board = from;
            return false;
        }
        self.history.push((self.board.zobrist_hash, self.board.halfmove_clock));
        true
    }

    pub fn flip(&mut self) {
        self.board.flip();
    }

    pub fn perft(&self, depth: usize) -> u64 {
        Perft::new().compute(&self.board, depth)
    }

    /// Default `movestogo` when the GUI doesn't send one.
    pub fn guess_moves_to_go(&self) -> i32 {
        let played_moves = (self.history.len() / 2) as i32;
        (50 - played_moves).max(20)
    }

    fn select_move_history(&self) -> Vec<u64> {
        let mut reps = Vec::with_capacity(self.history.len());
        for &(hash, halfmove_clock) in &self.history {
            if halfmove_clock == 0 {
                reps.clear();
            }
            reps.push(hash);
        }
        reps
    }

    /// Searches the current position, reporting each completed iteration to `on_info`. Blocks until done.
    pub fn go(&mut self, limits: &Limits, mut on_info: impl FnMut(&SearchInfo)) -> SearchResult {
        match limits.clock {
            Some(clock) => {
                let moves_to_go = clock.moves_to_go.unwrap_or_else(|| self.guess_moves_to_go());
                self.time.go_clock(limits.max_depth, clock.time, clock.increment, moves_to_go, limits.ponder)
            }
            None => self.time.go_fixed(limits.max_depth, limits.move_time, limits.ponder),
        }

        self.tt.increase_age();
        let mut options = self.options;
        options.max_nodes = limits.max_nodes;
        let mut search =
            Search::new(&self.board, options, Some(self.select_move_history()), limits.search_moves.clone(), self.tt.clone());

        let time = self.time.clone();
        let kill_switch: KillSwitchFn = Arc::new(move || time.check_time_budget());

        // no legal moves (mate or stalemate): the original would spin here until stopped
        if search.search_moves().is_empty() {
            return SearchResult::default();
        }

        let multi_pv = (options.multi_pv.max(1) as usize).min(search.search_moves().len()) as i32;
        let mut best_move_stability = 0.0f32;
        let mut best = Move::NONE;
        let mut ponder = Move::NONE;

        loop {
            for pv_index in 0..multi_pv {
                search.search_deeper(Some(kill_switch.clone()), pv_index as usize);
                if search.aborted() {
                    break;
                }
                if pv_index == 0 {
                    let pv = search.principal_variation();
                    best = pv.first().copied().unwrap_or(Move::NONE);
                    ponder = pv.get(1).copied().unwrap_or(Move::NONE);
                    best_move_stability = search.stability();
                }
                let info = SearchInfo {
                    depth: search.depth(),
                    multi_pv: pv_index + 1,
                    score: Score::from_raw(self.board.side_to_move.sign() * search.score()),
                    nodes: search.nodes_visited(),
                    time_ms: self.time.elapsed(),
                    pv: self.extended_pv(search.principal_variation(), search.depth()),
                };
                on_info(&info);
            }
            if search.aborted() || !self.time.can_search_deeper(search.depth(), multi_pv, best_move_stability) {
                break;
            }
        }

        // wait until pondering is over
        while self.time.is_pondering() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        SearchResult {
            best_move: if best == Move::NONE { None } else { Some(best) },
            ponder: if ponder == Move::NONE { None } else { Some(ponder) },
        }
    }

    /// The PV, extended with best moves from the transposition table up to `depth` moves.
    fn extended_pv(&self, pv: &[Move], depth: i32) -> Vec<Move> {
        let mut result = Vec::with_capacity(depth.max(0) as usize);
        let mut board = self.board.clone();
        for &mv in pv {
            if !board.play_move(mv) {
                return result;
            }
            result.push(mv);
        }
        while (result.len() as i32) < depth {
            let Some(mv) = self.tt.best_move(board.zobrist_hash) else { break };
            // only follow legal moves (the TT may contain anything for colliding keys)
            if !legal_moves(&board).contains(&mv) || !board.play_move(mv) {
                break;
            }
            result.push(mv);
        }
        result
    }

    pub fn variant(&self) -> Variant {
        self.options.variant
    }
}
