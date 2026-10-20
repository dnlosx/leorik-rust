#!/usr/bin/env python3 -I
"""Generate testdata/parity_fens.txt and testdata/openings.epd with a UCI engine.

Stdlib only. Uses the engine's unofficial "moves" (legal moves) and "fen"
commands to play semi-random games, so no python-chess is needed.

  python3 -I scripts/gen_testdata.py fens     --engine PATH [--out testdata/parity_fens.txt]
  python3 -I scripts/gen_testdata.py openings --engine PATH [--out testdata/openings.epd]

Both are deterministic for a given --seed and engine.
"""
import argparse
import os
import queue
import random
import subprocess
import sys
import threading

STARTPOS = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"

# The classic perft positions (chessprogramming.org "Perft Results").
STANDARD_PERFT = [
    STARTPOS,
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
]


class Engine:
    def __init__(self, path):
        self.p = subprocess.Popen([path], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1,
                                  cwd=os.path.dirname(os.path.abspath(path)))
        self.q = queue.Queue()
        threading.Thread(target=self._pump, daemon=True).start()
        self.send("uci")
        self.until(lambda l: l == "uciok", 60)
        self.sync()

    def _pump(self):
        for line in self.p.stdout:
            self.q.put(line.rstrip("\r\n"))
        self.q.put(None)

    def send(self, cmd):
        self.p.stdin.write(cmd + "\n")
        self.p.stdin.flush()

    def until(self, pred, timeout):
        lines = []
        while True:
            line = self.q.get(timeout=timeout)
            if line is None:
                raise RuntimeError("engine exited")
            lines.append(line)
            if pred(line):
                return lines

    def sync(self, timeout=60):
        self.send("isready")
        return self.until(lambda l: l == "readyok", timeout)[:-1]

    def query(self, cmd):
        self.send(cmd)
        return self.sync()

    def setpos(self, fen, moves=()):
        self.send(f"position fen {fen}" + (" moves " + " ".join(moves) if moves else ""))

    def legal_moves(self):
        out = self.query("moves")
        return out[-1].split() if out else []

    def fen(self):
        return self.query("fen")[-1]

    def go(self, args, timeout=600):
        self.send("go " + args)
        lines = self.until(lambda l: l.startswith("bestmove"), timeout)
        info = [l for l in lines if l.startswith("info depth")]
        last = info[-1].split() if info else []
        score = None
        if "score" in last:
            i = last.index("score")
            kind, val = last[i + 1], int(last[i + 2])
            score = val if kind == "cp" else (100000 if val > 0 else -100000)
        return lines[-1].split()[1], score

    def close(self):
        try:
            self.send("quit")
            self.p.wait(5)
        except Exception:
            self.p.kill()


# ---------- tiny board helpers (for classification only) ----------

def parse_board(fen):
    board = {}
    for r, row in enumerate(fen.split()[0].split("/")):
        f = 0
        for c in row:
            if c.isdigit():
                f += int(c)
            else:
                board[(f, 7 - r)] = c
                f += 1
    return board


def attacked(board, sq, by_white):
    f, r = sq
    def at(df, dr):
        return board.get((f + df, r + dr))
    pawn = "P" if by_white else "p"
    pdr = -1 if by_white else 1
    if at(-1, pdr) == pawn or at(1, pdr) == pawn:
        return True
    knight = "N" if by_white else "n"
    for d in ((1, 2), (2, 1), (-1, 2), (-2, 1), (1, -2), (2, -1), (-1, -2), (-2, -1)):
        if at(*d) == knight:
            return True
    king = "K" if by_white else "k"
    for df in (-1, 0, 1):
        for dr in (-1, 0, 1):
            if (df or dr) and at(df, dr) == king:
                return True
    for dirs, pieces in ((((1, 0), (-1, 0), (0, 1), (0, -1)), "RQ"),
                         (((1, 1), (1, -1), (-1, 1), (-1, -1)), "BQ")):
        pieces = pieces if by_white else pieces.lower()
        for df, dr in dirs:
            x, y = f + df, r + dr
            while 0 <= x < 8 and 0 <= y < 8:
                p = board.get((x, y))
                if p:
                    if p in pieces:
                        return True
                    break
                x, y = x + df, y + dr
    return False


def in_check(fen):
    board = parse_board(fen)
    white = fen.split()[1] == "w"
    k = "K" if white else "k"
    ksq = next((s for s, p in board.items() if p == k), None)
    return ksq is not None and attacked(board, ksq, not white)


def features(fen):
    f = fen.split()
    board = parse_board(fen)
    n = len(board)
    stm_white = f[1] == "w"
    promo = any(p == "P" and s[1] == 6 for s, p in board.items()) if stm_white else \
        any(p == "p" and s[1] == 1 for s, p in board.items())
    return {
        "pieces": n,
        "phase": "opening" if n >= 30 else ("endgame" if n <= 12 else "middlegame"),
        "castle": f[2] != "-",
        "ep": f[3] != "-",
        "promo": promo,
        "check": in_check(fen),
    }


def clean_ep(fen):
    """Leorik sets the en-passant square after every double push; keep it only if
    a pawn of the side to move can actually capture there (standard FEN)."""
    f = fen.split()
    if f[3] != "-":
        board = parse_board(fen)
        file, rank = ord(f[3][0]) - 97, int(f[3][1]) - 1
        pawn, r = ("P", rank - 1) if f[1] == "w" else ("p", rank + 1)
        if board.get((file - 1, r)) != pawn and board.get((file + 1, r)) != pawn:
            f[3] = "-"
    return " ".join(f)


def with_counters(fen, halfmove, fullmove):
    f = fen.split()
    return " ".join(f[:4] + [str(halfmove), str(fullmove)])


# ---------- parity FENs ----------

def random_game(eng, rng, maxply, engine_prob):
    """Play from startpos; yield (fen, nlegal) for every position reached."""
    moves = []
    halfmove = 0
    for ply in range(maxply):
        eng.setpos(STARTPOS, moves)
        fen = eng.fen()
        legal = eng.legal_moves()
        halfmove = int(fen.split()[4])
        yield with_counters(fen, halfmove, 1 + ply // 2), legal
        if not legal:
            return
        if rng.random() < engine_prob:
            mv, _ = eng.go("depth %d" % rng.choice((1, 2, 3)))
        else:
            mv = rng.choice(legal)
        moves.append(mv)


ENDGAME_SETS = ["KPk", "Kpk", "KRkr", "KRPkr", "KQkr", "KBNk", "KPPkp", "KRkp",
                "KQkq", "KNPkb", "KBPkn", "KRBkr", "KPPPkppp", "KQPkq", "KRRkrr", "KPkp"]


def random_endgame(eng, rng):
    pieces = rng.choice(ENDGAME_SETS)
    for _ in range(200):
        board = {}
        ok = True
        for p in pieces:
            for _ in range(50):
                sq = (rng.randrange(8), rng.randrange(8))
                if sq in board or (p in "Pp" and sq[1] in (0, 7)):
                    continue
                board[sq] = p
                break
            else:
                ok = False
        if not ok:
            continue
        ks = [s for s, p in board.items() if p in "Kk"]
        if abs(ks[0][0] - ks[1][0]) <= 1 and abs(ks[0][1] - ks[1][1]) <= 1:
            continue
        stm = rng.choice("wb")
        rows = []
        for r in range(7, -1, -1):
            row, e = "", 0
            for f in range(8):
                p = board.get((f, r))
                if p:
                    row += (str(e) if e else "") + p
                    e = 0
                else:
                    e += 1
            rows.append(row + (str(e) if e else ""))
        fen = "/".join(rows) + f" {stm} - - 0 {rng.randrange(30, 80)}"
        # side not to move must not be in check
        other = fen.replace(f" {stm} ", " w " if stm == "b" else " b ")
        if in_check(other):
            continue
        eng.setpos(fen)
        legal = eng.legal_moves()
        if legal:
            return fen, legal
    return None, None


def gen_fens(args):
    rng = random.Random(args.seed)
    eng = Engine(args.engine)
    pool = {}  # fen-without-counters -> (fen, feats)

    def add(fen):
        key = " ".join(fen.split()[:4])
        if key not in pool:
            pool[key] = (fen, features(fen))

    for g in range(args.games):
        engine_prob = rng.choice((0.0, 0.3, 0.6, 0.85))
        positions = list(random_game(eng, rng, rng.randrange(20, 160), engine_prob))
        # sample a handful of positions from each game
        for fen, legal in rng.sample(positions, min(8, len(positions))):
            add(fen)
        fen, legal = positions[-1]
        if not legal:  # keep game-ending positions (mate / stalemate) too
            add(fen)
        if g % 20 == 0:
            print(f"game {g}/{args.games}, pool {len(pool)}", file=sys.stderr)
    for _ in range(args.endgames):
        fen, _ = random_endgame(eng, rng)
        if fen:
            add(fen)

    # how many legal moves each candidate has (to keep a few terminal positions)
    def nlegal(fen):
        eng.setpos(fen)
        return len(eng.legal_moves())

    chosen = []
    seen = set()

    def take(pred, n):
        cands = [k for k, (fen, ft) in pool.items() if k not in seen and pred(fen, ft)]
        rng.shuffle(cands)
        for k in cands[:n]:
            seen.add(k)
            chosen.append(pool[k][0])

    perft_extra = []
    perft_path = os.path.join(os.path.dirname(os.path.abspath(args.out)), "perft.txt")
    if os.path.exists(perft_path):
        with open(perft_path) as fh:
            perft_extra = [l.split(";")[0].strip() for l in fh if l.strip()]
    for fen in STANDARD_PERFT + rng.sample(perft_extra, min(23, len(perft_extra))):
        key = " ".join(fen.split()[:4])
        if key not in seen:
            seen.add(key)
            chosen.append(fen)

    # keep only a few mate/stalemate positions; mark the rest as already seen
    terminal = [k for k, (fen, ft) in pool.items() if nlegal(fen) == 0]
    rng.shuffle(terminal)
    kept = [k for k in terminal if pool[k][1]["pieces"] > 3][:6]
    for k in kept:
        chosen.append(pool[k][0])
    seen.update(terminal)
    take(lambda f, t: t["check"], 35)
    take(lambda f, t: t["ep"], 25)
    take(lambda f, t: t["promo"], 25)
    take(lambda f, t: t["castle"] and t["phase"] != "opening", 20)
    take(lambda f, t: t["phase"] == "opening", 45)
    take(lambda f, t: t["phase"] == "endgame", 55)
    take(lambda f, t: t["phase"] == "middlegame", args.count - len(chosen))
    take(lambda f, t: True, args.count - len(chosen))
    eng.close()

    with open(args.out, "w") as fh:
        for fen in chosen[:args.count]:
            fh.write(fen + "\n")
    summary = {k: 0 for k in ("opening", "middlegame", "endgame", "castle", "ep", "promo", "check")}
    for fen in chosen[:args.count]:
        ft = features(fen)
        summary[ft["phase"]] += 1
        for k in ("castle", "ep", "promo", "check"):
            summary[k] += ft[k]
    print(f"wrote {min(len(chosen), args.count)} FENs to {args.out}: {summary}, "
          f"terminal={len(kept)}", file=sys.stderr)


# ---------- openings ----------

def gen_openings(args):
    rng = random.Random(args.seed)
    eng = Engine(args.engine)
    out, seen, tried = [], set(), 0
    while len(out) < args.count and tried < args.count * 50:
        tried += 1
        moves = []
        for _ in range(args.plies):
            eng.setpos(STARTPOS, moves)
            legal = eng.legal_moves()
            if not legal:
                break
            moves.append(rng.choice(legal))
        eng.setpos(STARTPOS, moves)
        if len(moves) < args.plies or not eng.legal_moves():
            continue
        fen = clean_ep(eng.fen())
        key = " ".join(fen.split()[:4])
        if key in seen:
            continue
        seen.add(key)
        eng.send("ucinewgame")
        eng.setpos(fen)
        _, score = eng.go(f"depth {args.depth}")
        if score is not None and abs(score) <= args.margin:
            out.append(key)
            if len(out) % 20 == 0:
                print(f"{len(out)}/{args.count} kept ({tried} tried)", file=sys.stderr)
    eng.close()
    with open(args.out, "w") as fh:
        for fen in out:
            fh.write(fen + "\n")
    print(f"wrote {len(out)} openings to {args.out} ({tried} tried)", file=sys.stderr)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("fens")
    a.add_argument("--engine", required=True)
    a.add_argument("--out", default=os.path.join(root, "testdata", "parity_fens.txt"))
    a.add_argument("--count", type=int, default=300)
    a.add_argument("--games", type=int, default=120)
    a.add_argument("--endgames", type=int, default=60)
    a.add_argument("--seed", type=int, default=3202)
    b = sub.add_parser("openings")
    b.add_argument("--engine", required=True)
    b.add_argument("--out", default=os.path.join(root, "testdata", "openings.epd"))
    b.add_argument("--count", type=int, default=200)
    b.add_argument("--plies", type=int, default=8)
    b.add_argument("--depth", type=int, default=8)
    b.add_argument("--margin", type=int, default=60)
    b.add_argument("--seed", type=int, default=3202)
    args = ap.parse_args()
    (gen_fens if args.cmd == "fens" else gen_openings)(args)


if __name__ == "__main__":
    main()
