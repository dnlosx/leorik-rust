# Leorik (Rust port)

A Rust port of [**Leorik**](https://github.com/lithander/Leorik), the MIT-licensed chess engine
by Thomas Jahn. The ported version is **Leorik 3.2** (git tag `3.2`, commit `6f41d23`), about
3400 Elo on the CCRL Blitz list for 3.1.

The port is meant to be faithful: same board representation, move generator, NNUE network,
search, pruning parameters and time management. At a fixed depth or node count it searches
**exactly the same tree** as the original: identical node counts, scores, principal variations
and best moves. It can be built as a UCI engine or as a static library with a small C API,
including an XCFramework for iOS.

## License

MIT, see [LICENSE](LICENSE). The original copyright notice (Copyright (c) 2023 Thomas Jahn) is
kept, with an additional notice for the port.

The NNUE network `leorik-core/net/640HL-S-5io8-6116M-FRCv1.nnue` is the network that Leorik 3.2
loads (`Leorik.Core/640HL-S-5io8-6116M-FRCv1.nnue`, added in commit `803d4bc`, SHA-256
`8d1ddff989e0172e9a7b02b745c5bf7e276963f7430c805d19e9e612109284be`). It's committed in the
Leorik repository, which is MIT-licensed as a whole. The README there says it was trained with
[bullet](https://github.com/jw1912/bullet) on Leorik's own self-play games. There is no separate
license statement for the network file, so it is covered by the repository's MIT license
like everything else in it. *This is a reading of the repository, not legal advice.*

## Workspace

| Crate | Contents |
|---|---|
| `leorik-core` | Board (copy-make), pseudo-legal move generation, Zobrist hashing, SEE, FEN/UCI notation, perft, NNUE evaluation with the network embedded via `include_bytes!`. No I/O, no threads. |
| `leorik-search` | Iterative deepening PVS search, history/killer/continuation/correction tables, lock-free transposition table, Lazy SMP, time management, and an `Engine` driver (position + history + `go`). |
| `leorik-uci` | The `leorik` UCI binary (same commands and options as the original, incl. the unofficial `eval`, `hash`, `fen`, `perft`, `moves`, `flip`). |
| `leorik-ffi` | `libleorik` static library with a C API ([`leorik.h`](leorik-ffi/include/leorik.h)). |

The NNUE accumulator updates and the SCReLU output layer use NEON intrinsics
(`std::arch::aarch64`) on aarch64, with a scalar fallback elsewhere. Both produce bit-identical
results to the original (16-bit wrapping accumulators, the same `(a·w)·a` trick as Leorik's AVX2
path; a test checks that the network's output weights make that trick exact).

## Building

```bash
cargo build --release            # target/release/leorik (UCI) and target/release/libleorik.a
cargo test --release             # unit tests incl. perft, eval/hash checks, C API tests
scripts/test-c-api.sh            # compiles and runs a C program against libleorik.a
```

iOS (device and Apple-silicon simulator):

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
scripts/build-xcframework.sh     # -> build/Leorik.xcframework (headers + module map "Leorik")
scripts/test-ios-sim.sh          # runs the C API test inside a booted iOS simulator
```

## C API

```c
LeorikEngine *e = leorik_new(/*hash_mb*/ 0, /*threads*/ 1);       // 0 = default 50 MB
leorik_set_position(e, NULL, "e2e4 e7e5 g1f3");                  // FEN or NULL/"startpos", then moves
char move[LEORIK_MOVE_LEN];
leorik_best_move(e, /*time_ms*/ 1000, /*depth*/ 0, move);         // "g8f6"
LeorikAnalysis a;
leorik_analyze(e, 0, 14, &a, on_info, user_data);                 // score_cp/mate (side to move),
                                                                  // white_score_cp/white_mate, pv
leorik_stop(e);                                                   // from any thread, non-blocking
leorik_free(e);
```

Calls on one handle are serialized; `leorik_stop` can be called from another thread to end a
running search early (it returns the best move of the last completed iteration). Invalid FENs and
illegal moves return `LEORIK_ERR_POSITION` and leave the previous position in place; positions
without legal moves return `LEORIK_ERR_GAME_OVER`. Panics never cross the API boundary.

## Verification

All numbers below were produced on an Apple M5 (10 cores) running macOS. The original was built
from the `3.2` tag with the .NET 10 SDK (`dotnet publish -c Release -r osx-arm64 --self-contained`).
The scripts in [`scripts/`](scripts) reproduce everything.

### 1. Perft

`scripts/perft-table.sh` (the full suites from the Leorik repository, `testdata/perft.txt` with 127
positions and `testdata/perft960.epd` with 964 Chess960 positions to depth 5, also pass:
`cargo test --release -p leorik-core -- --ignored`).

| Position | Depth | Nodes | Expected | Time (s) |
|---|---|---|---|---|
| Start position | 6 | 119060324 | 119060324 ✓ | 3.623 |
| Kiwipete | 5 | 193690690 | 193690690 ✓ | 5.389 |
| Position 3 | 6 | 11030083 | 11030083 ✓ | .308 |
| Position 4 | 5 | 15833292 | 15833292 ✓ | .383 |
| Position 4 (mirrored) | 5 | 15833292 | 15833292 ✓ | .341 |
| Position 5 | 5 | 89941194 | 89941194 ✓ | 1.830 |
| Position 6 | 5 | 164075551 | 164075551 ✓ | 4.162 |

### 2. Evaluation parity

`scripts/parity.py --mode eval` against the original on 300 FENs (`testdata/parity_fens.txt`:
openings, middlegames, endgames, castling rights, en passant, promotions, checks) plus 837
positions reached from them by playing random legal moves (testing the incremental NNUE and Zobrist
updates). Compared: the NNUE evaluation (`eval`), the Zobrist hash (`hash`), the FEN and the list
of legal moves including its order.

| Check | Equal |
|---|---|
| Root positions: eval, hash, FEN, legal moves (set and order) | 300 / 300 |
| After random moves (incremental updates): eval, hash, FEN, legal moves | 837 / 837 |
| Incremental vs. from-scratch evaluation, each engine | 837 / 837 |

### 3. Search parity

`scripts/parity.py --mode search --depth 12`: fresh hash table (`ucinewgame`, 50 MB like the
original's default), then `go depth 12` on each of the 300 positions (294 with legal moves).

| Run | Positions | Best move, depth, score, nodes, PV equal | Total nodes (each engine) |
|---|---|---|---|
| `--depth 12`, all 300 FENs | 294 | 294 / 294 | 17,363,622 |
| `--depth 15`, first 100 FENs | 94 | 94 / 94 | 20,539,526 |

Chess960 castling and MultiPV were also spot-checked: the output of both engines (without the
`nps`/`time` fields) is byte-identical for MultiPV 3 on a Chess960 position and for Kiwipete in
Chess960 mode.

### 4. Strength

[fastchess](https://github.com/Disservin/fastchess), 200 self-generated balanced openings
(`testdata/openings.epd`), each played with both colors, 16 MB hash, 4 games in parallel.

**Fixed nodes** (`scripts/match.sh $CS $RS games=400 tc=inf nodes=20000`), which compares the
algorithms independently of speed:

| Games | Rust W / L / D | Score | Elo | Pentanomial (LL, LD, DD/WL, WD, WW) |
|---|---|---|---|---|
| 400 | 110 / 110 / 180 | 50.0 % | **0.00 ± 0.00** | [0, 0, 200, 0, 0] |

Every one of the 200 opening pairs produced the same game with colors swapped
(`scripts/check-pairs.py`: 200 identical pairs, 0 differing). The port plays exactly the same
moves as the original.

**Time control 10+0.1:** in progress; results will be added here.

### 5. Speed

In progress; measured on an otherwise idle machine, results will be added here.

### 6. C API and iOS

- `cargo test --release`: core (slider attacks, SIMD vs. scalar NNUE, perft, Zobrist/eval values
  of the original, FEN handling), search (node counts of the original at depth 1–12, mate, TT),
  and 10 C API tests (argument checks, invalid positions, mate scores from both points of view,
  game over, time limit, `leorik_stop` from another thread, info callback, 4 threads).
- `scripts/test-c-api.sh`: a C program compiled against `leorik.h` and `libleorik.a`.
- `scripts/build-xcframework.sh` builds `aarch64-apple-ios` and `aarch64-apple-ios-sim` (Xcode 27.1),
  and `scripts/test-ios-sim.sh` runs the C test inside an iOS 26.5 simulator, with the same node
  counts as on macOS.

## Differences from the original

Behavior-preserving changes (none of them changes the search tree):

- The network is embedded in the binary instead of being loaded from a file next to the executable.
- Only the "Classic" slider attack generator is ported (the original picks PEXT on x86 CPUs with
  BMI2 and Classic elsewhere; both produce the same attack sets).
- The transposition table is an instance owned by the engine instead of a static global, so
  several engines can coexist in one process.
- A move taken from the transposition table is sanity-checked against the position before use.
  With a verified 64-bit key this never triggers in single-threaded search; it protects against
  torn entries in multi-threaded search, which in the original could at worst lead to an exception.

Robustness changes for use as a library:

- FENs are validated (8 ranks, exactly one king per side), and `position ... moves` / the C API
  reject illegal moves instead of playing them.
- With no legal moves, `go` returns immediately (`bestmove 0000`); the original would keep
  iterating until stopped.
- On Apple platforms the UCI search thread runs at user-interactive QoS, like the original's
  `ThreadPriority.Highest`.

Not ported: the tuning, test-suite and perft tools of the original solution (`Leorik.Tuning`,
`Leorik.Test`, `Leorik.Perft`), SAN move parsing, and the alternative slider generators.
