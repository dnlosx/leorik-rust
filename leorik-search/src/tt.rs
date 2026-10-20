//! Lock-free transposition table with two-entry buckets and aging (port of Leorik's `Transpositions`).
//!
//! Each entry is stored as two 64-bit words, `key ^ data` and `data`, so a torn write from another
//! thread is detected as a miss (the classic "xor trick" that Leorik also uses).

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering::Relaxed};
use leorik_core::eval::is_checkmate;
use leorik_core::Move;

pub const DEFAULT_SIZE_MB: usize = 50;
const ENTRY_SIZE: usize = 16;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ScoreType {
    GreaterOrEqual = 0,
    LessOrEqual = 1,
    Exact = 2,
}

#[derive(Default)]
struct Slot {
    key: AtomicU64,
    data: AtomicU64,
}

/// Unpacked view of an entry's data word (same byte layout as Leorik's `HashEntry`).
#[derive(Copy, Clone, Default)]
struct Entry {
    key: u64,
    score: i16,
    depth: u8,
    age: u8,
    move_and_type: i32,
}

impl Entry {
    #[inline(always)]
    fn data(&self) -> u64 {
        (self.score as u16 as u64) | (self.depth as u64) << 16 | (self.age as u64) << 24 | (self.move_and_type as u32 as u64) << 32
    }

    #[inline(always)]
    fn from_words(key: u64, data: u64) -> Entry {
        Entry { key, score: data as u16 as i16, depth: (data >> 16) as u8, age: (data >> 24) as u8, move_and_type: (data >> 32) as u32 as i32 }
    }

    #[inline(always)]
    fn hash(&self) -> u64 {
        self.key ^ self.data()
    }

    #[inline(always)]
    fn set_hash(&mut self, hash: u64) {
        self.key = hash ^ self.data();
    }

    #[inline(always)]
    fn score_type(&self) -> ScoreType {
        match self.move_and_type >> 29 {
            0 => ScoreType::GreaterOrEqual,
            1 => ScoreType::LessOrEqual,
            _ => ScoreType::Exact,
        }
    }

    #[inline(always)]
    fn best_move(&self) -> Move {
        Move::from_bits((self.move_and_type & 0x1FFF_FFFF) as u32)
    }
}

pub struct TranspositionTable {
    table: Vec<Slot>,
    age: AtomicU8,
}

#[inline(always)]
fn encode(mv: Move, score_type: ScoreType) -> i32 {
    // a move can be represented in 29 bits, leaving room for the score type
    mv.to_bits() as i32 | (score_type as i32) << 29
}

#[inline(always)]
fn adjust_mate_distance(score: i32, ply: i32) -> i16 {
    // mate scores are stored relative to the position, not the root
    if is_checkmate(score) {
        (score + score.signum() * ply) as i16
    } else {
        score as i16
    }
}

impl Default for TranspositionTable {
    fn default() -> Self {
        TranspositionTable::new(DEFAULT_SIZE_MB)
    }
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> TranspositionTable {
        let length = ((size_mb.max(1) * 1024 * 1024) / ENTRY_SIZE).max(2);
        let mut table = Vec::with_capacity(length);
        table.resize_with(length, Slot::default);
        TranspositionTable { table, age: AtomicU8::new(0) }
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub fn clear(&self) {
        self.age.store(0, Relaxed);
        for slot in &self.table {
            slot.key.store(0, Relaxed);
            slot.data.store(0, Relaxed);
        }
    }

    pub fn increase_age(&self) {
        self.age.fetch_add(1, Relaxed);
    }

    #[inline(always)]
    fn load(&self, index: usize) -> Entry {
        let slot = &self.table[index];
        Entry::from_words(slot.key.load(Relaxed), slot.data.load(Relaxed))
    }

    #[inline(always)]
    fn save(&self, index: usize, entry: &Entry) {
        let slot = &self.table[index];
        slot.key.store(entry.key, Relaxed);
        slot.data.store(entry.data(), Relaxed);
    }

    #[inline(always)]
    fn index(&self, hash: u64) -> usize {
        (hash % self.table.len() as u64) as usize
    }

    #[allow(clippy::too_many_arguments)]
    pub fn store(&self, hash: u64, depth: i32, ply: i32, alpha: i32, beta: i32, score: i32, mut best_move: Move) {
        let age = self.age.load(Relaxed);
        let mut index = self.index(hash);
        let mut entry = self.load(index);
        if entry.hash() != hash {
            let other = self.load(index ^ 1);
            // other slot is correct -OR- both are wrong but other is shallower -OR- undercut replacement
            if other.hash() == hash
                || (entry.depth >= other.depth && entry.depth as i32 + entry.age as i32 > depth + 1 + age as i32)
            {
                index ^= 1;
                entry = other;
            }
        }

        // don't overwrite a bestmove with 'default' unless it's a new position
        if best_move == Move::NONE && entry.hash() == hash {
            best_move = entry.best_move();
        }

        entry.depth = if depth < 0 { 0 } else { depth as u8 };
        entry.age = age;

        if score >= beta {
            entry.move_and_type = encode(best_move, ScoreType::GreaterOrEqual);
            entry.score = adjust_mate_distance(beta, ply);
        } else if score <= alpha {
            entry.move_and_type = encode(best_move, ScoreType::LessOrEqual);
            entry.score = adjust_mate_distance(alpha, ply);
        } else {
            entry.move_and_type = encode(best_move, ScoreType::Exact);
            entry.score = adjust_mate_distance(score, ply);
        }

        entry.set_hash(hash);
        self.save(index, &entry);
    }

    pub fn best_move(&self, hash: u64) -> Option<Move> {
        let index = self.index(hash);
        for i in [index, index ^ 1] {
            let entry = self.load(i);
            if entry.hash() == hash {
                let mv = entry.best_move();
                return if mv == Move::NONE { None } else { Some(mv) };
            }
        }
        None
    }

    /// Returns `(cutoff, best_move, score)`: `cutoff` is true when the stored score can be returned directly.
    #[inline]
    pub fn get_score(&self, hash: u64, depth: i32, ply: i32, alpha: i32, beta: i32) -> (bool, Move, i32) {
        let mut index = self.index(hash);
        let mut entry = self.load(index);
        if entry.hash() != hash {
            index ^= 1;
            entry = self.load(index);
        }
        if entry.hash() != hash {
            return (false, Move::NONE, 0);
        }

        // a table hit resets the age
        entry.age = self.age.load(Relaxed);
        entry.set_hash(hash);
        self.save(index, &entry);

        let best_move = entry.best_move();
        if (entry.depth as i32) < depth {
            return (false, best_move, 0);
        }

        let score = adjust_mate_distance(entry.score as i32, -ply) as i32;
        let cutoff = match entry.score_type() {
            ScoreType::GreaterOrEqual => score >= beta,
            ScoreType::LessOrEqual => score <= alpha,
            ScoreType::Exact => true,
        };
        (cutoff, best_move, score)
    }
}
