//! Search, transposition table and time management of the
//! [Leorik](https://github.com/lithander/Leorik) chess engine, ported to Rust.

pub mod engine;
pub mod history;
pub mod search;
pub mod time;
pub mod tt;

pub use engine::{Clock, Engine, Limits, Score, SearchInfo, SearchResult, StopHandle};
pub use search::{IterativeSearch, Search, SearchOptions, MAX_PLY};
pub use tt::TranspositionTable;
