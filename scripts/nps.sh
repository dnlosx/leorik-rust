#!/usr/bin/env bash
# Measure an engine's speed: "go depth N" on 10 bench positions, fresh TT each.
#
#   scripts/nps.sh ENGINE [DEPTH=12] [HASH_MB=16] [PASSES=2]
#
# Prints per-position nodes/time as reported by the engine's last "info depth"
# line, then total nodes / total time per pass. The bench is run PASSES times
# in the same process and the LAST pass is the result: the C# build needs a
# warm-up because .NET's tiered JIT runs the first ~second of search in
# unoptimised code (about 5-10x slower). Run alone on an idle machine.
set -euo pipefail
if [ $# -lt 1 ]; then sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 2; fi
exec python3 -I - "$@" <<'PY'
import os, subprocess, sys, time

engine = os.path.abspath(sys.argv[1])
depth = int(sys.argv[2]) if len(sys.argv) > 2 else 12
hash_mb = int(sys.argv[3]) if len(sys.argv) > 3 else 16
passes = int(sys.argv[4]) if len(sys.argv) > 4 else 2
BENCH = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "r1bq1rk1/pp2bppp/2n1pn2/2pp4/2PP4/2N1PN2/PP1QBPPP/R1B1K2R w KQ - 4 8",
    "2r2rk1/1bqnbppp/pp1ppn2/8/2PNP3/1PN1BP2/P2QB1PP/2RR2K1 w - - 0 15",
    "8/5pk1/6p1/3P3p/1p3P1P/1P2K1P1/8/8 w - - 0 45",
    "6k1/5p2/4p1p1/3pP1Pp/1r1P3P/2R2K2/8/8 b - - 3 50",
]

p = subprocess.Popen([engine], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.DEVNULL, text=True, bufsize=1,
                     cwd=os.path.dirname(engine))

def send(cmd):
    p.stdin.write(cmd + "\n"); p.stdin.flush()

def until(prefix):
    lines = []
    for line in p.stdout:
        line = line.strip()
        lines.append(line)
        if line.startswith(prefix):
            return lines
    sys.exit("engine exited unexpectedly")

send("uci"); name = [l for l in until("uciok") if l.startswith("id name")]
send(f"setoption name Hash value {hash_mb}")
send("setoption name Threads value 1")
send("isready"); until("readyok")
print(f"{name[0][8:] if name else engine}  depth {depth}  hash {hash_mb}MB")
def bench(label):
    tot_nodes = tot_ms = 0
    wall = 0.0
    print(f"-- {label}")
    for i, fen in enumerate(BENCH, 1):
        send("ucinewgame"); send("isready"); until("readyok")
        send(f"position fen {fen}")
        t0 = time.monotonic()
        send(f"go depth {depth}")
        lines = until("bestmove")
        wall += time.monotonic() - t0
        info = [l.split() for l in lines if l.startswith("info depth")][-1]
        nodes = int(info[info.index("nodes") + 1])
        ms = int(info[info.index("time") + 1])
        tot_nodes += nodes; tot_ms += ms
        print(f"{i:2d} {nodes:11d} nodes {ms:7d} ms {nodes * 1000 // max(ms, 1):9d} nps  {lines[-1]}")
    nps = tot_nodes * 1000 // max(tot_ms, 1)
    print(f"   total {tot_nodes} nodes in {tot_ms} ms (engine clock), {wall:.2f}s wall, {nps} nps")
    return tot_nodes, nps

for n in range(1, passes + 1):
    nodes, nps = bench(f"pass {n}/{passes}" + (" (warm-up)" if n < passes else ""))
send("quit")
print(f"bench nodes {nodes}")
print(f"nps {nps}")
PY
