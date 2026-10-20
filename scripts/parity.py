#!/usr/bin/env python3 -I
"""Compare two Leorik UCI engines position by position (stdlib only).

  python3 -I scripts/parity.py REF_ENGINE TEST_ENGINE [options]

Mode "eval" (static parity), for every FEN in --fens:
  position fen X            -> compare "fen", "eval", "hash" and "moves"
  position fen X moves m1.. -> same, after 1..--plies random legal moves
                               (tests incremental eval/hash updates), and each
                               engine's incremental result must also equal its
                               own from-scratch result for the resulting FEN.
Mode "search", for every FEN with legal moves:
  ucinewgame / position fen X / go depth N
                            -> compare last "info depth" line (depth, score,
                               nodes, pv) and bestmove.
Mode "all" runs both. Exit status is 1 if anything differed.

"eval"/"hash"/"fen"/"moves" are Leorik's unofficial commands; each is followed
by "isready" and the reply is whatever arrives before "readyok".
"""
import argparse
import os
import queue
import random
import subprocess
import sys
import threading
import time
import zlib


class EngineError(Exception):
    pass


class Engine:
    def __init__(self, path, name, options, timeout):
        self.path, self.name, self.options, self.timeout = path, name, options, timeout
        self.start()

    def start(self):
        self.p = subprocess.Popen([self.path], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1,
                                  cwd=os.path.dirname(os.path.abspath(self.path)))
        self.q = queue.Queue()
        threading.Thread(target=self._pump, args=(self.p, self.q), daemon=True).start()
        self.send("uci")
        self.banner = self.until(lambda l: l == "uciok", 60)
        for name, value in self.options:
            self.send(f"setoption name {name} value {value}")
        self.sync()

    @staticmethod
    def _pump(p, q):
        for line in p.stdout:
            q.put(line.rstrip("\r\n"))
        q.put(None)

    def restart(self):
        try:
            self.p.kill()
            self.p.wait(5)
        except Exception:
            pass
        self.start()

    def send(self, cmd):
        try:
            self.p.stdin.write(cmd + "\n")
            self.p.stdin.flush()
        except (BrokenPipeError, OSError):
            raise EngineError(f"{self.name}: engine pipe closed")

    def until(self, pred, timeout):
        lines = []
        deadline = time.monotonic() + timeout
        while True:
            try:
                line = self.q.get(timeout=max(0.01, deadline - time.monotonic()))
            except queue.Empty:
                raise EngineError(f"{self.name}: timeout after {timeout}s")
            if line is None:
                raise EngineError(f"{self.name}: engine exited")
            lines.append(line)
            if pred(line):
                return lines

    def sync(self):
        self.send("isready")
        return [l for l in self.until(lambda l: l == "readyok", self.timeout)[:-1]
                if not l.startswith("info string")]

    def query(self, cmd):
        """Send an unofficial command; return its reply (last line before readyok)."""
        self.send(cmd)
        out = self.sync()
        return out[-1] if out else ""

    def position(self, fen, moves=()):
        self.send(f"position fen {fen}" + (" moves " + " ".join(moves) if moves else ""))

    def snapshot(self):
        return {
            "fen": self.query("fen"),
            "eval": self.query("eval"),
            "hash": self.query("hash"),
            "moves": self.query("moves"),
        }

    def close(self):
        try:
            self.send("quit")
            self.p.wait(5)
        except Exception:
            self.p.kill()


def norm_fen(fen):
    """Leorik's "fen" always prints fullmove 1; compare only the first 5 fields."""
    return " ".join(fen.split()[:5])


class Report:
    def __init__(self, verbose, max_print):
        self.counts = {}
        self.verbose, self.max_print, self.printed = verbose, max_print, 0

    def check(self, key, equal, detail):
        ok, total = self.counts.get(key, (0, 0))
        self.counts[key] = (ok + bool(equal), total + 1)
        if not equal and (self.verbose or self.printed < self.max_print):
            self.printed += 1
            print(f"MISMATCH {key}: {detail}")
        return equal

    def summary(self):
        print("\n=== summary ===")
        bad = False
        for key, (ok, total) in self.counts.items():
            flag = "" if ok == total else "   <-- MISMATCH"
            bad |= ok != total
            print(f"{key:28s} {ok:5d}/{total:<5d}{flag}")
        return bad


def compare_snapshots(rep, label, ctx, a, b):
    rep.check(f"{label} fen", norm_fen(a["fen"]) == norm_fen(b["fen"]),
              f"{ctx}\n  ref : {a['fen']}\n  test: {b['fen']}")
    rep.check(f"{label} eval", a["eval"] == b["eval"], f"{ctx}  ref={a['eval']} test={b['eval']}")
    rep.check(f"{label} hash", a["hash"] == b["hash"], f"{ctx}  ref={a['hash']} test={b['hash']}")
    ma, mb = a["moves"].split(), b["moves"].split()
    if rep.check(f"{label} moves (set)", sorted(ma) == sorted(mb),
                 f"{ctx}\n  only ref : {sorted(set(ma) - set(mb))}\n  only test: {sorted(set(mb) - set(ma))}"):
        rep.check(f"{label} moves (order)", ma == mb, f"{ctx}\n  ref : {a['moves']}\n  test: {b['moves']}")


def run_eval(engines, fens, args, rep):
    for n, fen in enumerate(fens, 1):
        ctx = f"[{n}] {fen}"
        try:
            snaps = []
            for e in engines:
                e.position(fen)
                snaps.append(e.snapshot())
            compare_snapshots(rep, "root", ctx, *snaps)

            rng = random.Random(zlib.crc32(fen.encode()) ^ args.seed)
            moves = []
            for _ in range(args.plies):
                legal = snaps[0]["moves"].split()
                if not legal:
                    break
                moves.append(rng.choice(legal))
                mctx = f"[{n}] {fen} moves {' '.join(moves)}"
                snaps = []
                for e in engines:
                    e.position(fen, moves)
                    snaps.append(e.snapshot())
                compare_snapshots(rep, "incremental", mctx, *snaps)
                # incremental update vs. setting the resulting FEN from scratch
                for e, s in zip(engines, snaps):
                    e.position(s["fen"])
                    eva, hsh = e.query("eval"), e.query("hash")
                    rep.check(f"self-consistency ({e.name})", (eva, hsh) == (s["eval"], s["hash"]),
                              f"{mctx}\n  incremental eval={s['eval']} hash={s['hash']}"
                              f"\n  from fen    eval={eva} hash={hsh}  ({s['fen']})")
        except EngineError as ex:
            rep.check("engine errors", False, f"{ctx}: {ex}")
            for e in engines:
                e.restart()
        if n % 50 == 0:
            print(f"... eval {n}/{len(fens)}", file=sys.stderr)


def parse_info(line):
    t = line.split()
    out = {}
    for key in ("depth", "seldepth", "nodes"):
        if key in t:
            out[key] = t[t.index(key) + 1]
    if "score" in t:
        i = t.index("score")
        out["score"] = " ".join(t[i + 1:i + 3])
    out["pv"] = " ".join(t[t.index("pv") + 1:]) if "pv" in t else ""
    return out


def run_search(engines, fens, args, rep):
    ref = engines[0]
    totals = {e.name: [0, 0.0] for e in engines}
    for n, fen in enumerate(fens, 1):
        ctx = f"[{n}] {fen}"
        try:
            ref.position(fen)
            if not ref.query("moves").split():
                rep.check("search skipped (no legal moves)", True, "")
                continue
            for e in engines:
                e.send("ucinewgame")
                e.sync()
                e.position(fen)
            started = {}
            for e in engines:  # both search at the same time
                started[e.name] = time.monotonic()
                e.send(f"go depth {args.depth}")
            res = []
            for e in engines:
                lines = e.until(lambda l: l.startswith("bestmove"), args.search_timeout)
                elapsed = time.monotonic() - started[e.name]
                info = [l for l in lines if l.startswith("info depth")]
                r = parse_info(info[-1]) if info else {}
                r["bestmove"] = lines[-1].split()[1] if len(lines[-1].split()) > 1 else ""
                totals[e.name][0] += int(r.get("nodes", 0))
                totals[e.name][1] += elapsed
                res.append(r)
            a, b = res
            rep.check("search bestmove", a["bestmove"] == b["bestmove"],
                      f"{ctx}  ref={a['bestmove']} test={b['bestmove']}")
            for key in ("depth", "score", "nodes", "pv"):
                rep.check(f"search {key}", a.get(key) == b.get(key),
                          f"{ctx}\n  ref : {a.get(key)}\n  test: {b.get(key)}")
        except EngineError as ex:
            rep.check("engine errors", False, f"{ctx}: {ex}")
            for e in engines:
                e.restart()
        if n % 25 == 0:
            print(f"... search {n}/{len(fens)}", file=sys.stderr)
    print("\n=== search totals (wall clock, both engines searching concurrently) ===")
    for name, (nodes, secs) in totals.items():
        nps = nodes / secs if secs else 0
        print(f"{name:5s} nodes {nodes:12d}  time {secs:8.1f}s  ~{nps:,.0f} nps")


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ref", help="reference engine (e.g. the C# Leorik.Engine)")
    ap.add_argument("test", help="engine under test (e.g. target/release/leorik)")
    ap.add_argument("--fens", default=os.path.join(root, "testdata", "parity_fens.txt"))
    ap.add_argument("--mode", choices=("eval", "search", "all"), default="all")
    ap.add_argument("--depth", type=int, default=8, help="search depth (default 8)")
    ap.add_argument("--plies", type=int, default=3, help="random moves for incremental tests")
    ap.add_argument("--limit", type=int, default=0, help="only the first N FENs")
    ap.add_argument("--hash", type=int, default=50, help="Hash MB sent to both (default 50, Leorik's default)")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--timeout", type=float, default=30, help="per-command timeout (s)")
    ap.add_argument("--search-timeout", type=float, default=600, help="per-search timeout (s)")
    ap.add_argument("--max-print", type=int, default=30, help="print at most N mismatches")
    ap.add_argument("-v", "--verbose", action="store_true", help="print every mismatch")
    args = ap.parse_args()

    with open(args.fens) as fh:
        fens = [l.strip() for l in fh if l.strip() and not l.startswith("#")]
    if args.limit:
        fens = fens[:args.limit]
    opts = [("Hash", args.hash), ("Threads", 1)]
    engines = [Engine(args.ref, "ref", opts, args.timeout), Engine(args.test, "test", opts, args.timeout)]
    for e in engines:
        print(f"{e.name}: {e.path}")
        for l in e.banner:
            if l.startswith("id name") or not l.startswith(("id ", "option", "uciok")):
                print(f"    {l}")
    rep = Report(args.verbose, args.max_print)
    t0 = time.monotonic()
    if args.mode in ("eval", "all"):
        run_eval(engines, fens, args, rep)
    if args.mode in ("search", "all"):
        run_search(engines, fens, args, rep)
    for e in engines:
        e.close()
    bad = rep.summary()
    print(f"\n{len(fens)} FENs, {time.monotonic() - t0:.1f}s, result: {'MISMATCHES' if bad else 'ALL EQUAL'}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
