#!/bin/sh
# Prints a Markdown perft table for the standard test positions using the UCI "perft" command.
# Usage: scripts/perft-table.sh [engine]   (default: target/release/leorik)
set -eu
ENGINE="${1:-$(dirname "$0")/../target/release/leorik}"
echo "| Position | Depth | Nodes | Expected | Time (s) |"
echo "|---|---|---|---|---|"
while IFS='|' read -r name fen depth expected; do
    start=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
    nodes=$(printf 'position fen %s\nperft %s\nquit\n' "$fen" "$depth" | "$ENGINE" | tail -1)
    end=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
    ok=$([ "$nodes" = "$expected" ] && echo "$expected ✓" || echo "$expected ✗")
    echo "| $name | $depth | $nodes | $ok | $(echo "$end - $start" | bc) |"
done <<'POSITIONS'
Start position|rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1|6|119060324
Kiwipete|r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1|5|193690690
Position 3|8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1|6|11030083
Position 4|r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1|5|15833292
Position 4 (mirrored)|r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1|5|15833292
Position 5|rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8|5|89941194
Position 6|r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10|5|164075551
POSITIONS
