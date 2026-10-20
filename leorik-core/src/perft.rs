//! Perft (move path enumeration), used to verify the move generator.

use crate::board::Board;
use crate::movegen::{MoveGen, MAX_MOVES};
use crate::moves::Move;

const MAX_PLY: usize = 10;

pub struct Perft {
    positions: Vec<Board>,
    moves: Vec<Move>,
}

impl Default for Perft {
    fn default() -> Self {
        Perft::new()
    }
}

impl Perft {
    pub fn new() -> Perft {
        Perft { positions: (0..=MAX_PLY).map(|_| Board::new()).collect(), moves: vec![Move::NONE; MAX_PLY * MAX_MOVES] }
    }

    pub fn compute(&mut self, position: &Board, depth: usize) -> u64 {
        assert!(depth < MAX_PLY, "perft depth must be below {MAX_PLY}");
        if depth == 0 {
            return 1;
        }
        self.positions[0].copy_from(position);
        count(&mut self.positions, &mut self.moves, 0, depth)
    }
}

fn count(positions: &mut [Board], moves: &mut [Move], first: usize, remaining: usize) -> u64 {
    let (current, rest) = positions.split_first_mut().unwrap();
    let mut gen = MoveGen::new(moves, first);
    gen.collect_all(current);
    let end = gen.next;
    let mut sum = 0;
    for i in first..end {
        let mv = moves[i];
        if rest[0].play_without_hash_and_eval(current, mv) {
            if remaining > 1 {
                sum += count(rest, moves, end, remaining - 1);
            } else {
                sum += 1;
            }
        }
    }
    sum
}
