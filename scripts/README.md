# Verification scripts

Tools for checking that the Rust port plays like the original C# Leorik 3.2.
Python scripts are stdlib only (run with `python3 -I`). Every script takes
engine paths as arguments.

```bash
CS=~/development/LeorikReference/build-3.2/Leorik.Engine   # original C# build
RS=target/release/leorik                                     # Rust port
```

## parity.py: eval, hash, movegen and search parity

```bash
python3 -I scripts/parity.py $CS $RS                 # everything, depth 8
python3 -I scripts/parity.py $CS $RS --mode eval     # static checks only (~15 s)
python3 -I scripts/parity.py $CS $RS --mode search --depth 12
```

For each FEN in `testdata/parity_fens.txt`:

- **eval mode** compares `fen`, `eval`, `hash` and `moves` (as a set, then in
  order) after `position fen X`. It then plays 1..`--plies` (default 3) random
  legal moves via `position fen X moves …` and compares again. As a separate
  check, each engine's incremental eval/hash must equal what it computes from
  scratch for the resulting FEN.
- **search mode** sends `ucinewgame` / `position fen X` / `go depth N` to both
  engines at once. It compares the last `info depth` line (depth, score, nodes,
  pv) and `bestmove`. Positions with no legal moves are skipped (see the hang
  below).

Both engines get `Hash 50` (Leorik's default; change it with `--hash`) and
`Threads 1`. The TT size changes node counts, so the two engines must always
use the same value. The script prints at most 30 mismatches (`-v` prints all),
then a summary table. It exits with status 1 if anything differed.

C# against itself, depth 10: everything equal on 300 FENs and 882 incremental
positions, including identical node counts. That run took about 17 s.

## match.sh: fastchess match

```bash
scripts/match.sh $CS $RS games=400 concurrency=8 tc=inf nodes=20000   # strength at equal nodes
scripts/match.sh $CS $RS games=200 concurrency=4 tc=10+0.1            # time control
```

Keys: `games` (rounded up to even), `concurrency`, `tc`, `nodes`, `depth`,
`hash` (16), `name1`/`name2`, `openings` (`testdata/openings.epd`),
`adjudicate=0|1`, `tag`. Anything after `--` goes straight to fastchess.
Openings are random, and each is played twice with colours swapped (`-repeat`),
with `-recover`. The script writes `<tag>-<stamp>.{pgn,log,out}` to
`match-results/` (git-ignored) and ends by printing fastchess's last results
block. Set `FASTCHESS=…` to use a different binary, and `SRAND=n` for a
reproducible opening order.

- **Fixed nodes is deterministic.** In a C#-vs-C# match at `nodes=N`, every
  game pair comes out 1–1 (pentanomial `[0, 0, P, 0, 0]`). An exact port should
  show the same against the C# engine. Pairs that are not 1–1 point to a search
  divergence, even when the Elo is near 0.
- **Time-control matches are noisy.** The .NET JIT warms up during the first
  games, so leave the first few out when reading short runs.

## nps.sh: speed

```bash
scripts/nps.sh $CS 12        # depth 12, 16 MB, 2 passes; the last pass is the result
scripts/nps.sh $RS 12
```

nps.sh runs 10 bench positions, each with a fresh TT, and prints per-position
and total nodes and time from the engine's own `info` output. The total node
count is deterministic, so it doubles as a bench signature: C# depth 12 gives
**1091680** nodes, and a faithful port should print the same number.

## gen_testdata.py: regenerate the test data

```bash
python3 -I scripts/gen_testdata.py fens     --engine $CS   # testdata/parity_fens.txt (~75 s)
python3 -I scripts/gen_testdata.py openings --engine $CS   # testdata/openings.epd (~12 s)
```

Both commands are deterministic for a fixed `--seed`.

- **fens** builds 300 positions:
  - the 7 classic perft positions plus 23 picked from `testdata/perft.txt`;
  - 6 mate or stalemate positions;
  - positions sampled from 120 semi-random games (random moves mixed with
    depth 1–3 engine moves);
  - random small-material endgames;
  - quotas for check, en passant, promotion and castling rights.

  The current file has 71 opening, 121 middlegame and 108 endgame positions:
  48 in check, 39 with an en-passant square, 31 with a promotion available and
  123 with castling rights.
- **openings** plays 8 random plies from the start position and keeps the
  result if `go depth 8` scores within ±60 cp. It produces 200 openings from
  407 tries. The en-passant field is cleared unless a capture is actually
  possible.

## Quirks of the original C# engine's UCI interface

The Rust port should match these where they affect output, and avoid them
where they are plain bugs:

1. **Any command stops a running search.** `position` and `ucinewgame` call
   `Stop()`, and stdin is read while the search runs, so a pipelined
   `go depth 8` followed by `position …` aborts the search early (at depth 1 in
   one test). `isready` answers right away during a search and does not wait
   for it. Always wait for `bestmove` before sending the next command, as these
   scripts do.
2. **`go` on a checkmate or stalemate position hangs for good.** No `bestmove`
   is ever sent and `stop` does not help: the process spins at 100% CPU and has
   to be killed. parity.py skips these positions in search mode.
3. **The en-passant square is set after every double push**, even when no
   capture is possible. `fen` prints it and it is included in the Zobrist hash.
   `parity_fens.txt` keeps these squares on purpose, so the port's hash must
   do the same. `openings.epd` clears them.
4. **`fen` always prints fullmove number 1.** The fullmove field is ignored
   when parsing, and the halfmove clock is read as a byte. parity.py compares
   only the first five FEN fields.
5. **`eval` is from the side to move's point of view**, as is the score in
   `info`: 74 at the start position, -158 for bare kings with White to move.
6. **Moves in `position … moves` are matched against pseudo-legal moves**, so a
   king can be moved into check. An unknown move, a FEN without kings, or
   `position` with no arguments throws an unhandled .NET exception and the
   engine process exits.
7. **An empty input line prints `Unknown command: `.**
8. **The `info` PV is extended from the TT past threefold repetitions.**
   fastchess warns `PV continues after threefold repetition` about it, tens of
   times per game. match.sh hides these warnings on screen and counts them;
   they stay in the `.out` file.
9. **Startup prints two lines before any command:** `Leorik 3.2 Classic` and
   `Loading NNUE weights from 640HL-S-5io8-6116M-FRCv1.nnue!`. The network
   loads correctly from any working directory.
10. **Speed depends heavily on JIT warm-up.** The first search in a process
    runs at about 70–150k nps; warmed up, the engine reaches 450k–1.1M nps on
    this M-series Mac (about 550k on the nps.sh bench). The "about 90k nps"
    figure is mostly unoptimised Tier-0 JIT code, not only the missing
    Vector256 support. A time-control match is therefore much closer to fair
    than expected, but fixed nodes remains the cleanest comparison.
11. **`Temperature` defaults to 0**, so root moves get no random offset and the
    search is deterministic. Each `go` builds a new search (new history and
    killers), and only `ucinewgame` clears the TT. For the same node counts in
    both engines, send `ucinewgame` before each search and use the same Hash
    size.
