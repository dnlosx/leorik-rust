//! UCI front end for the Leorik Rust port (port of Leorik's `Program` and `Uci`).

use leorik_core::notation::{self, move_name, Variant};
use leorik_core::Move;
use leorik_search::tt::DEFAULT_SIZE_MB;
use leorik_search::{Clock, Engine, Limits, Score, SearchInfo, SearchOptions, StopHandle, MAX_PLY};
use std::io::{self, BufRead, Write};
use std::thread::JoinHandle;

const NAME_VERSION: &str = concat!("Leorik ", env!("CARGO_PKG_VERSION"), " (Rust port)");
const AUTHOR: &str = "Thomas Jahn (Rust port contributors)";

struct Uci {
    engine: Option<Engine>,
    searching: Option<JoinHandle<Engine>>,
    stop: StopHandle,
}

fn write(line: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn join_moves(moves: &[Move], variant: Variant) -> String {
    moves.iter().map(|&m| move_name(m, variant)).collect::<Vec<_>>().join(" ")
}

fn print_info(info: &SearchInfo, variant: Variant) {
    let score = match info.score {
        Score::Cp(cp) => format!("cp {cp}"),
        Score::Mate(m) => format!("mate {m}"),
    };
    let t = info.time_ms.max(1) as f64 / 1000.0;
    let nps = (info.nodes as f64 / t) as i64;
    write(&format!(
        "info depth {} multipv {} score {} nodes {} nps {} time {} pv {}",
        info.depth,
        info.multi_pv,
        score,
        info.nodes,
        nps,
        info.time_ms,
        join_moves(&info.pv, variant)
    ));
}

fn token<'a>(tokens: &[&'a str], name: &str) -> Option<&'a str> {
    let i = tokens.iter().position(|&t| t == name)?;
    tokens.get(i + 1).copied()
}

fn parse<T: std::str::FromStr>(tokens: &[&str], name: &str) -> Option<T> {
    token(tokens, name).and_then(|v| v.parse().ok())
}

impl Uci {
    fn new() -> Uci {
        let engine = Engine::new(DEFAULT_SIZE_MB);
        let stop = engine.stop_handle();
        Uci { engine: Some(engine), searching: None, stop }
    }

    /// Stops a running search (if any) and gives back the engine.
    fn engine(&mut self) -> &mut Engine {
        if let Some(handle) = self.searching.take() {
            self.stop.stop();
            self.engine = Some(handle.join().expect("search thread panicked"));
        }
        self.engine.as_mut().unwrap()
    }

    fn handle(&mut self, input: &str) -> bool {
        let tokens: Vec<&str> = input.split_whitespace().collect();
        let Some(&command) = tokens.first() else { return true };
        match command {
            "uci" => {
                write(&format!("id name {NAME_VERSION}"));
                write(&format!("id author {AUTHOR}"));
                let defaults = SearchOptions::default();
                write(&format!("option name Hash type spin default {DEFAULT_SIZE_MB} min 1 max 2047"));
                write(&format!("option name Threads type spin default {} min 1 max 32", defaults.threads));
                write(&format!("option name Temperature type spin default {} min 0 max 1000", defaults.temperature));
                write("option name Clear Hash type button");
                write("option name UCI_Chess960 type check default false");
                write("option name Ponder type check default false");
                write("option name MultiPV type spin default 1 min 1 max 256");
                write("uciok");
            }
            "isready" => write("readyok"),
            "position" => self.position(&tokens),
            "go" => self.go(&tokens),
            "ucinewgame" => self.engine().reset(),
            "stop" => {
                self.engine();
            }
            "ponderhit" => self.stop.ponderhit(),
            "quit" => {
                self.engine();
                return false;
            }
            "setoption" => self.set_option(&tokens),
            // unofficial commands
            "fen" => write(&self.engine().fen()),
            "hash" => write(&self.engine().board().zobrist_hash.to_string()),
            "eval" => write(&self.engine().board().eval.score.to_string()),
            "perft" => match tokens.get(1).map(|d| d.parse::<usize>()) {
                Some(Ok(depth)) if depth < 10 => write(&self.engine().perft(depth).to_string()),
                Some(_) => write(&format!("Invalid depth: {}", tokens[1])),
                None => write("Usage: perft <depth>"),
            },
            "flip" => self.engine().flip(),
            "moves" => {
                let engine = self.engine();
                write(&join_moves(&engine.legal_moves(), engine.variant()));
            }
            _ => write(&format!("Unknown command: {input}")),
        }
        true
    }

    fn set_option(&mut self, tokens: &[&str]) {
        // setoption name <id> [value <x>]
        let name_at = tokens.iter().position(|&t| t == "name");
        let value_at = tokens.iter().position(|&t| t == "value");
        let name = match (name_at, value_at) {
            (Some(n), Some(v)) if v > n => tokens[n + 1..v].join(" "),
            (Some(n), _) => tokens[n + 1..].join(" "),
            _ => String::new(),
        };
        let value = value_at.and_then(|v| tokens.get(v + 1)).copied().unwrap_or("");
        let engine = self.engine();
        match (name.as_str(), value) {
            ("Hash", v) if v.parse::<usize>().is_ok() => engine.resize_hash(v.parse().unwrap()),
            ("Threads", v) if v.parse::<i32>().is_ok() => engine.options.threads = v.parse::<i32>().unwrap().clamp(1, 256),
            ("Clear Hash", _) => engine.reset(),
            ("Temperature", v) if v.parse::<i32>().is_ok() => engine.options.temperature = v.parse::<i32>().unwrap().max(0),
            ("UCI_Chess960", v) if v.parse::<bool>().is_ok() => {
                engine.options.variant = if v.parse().unwrap() { Variant::Chess960 } else { Variant::Standard }
            }
            ("Ponder", v) if v.parse::<bool>().is_ok() => engine.options.ponder = v.parse().unwrap(),
            ("MultiPV", v) if v.parse::<i32>().is_ok() => engine.options.multi_pv = v.parse::<i32>().unwrap().max(1),
            _ => write(&format!("Unknown UCI option: {}", tokens.get(2..).map(|t| t.join(" ")).unwrap_or_default())),
        }
    }

    fn position(&mut self, tokens: &[&str]) {
        let engine = self.engine();
        let moves_at = tokens.iter().position(|&t| t == "moves");
        match tokens.get(1) {
            Some(&"startpos") => engine.set_position(notation::starting_position()),
            Some(&"fen") => {
                let end = moves_at.unwrap_or(tokens.len());
                let fen = tokens[2.min(end)..end].join(" ");
                if let Err(e) = engine.set_fen(&fen) {
                    write(&format!("info string Invalid FEN: {e}"));
                    return;
                }
            }
            other => {
                write(&format!("Unknown parameter: {}", other.unwrap_or(&"")));
                return;
            }
        }
        if let Some(i) = moves_at {
            for uci in &tokens[i + 1..] {
                if let Err(e) = engine.play(uci) {
                    write(&format!("info string {e}"));
                    break;
                }
            }
        }
    }

    fn go(&mut self, tokens: &[&str]) {
        let engine = self.engine();
        let pondering = tokens.contains(&"ponder");
        let mut limits = Limits {
            max_depth: parse::<i32>(tokens, "depth").unwrap_or(MAX_PLY),
            move_time: parse::<i32>(tokens, "movetime").unwrap_or(i32::MAX),
            max_nodes: parse::<i64>(tokens, "nodes").unwrap_or(i64::MAX),
            clock: None,
            search_moves: None,
            ponder: pondering,
        };
        let moves_to_go = parse::<i32>(tokens, "movestogo");
        if let Some(i) = tokens.iter().position(|&t| t == "searchmoves") {
            let mut moves = Vec::new();
            for uci in &tokens[i + 1..] {
                match notation::move_from_uci(engine.board(), uci, engine.variant()) {
                    Ok(mv) => moves.push(mv),
                    Err(_) => write(&format!("Invalid Move: {uci} is not viable in this position!")),
                }
            }
            limits.search_moves = Some(moves);
        }
        let (time_key, inc_key) =
            if engine.side_to_move() == leorik_core::Color::White { ("wtime", "winc") } else { ("btime", "binc") };
        if let Some(time) = parse::<i32>(tokens, time_key) {
            let increment = parse::<i32>(tokens, inc_key).unwrap_or(0);
            limits.clock = Some(Clock { time, increment, moves_to_go });
        }

        let mut engine = self.engine.take().unwrap();
        self.stop = engine.stop_handle();
        let handle = std::thread::Builder::new()
            .name("search".into())
            .stack_size(16 << 20)
            .spawn(move || {
                let variant = engine.variant();
                let result = engine.go(&limits, |info| print_info(info, variant));
                let best = result.best_move.map(|m| move_name(m, variant)).unwrap_or_else(|| "0000".to_string());
                match result.ponder {
                    Some(p) => write(&format!("bestmove {best} ponder {}", move_name(p, variant))),
                    None => write(&format!("bestmove {best}")),
                }
                engine
            })
            .expect("failed to spawn search thread");
        self.searching = Some(handle);
    }
}

fn main() {
    write(NAME_VERSION);
    let mut uci = Uci::new();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if !uci.handle(line.trim()) {
            return;
        }
    }
    // stdin closed: finish any running search before exiting
    uci.engine();
}
