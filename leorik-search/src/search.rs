//! Iterative deepening alpha-beta search (port of Leorik's `IterativeSearch` and `ParallelSearch`).

use crate::history::{History, CONT_DEPTH};
use crate::tt::TranspositionTable;
use leorik_core::dotnet_random::DotNetRandom;
use leorik_core::eval::*;
use leorik_core::movegen::{legal_moves, MoveGen, MAX_MOVES};
use leorik_core::notation::Variant;
use leorik_core::piece::NONE;
use leorik_core::{Board, Move, StaticExchange};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub const MAX_PLY: i32 = 99;
const ASPIRATION_WINDOW: i32 = 40;
const HISTORY_SCALE: f32 = 1.5;

#[derive(Copy, Clone, Debug)]
pub struct SearchOptions {
    pub variant: Variant,
    pub max_nodes: i64,
    pub multi_pv: i32,
    pub threads: i32,
    pub temperature: i32,
    pub seed: i32,
    pub ponder: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions { variant: Variant::Standard, max_nodes: i64::MAX, multi_pv: 1, threads: 1, temperature: 0, seed: -1, ponder: false }
    }
}

/// Returns true when the search should be aborted. Shared between threads.
pub type KillSwitchFn = Arc<dyn Fn() -> bool + Send + Sync>;

struct KillSwitch {
    func: Option<KillSwitchFn>,
    aborted: bool,
    query: u64,
}

impl KillSwitch {
    const QUERY_INTERVAL: u64 = 50;

    fn new(func: Option<KillSwitchFn>) -> KillSwitch {
        let aborted = func.as_ref().map(|f| f()).unwrap_or(false);
        KillSwitch { func, aborted, query: 0 }
    }

    #[inline(always)]
    fn get(&mut self) -> bool {
        if !self.aborted {
            self.query += 1;
            if self.query >= Self::QUERY_INTERVAL {
                if let Some(f) = &self.func {
                    self.query = 0;
                    self.aborted = f();
                }
            }
        }
        self.aborted
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Stage {
    Best,
    Captures,
    Killers,
    Continuation,
    SortedQuiets,
    Quiets,
}

struct PlayState {
    stage: Stage,
    next: usize,
    played_moves: u8,
}

pub struct IterativeSearch {
    root_moves: Vec<Move>,
    positions: Vec<Board>,
    moves: Vec<Move>,
    principal_variations: Vec<Move>,
    root_move_offsets: Vec<i32>,
    history: History,
    see: StaticExchange,
    legacy: Vec<u64>,
    options: SearchOptions,
    tt: Arc<TranspositionTable>,

    best_move_nodes: i64,
    total_nodes: i64,
    kill_switch: KillSwitch,

    eval: i32,
    nodes_visited: i64,
    stability: f32,
    depth: i32,
    aborted: bool,
}

impl IterativeSearch {
    /// `history`: hashes of the positions played before (incl. the current one), used to detect repetitions.
    /// `moves`: restrict the root moves (UCI `searchmoves`); defaults to all legal moves.
    pub fn new(
        board: &Board,
        options: SearchOptions,
        history: Option<Vec<u64>>,
        moves: Option<Vec<Move>>,
        tt: Arc<TranspositionTable>,
    ) -> IterativeSearch {
        let root_moves = moves.unwrap_or_else(|| legal_moves(board));
        const D: usize = MAX_PLY as usize + 1;
        // one spare position/ply worth of moves as a safety margin; never used by the algorithm
        let mut positions: Vec<Board> = (0..MAX_PLY as usize + 1).map(|_| Board::new()).collect();
        positions[0].copy_from(board);

        let mut root_move_offsets = vec![0; root_moves.len()];
        if options.temperature > 0 {
            let seed = if options.seed >= 0 { options.seed } else { random_seed() };
            let mut random = DotNetRandom::new(seed);
            for offset in root_move_offsets.iter_mut() {
                *offset = random.next_max(options.temperature);
            }
        }

        IterativeSearch {
            root_moves,
            positions,
            moves: vec![Move::NONE; (MAX_PLY as usize + 1) * MAX_MOVES],
            principal_variations: vec![Move::NONE; (D * D + D) / 2],
            root_move_offsets,
            history: History::new(),
            see: StaticExchange::new(),
            legacy: history.unwrap_or_default(),
            options,
            tt,
            best_move_nodes: 0,
            total_nodes: 0,
            kill_switch: KillSwitch::new(None),
            eval: 0,
            nodes_visited: 0,
            stability: 0.0,
            depth: 0,
            aborted: false,
        }
    }

    /// Score of the last completed iteration from White's point of view:
    /// normalized centipawns, or a raw mate score (see [`leorik_core::eval::is_checkmate`]).
    pub fn score(&self) -> i32 {
        if is_checkmate(self.eval) {
            self.eval
        } else {
            (self.eval * 100) / NORMALIZE_TO_PAWN_VALUE
        }
    }

    pub fn nodes_visited(&self) -> i64 {
        self.nodes_visited
    }

    pub fn stability(&self) -> f32 {
        self.stability
    }

    pub fn depth(&self) -> i32 {
        self.depth
    }

    pub fn aborted(&self) -> bool {
        self.aborted
    }

    pub fn search_moves(&self) -> &[Move] {
        &self.root_moves
    }

    pub fn principal_variation(&self) -> &[Move] {
        let depth = self.depth.max(0) as usize;
        let pv = &self.principal_variations[..depth];
        let end = pv.iter().position(|&m| m == Move::NONE).unwrap_or(depth);
        &pv[..end]
    }

    pub fn search(&mut self, max_depth: i32) {
        while self.depth < max_depth {
            self.search_deeper(None, 0);
        }
    }

    pub fn search_deeper(&mut self, kill_switch: Option<KillSwitchFn>, first_move: usize) {
        if first_move == 0 {
            self.depth += 1;
        }
        self.best_move_nodes = 0;
        self.total_nodes = 1;
        self.kill_switch = KillSwitch::new(kill_switch);

        let stm = self.positions[0].side_to_move.sign();
        let mut eval = stm * self.eval;
        let mut window = ASPIRATION_WINDOW;
        while !self.aborted {
            let mut alpha = eval - window;
            if self.depth == 1 || is_checkmate(alpha) {
                alpha = -SCORE_BOUNDS;
            }
            let mut beta = eval + window;
            if self.depth == 1 || is_checkmate(beta) {
                beta = SCORE_BOUNDS;
            }

            eval = self.evaluate_root(self.depth, alpha, beta, first_move);

            if eval > alpha && eval < beta {
                self.eval = stm * eval;
                self.stability = self.best_move_nodes as f32 / self.total_nodes as f32;
                return;
            }
            window *= 2;
        }
    }

    #[inline(always)]
    fn play_next(&mut self, ply: usize, mv: Move) -> bool {
        let (head, tail) = self.positions.split_at_mut(ply + 1);
        tail[0].play(&head[ply], mv)
    }

    #[inline(always)]
    fn quick_play_next(&mut self, ply: usize, mv: Move) -> bool {
        let (head, tail) = self.positions.split_at_mut(ply + 1);
        tail[0].quick_play(&head[ply], mv)
    }

    fn evaluate_root(&mut self, depth: i32, mut alpha: i32, beta: i32, first_move: usize) -> i32 {
        self.nodes_visited += 1;

        for i in first_move..self.root_moves.len() {
            let mv = self.root_moves[i];
            if !self.play_next(0, mv) {
                continue;
            }
            let pre_nodes = self.nodes_visited;

            // Scoring root moves with a random bonus: https://www.chessprogramming.org/Ronald_de_Man
            let bonus = if is_checkmate(self.eval) { 0 } else { self.root_move_offsets[i] };
            // moves after the PV move are searched with a null window and, if non-tactical, with reduced depth
            let r = if mv.captured_piece() != NONE || self.positions[1].in_check() { 0 } else { 2 };
            let mut score = alpha;
            if i == first_move || self.evaluate_next(0, depth - r, alpha - bonus, alpha + 1 - bonus, 0) + bonus > alpha {
                score = self.evaluate_next(0, depth, alpha - bonus, beta - bonus, 0) + bonus;
            }

            self.total_nodes += self.nodes_visited - pre_nodes;

            if score > alpha {
                self.best_move_nodes = self.nodes_visited - pre_nodes;
                alpha = score;
                self.extend_pv(0, depth, mv);
                self.promote_best_move(i, first_move);
                if score >= beta {
                    return beta;
                }
            }
        }

        // checkmate or draw?
        if alpha <= -CHECKMATE_SCORE {
            return if self.positions[0].in_check() { mated_score(0) } else { 0 };
        }
        alpha
    }

    #[inline(always)]
    fn evaluate_next(&mut self, ply: usize, remaining: i32, alpha: i32, beta: i32, gen_next: usize) -> i32 {
        -self.evaluate_tt(ply + 1, remaining - 1, -beta, -alpha, gen_next)
    }

    fn evaluate_tt(&mut self, ply: usize, remaining: i32, mut alpha: i32, mut beta: i32, gen_next: usize) -> i32 {
        let cut = self.forced_cut(ply);
        self.aborted |= cut;
        if self.aborted {
            return self.positions[ply].side_to_move_score();
        }

        // mate distance pruning
        let iply = ply as i32;
        alpha = alpha.max(mated_score(iply));
        beta = beta.min(mate_score(iply + 1));
        if alpha >= beta {
            return beta;
        }

        if remaining <= 0 {
            return self.evaluate_quiescence(ply, alpha, beta, gen_next);
        }

        self.truncate_pv(ply);

        // handle draws
        if is_insufficient_mating_material(&self.positions[ply]) {
            return 0;
        }
        if self.positions[ply].halfmove_clock > 99 {
            return 0;
        }
        if self.is_repetition(ply) {
            return 0;
        }

        let hash = self.positions[ply].zobrist_hash;
        let (cutoff, mut bm, tt_score) = self.tt.get_score(hash, remaining, iply, alpha, beta);
        if cutoff {
            return tt_score;
        }
        // Robustness (not in the original): with several threads a torn or colliding entry could carry a
        // move that doesn't fit this position. A verified 64-bit key makes this practically impossible.
        if bm != Move::NONE && !is_plausible(&self.positions[ply], bm) {
            bm = Move::NONE;
        }

        let score = self.evaluate(ply, remaining, alpha, beta, gen_next, &mut bm);
        if self.aborted {
            return score;
        }

        self.tt.store(hash, remaining, iply, alpha, beta, score, bm);

        // no history updates for captures, positions in check and mate scores
        if bm.captured_piece() != NONE || is_checkmate(score) || self.positions[ply].in_check() {
            return score;
        }

        let static_eval = self.history.adjusted_static_eval(&self.positions[ply]);

        // update statistics on good quiet moves
        if score > alpha && bm != Move::NONE && remaining <= 2 {
            self.play_next(ply, bm);
            let next_static_eval = -self.history.adjusted_static_eval(&self.positions[ply + 1]);
            if next_static_eval > static_eval {
                let stm = self.positions[ply].side_to_move;
                self.history.record_quiet_improvement(stm, next_static_eval - static_eval);
            }
        }

        // update correction history
        if !(score <= alpha && score > static_eval) && !(score >= beta && score < static_eval) {
            self.history.update_correction(&self.positions[ply], remaining, score - static_eval);
        }

        score
    }

    fn evaluate(&mut self, ply: usize, remaining: i32, mut alpha: i32, beta: i32, gen_next: usize, best_move: &mut Move) -> i32 {
        self.nodes_visited += 1;

        let in_check = self.positions[ply].in_check();
        let is_null_window = beta <= alpha + 1;
        let static_eval = self.history.adjusted_static_eval(&self.positions[ply]);

        // if we can't expect our last quiet move to make a reasonable difference go into QSearch directly (razoring)
        if !in_check && is_null_window && remaining <= 2 && !is_checkmate(alpha) {
            const P98: f32 = 2.05;
            let razor_margin = self.history.estimate_quiet_improvement_upper_bound(self.positions[ply].side_to_move, P98);
            if static_eval <= alpha - razor_margin {
                return self.evaluate_quiescence(ply, alpha, beta, gen_next);
            }
        }

        // null move pruning
        if !in_check && is_null_window && static_eval > beta && !self.positions[ply].is_endgame() && !is_checkmate(beta) {
            let r = 2 + 2 * (remaining / 4);
            // RFP-like early return if a fail-high is expected anyway
            if remaining <= r + 1 && self.history.is_expected_fail_high(static_eval, beta) {
                return beta;
            }

            {
                let (head, tail) = self.positions.split_at_mut(ply + 1);
                tail[0].play_null_move(&head[ply]);
            }

            if self.evaluate_next(ply, remaining - r, beta - 1, beta, gen_next) >= beta {
                return beta;
            }

            if remaining > r + 1 {
                self.history.null_move_pass(static_eval, beta);
            }
        }

        // staged move generation; play all moves
        let mut gen_end = gen_next;
        let first = MoveGen::new(&mut self.moves, gen_next).collect_move_into(&mut gen_end, *best_move);
        let mut state = PlayState { stage: Stage::Best, next: first, played_moves: 0 };
        while self.play(ply, &mut state, &mut gen_end) {
            // if not PV skip late quiet moves when almost in QSearch depth (late razoring)
            if !in_check && is_null_window && remaining <= 2 && state.stage == Stage::Quiets {
                return alpha;
            }

            let mv = self.moves[state.next - 1];
            self.history.played(ply, remaining, mv);

            // moves after the PV are searched with a null window around alpha expecting a fail-low
            if !in_check && remaining > 1 && state.stage > Stage::Best {
                let max_r = 4.min(remaining - 1);
                let mut r = 0;

                // non-tactical late moves are searched at a reduced depth
                if state.stage >= Stage::Quiets && !self.positions[ply + 1].in_check() {
                    r = 2;
                }

                // a reduced quiet move that doesn't look promising in the static evaluation gets reduced further
                if r < max_r && r > 0 && -self.history.adjusted_static_eval(&self.positions[ply + 1]) < static_eval {
                    r += 2;
                }

                // reduce because of bad SEE
                if r < max_r && self.see.is_bad(&self.positions[ply], mv) {
                    r += 2;
                }

                if self.evaluate_next(ply, remaining - r, alpha, alpha + 1, gen_end) <= alpha {
                    continue;
                }
            }

            // full window search without reduction
            let score = self.evaluate_next(ply, remaining, alpha, beta, gen_end);
            if score <= alpha {
                continue;
            }

            alpha = score;
            *best_move = mv;
            self.extend_pv(ply, remaining, mv);
            self.history.good(ply, remaining, mv);

            if score >= beta {
                return beta;
            }
        }

        // checkmate or draw?
        if state.played_moves == 0 {
            return if in_check { mated_score(ply as i32) } else { 0 };
        }
        alpha
    }

    fn evaluate_quiescence(&mut self, ply: usize, mut alpha: i32, beta: i32, gen_next: usize) -> i32 {
        self.nodes_visited += 1;

        if is_insufficient_mating_material(&self.positions[ply]) {
            return 0;
        }

        let in_check = self.positions[ply].in_check();
        // if in check we can't use stand pat, need to escape check!
        if !in_check {
            let stand_pat = self.history.adjusted_static_eval(&self.positions[ply]);
            if stand_pat >= beta {
                return beta;
            }
            if stand_pat > alpha {
                alpha = stand_pat;
            }
        }

        let cut = self.forced_cut(ply);
        self.aborted |= cut;
        if self.aborted {
            return self.positions[ply].side_to_move_score();
        }

        // play all captures
        let mut moves_played = false;
        let (start, end) = {
            let mut gen = MoveGen::new(&mut self.moves, gen_next);
            let start = gen.collect_captures(&self.positions[ply]);
            (start, gen.next)
        };
        for i in start..end {
            self.pick_best_capture(i, end);

            // skip bad captures when not in check
            if !in_check && self.see.is_bad(&self.positions[ply], self.moves[i]) {
                continue;
            }

            if self.quick_play_next(ply, self.moves[i]) {
                moves_played = true;
                let score = -self.evaluate_quiescence(ply + 1, -beta, -alpha, end);
                if score >= beta {
                    return beta;
                }
                if score > alpha {
                    alpha = score;
                }
            }
        }

        if !in_check {
            return alpha;
        }

        // play quiets only when in check
        let (start, end2) = {
            let mut gen = MoveGen::new(&mut self.moves, end);
            let start = gen.collect_quiets(&self.positions[ply]);
            (start, gen.next)
        };
        for i in start..end2 {
            if self.quick_play_next(ply, self.moves[i]) {
                moves_played = true;
                let score = -self.evaluate_quiescence(ply + 1, -beta, -alpha, end2);
                if score >= beta {
                    return beta;
                }
                if score > alpha {
                    alpha = score;
                }
            }
        }

        if moves_played {
            alpha
        } else {
            mated_score(ply as i32)
        }
    }

    #[inline(always)]
    fn play(&mut self, ply: usize, state: &mut PlayState, gen_end: &mut usize) -> bool {
        loop {
            if state.next == *gen_end {
                match state.stage {
                    Stage::Best => {
                        let mut gen = MoveGen::new(&mut self.moves, *gen_end);
                        state.next = gen.collect_captures(&self.positions[ply]);
                        *gen_end = gen.next;
                        state.stage = Stage::Captures;
                        continue;
                    }
                    Stage::Captures => {
                        let mut gen = MoveGen::new(&mut self.moves, *gen_end);
                        state.next = gen.collect_quiets(&self.positions[ply]);
                        *gen_end = gen.next;
                        state.stage = Stage::Killers;
                        continue;
                    }
                    _ => return false,
                }
            }

            let end = *gen_end;
            match state.stage {
                Stage::Captures => self.pick_best_capture(state.next, end),
                Stage::Killers => self.pick_killer(state, ply, end),
                Stage::Continuation => self.pick_continuation(state, ply, end),
                Stage::SortedQuiets => self.pick_best_history(state, end),
                _ => {}
            }

            let mv = self.moves[state.next];
            state.next += 1;
            if self.play_next(ply, mv) {
                state.played_moves = state.played_moves.wrapping_add(1);
                return true;
            }
        }
    }

    #[inline(always)]
    fn pick_killer(&mut self, state: &mut PlayState, ply: usize, end: usize) {
        state.stage = Stage::Continuation;
        if self.pick_move(state.next, end, self.history.killer(ply)) {
            return;
        }
        self.pick_continuation(state, ply, end);
    }

    #[inline(always)]
    fn pick_continuation(&mut self, state: &mut PlayState, ply: usize, end: usize) {
        for i in 0..CONT_DEPTH {
            if self.pick_move(state.next, end, self.history.continuation(ply, i)) {
                return;
            }
        }
        self.pick_best_history(state, end);
    }

    #[inline(always)]
    fn is_repetition(&self, ply: usize) -> bool {
        let hash = self.positions[ply].zobrist_hash;
        // start with the positions we've been searching
        let mut i = ply as isize - 4;
        while i >= 0 {
            let p = &self.positions[i as usize];
            if p.zobrist_hash == hash {
                return true;
            }
            // captures and pawn moves reset the halfmove clock and make a repetition impossible
            if p.halfmove_clock <= 1 {
                return false;
            }
            i -= 2;
        }
        // continue with the history of positions from the start position, truncated based on the halfmove clock
        let mut i = self.legacy.len() as isize - 1 - (ply as isize & 1);
        while i >= 0 {
            if self.legacy[i as usize] == hash {
                return true;
            }
            i -= 2;
        }
        false
    }

    #[inline(always)]
    fn pick_move(&mut self, first: usize, end: usize, mv: Move) -> bool {
        if mv == Move::NONE {
            return false;
        }
        for i in first + 1..end {
            if self.moves[i] == mv {
                self.moves.swap(first, i);
                return true;
            }
        }
        false
    }

    #[inline(always)]
    fn pick_best_history(&mut self, state: &mut PlayState, end: usize) {
        let first = state.next;
        let mut best = first;
        let mut best_score = self.history.value(self.moves[best]);
        for i in first + 1..end {
            let score = self.history.value(self.moves[i]);
            if score > best_score {
                best = i;
                best_score = score;
            }
        }
        if best != first {
            self.moves.swap(first, best);
        }
        let threshold = HISTORY_SCALE * state.played_moves as f32;
        state.stage = if best_score > threshold { Stage::SortedQuiets } else { Stage::Quiets };
    }

    #[inline(always)]
    fn pick_best_capture(&mut self, first: usize, end: usize) {
        let mut best = first;
        let mut best_score = self.moves[first].mvv_lva_score();
        for i in first + 1..end {
            let score = self.moves[i].mvv_lva_score();
            if score >= best_score {
                best = i;
                best_score = score;
            }
        }
        if best != first {
            self.moves.swap(first, best);
        }
    }

    #[inline(always)]
    fn forced_cut(&mut self, ply: usize) -> bool {
        self.depth > 1
            && (ply as i32 >= MAX_PLY - 1 || self.nodes_visited >= self.options.max_nodes || self.kill_switch.get())
    }

    #[inline(always)]
    fn index_pv(&self, ply: usize) -> usize {
        let ply = ply as i32;
        (self.depth * ply - (ply * ply - ply) / 2) as usize
    }

    #[inline(always)]
    fn extend_pv(&mut self, ply: usize, remaining: i32, mv: Move) {
        let index = self.index_pv(ply);
        self.principal_variations[index] = mv;
        let stride = (self.depth - ply as i32) as usize;
        let from = index + stride - 1;
        let remaining = remaining.max(0) as usize;
        for i in 1..remaining {
            self.principal_variations[index + i] = self.principal_variations[from + i];
        }
        for i in remaining..stride {
            self.principal_variations[index + i] = Move::NONE;
        }
    }

    #[inline(always)]
    fn truncate_pv(&mut self, ply: usize) {
        let index = self.index_pv(ply);
        self.principal_variations[index] = Move::NONE;
    }

    fn promote_best_move(&mut self, from: usize, to: usize) {
        if from <= to {
            return;
        }
        let mv = self.root_moves[from];
        let offset = self.root_move_offsets[from];
        for j in (to + 1..=from).rev() {
            self.root_moves[j] = self.root_moves[j - 1];
            self.root_move_offsets[j] = self.root_move_offsets[j - 1];
        }
        self.root_moves[to] = mv;
        self.root_move_offsets[to] = offset;
    }
}

trait CollectInto {
    fn collect_move_into(self, end: &mut usize, mv: Move) -> usize;
}

impl CollectInto for MoveGen<'_> {
    #[inline(always)]
    fn collect_move_into(mut self, end: &mut usize, mv: Move) -> usize {
        let old = self.collect_move(mv);
        *end = self.next;
        old
    }
}

/// Cheap sanity check that a move taken from the transposition table fits the position.
#[inline(always)]
fn is_plausible(board: &Board, mv: Move) -> bool {
    use leorik_core::piece::{COLOR_MASK, KING, TYPE_MASK, WHITE};
    let mover = mv.moving_piece();
    let own = if board.side_to_move == leorik_core::Color::White { WHITE } else { leorik_core::piece::BLACK };
    if mv.from >= 64 || mv.to >= 64 || (mover & COLOR_MASK) != own || board.get_piece(mv.from as usize) != mover {
        return false;
    }
    if mv.is_en_passant() {
        return mv.target == NONE && (board.en_passant >> mv.to) & 1 == 1;
    }
    (mv.target & TYPE_MASK) != KING && board.get_piece(mv.to as usize) == mv.target
}

fn random_seed() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (nanos as u64 ^ (nanos >> 64) as u64) as i32 & i32::MAX
}

/// Lazy SMP: several independent searches sharing the transposition table.
pub struct ParallelSearch {
    workers: Vec<IterativeSearch>,
    best: AtomicUsize,
}

impl ParallelSearch {
    pub fn new(
        board: &Board,
        options: SearchOptions,
        history: Option<Vec<u64>>,
        moves: Option<Vec<Move>>,
        tt: Arc<TranspositionTable>,
    ) -> ParallelSearch {
        let workers = (0..options.threads.max(1))
            .map(|_| IterativeSearch::new(board, options, history.clone(), moves.clone(), tt.clone()))
            .collect();
        ParallelSearch { workers, best: AtomicUsize::new(0) }
    }

    fn best(&self) -> &IterativeSearch {
        &self.workers[self.best.load(Ordering::Relaxed)]
    }

    pub fn search_deeper(&mut self, kill_switch: Option<KillSwitchFn>, first_move: usize) {
        let best = &self.best;
        std::thread::scope(|s| {
            for (i, worker) in self.workers.iter_mut().enumerate() {
                let ks = kill_switch.clone();
                std::thread::Builder::new()
                    .stack_size(8 << 20)
                    .spawn_scoped(s, move || {
                        worker.search_deeper(ks, first_move);
                        best.store(i, Ordering::Relaxed);
                    })
                    .expect("failed to spawn search thread");
            }
        });
    }

    pub fn search(&mut self, max_depth: i32) {
        let best = &self.best;
        std::thread::scope(|s| {
            for (i, worker) in self.workers.iter_mut().enumerate() {
                std::thread::Builder::new()
                    .stack_size(8 << 20)
                    .spawn_scoped(s, move || {
                        worker.search(max_depth);
                        best.store(i, Ordering::Relaxed);
                    })
                    .expect("failed to spawn search thread");
            }
        });
    }
}

/// Either a single-threaded or a parallel search, mirroring Leorik's `ISearch`.
pub enum Search {
    Single(Box<IterativeSearch>),
    Parallel(ParallelSearch),
}

impl Search {
    pub fn new(
        board: &Board,
        options: SearchOptions,
        history: Option<Vec<u64>>,
        moves: Option<Vec<Move>>,
        tt: Arc<TranspositionTable>,
    ) -> Search {
        if options.threads > 1 {
            Search::Parallel(ParallelSearch::new(board, options, history, moves, tt))
        } else {
            Search::Single(Box::new(IterativeSearch::new(board, options, history, moves, tt)))
        }
    }

    fn current(&self) -> &IterativeSearch {
        match self {
            Search::Single(s) => s,
            Search::Parallel(p) => p.best(),
        }
    }

    pub fn search_deeper(&mut self, kill_switch: Option<KillSwitchFn>, first_move: usize) {
        match self {
            Search::Single(s) => s.search_deeper(kill_switch, first_move),
            Search::Parallel(p) => p.search_deeper(kill_switch, first_move),
        }
    }

    pub fn search(&mut self, max_depth: i32) {
        match self {
            Search::Single(s) => s.search(max_depth),
            Search::Parallel(p) => p.search(max_depth),
        }
    }

    pub fn aborted(&self) -> bool {
        self.current().aborted()
    }
    pub fn depth(&self) -> i32 {
        self.current().depth()
    }
    pub fn score(&self) -> i32 {
        self.current().score()
    }
    pub fn stability(&self) -> f32 {
        self.current().stability()
    }
    pub fn principal_variation(&self) -> &[Move] {
        self.current().principal_variation()
    }
    pub fn search_moves(&self) -> &[Move] {
        self.current().search_moves()
    }
    pub fn nodes_visited(&self) -> i64 {
        match self {
            Search::Single(s) => s.nodes_visited(),
            Search::Parallel(p) => p.workers.iter().map(|w| w.nodes_visited()).sum(),
        }
    }
}
