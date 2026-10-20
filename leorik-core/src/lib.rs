//! Board representation, move generation and NNUE evaluation of the
//! [Leorik](https://github.com/lithander/Leorik) chess engine, ported to Rust.
//!
//! This crate does no I/O and spawns no threads.

pub mod bitboard;
pub mod board;
pub mod dotnet_random;
pub mod eval;
pub mod movegen;
pub mod moves;
pub mod network;
pub mod nnue;
pub mod notation;
pub mod perft;
pub mod piece;
mod tables;
pub mod zobrist;

pub use board::Board;
pub use eval::see::StaticExchange;
pub use moves::Move;
pub use notation::Variant;
pub use piece::{Color, Piece};
