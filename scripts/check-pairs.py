#!/usr/bin/env python3 -I
"""Checks that the two games of every opening pair in a fastchess PGN have identical moves.

  python3 -I scripts/check-pairs.py match-results/<file>.pgn

With two engines that search identical trees (fixed nodes), swapping colors must reproduce the
same game move for move. Prints the number of identical pairs and the first differences.
"""
import re
import sys


def games(text):
    for block in re.split(r"\n(?=\[Event )", text.strip()):
        headers = dict(re.findall(r'^\[(\w+) "(.*)"\]$', block, re.M))
        body = re.sub(r"^\[.*\]$", "", block, flags=re.M)
        body = re.sub(r"\{[^}]*\}", " ", body)
        body = re.sub(r"\d+\.(\.\.)?", " ", body)
        moves = [t for t in body.split() if t not in ("1-0", "0-1", "1/2-1/2", "*")]
        yield headers, moves


def main(path):
    all_games = list(games(open(path).read()))
    pairs = {}
    for headers, moves in all_games:
        key = (headers.get("FEN", "startpos"), headers.get("Round", "").split(".")[0])
        pairs.setdefault(key, []).append((headers, moves))
    same = differ = incomplete = 0
    for key, gs in pairs.items():
        if len(gs) != 2:
            incomplete += 1
            continue
        (h1, m1), (h2, m2) = gs
        if m1 == m2 and h1.get("Result") == h2.get("Result") and h1.get("White") == h2.get("Black"):
            same += 1
        else:
            differ += 1
            if differ <= 5:
                n = next((i for i, (a, b) in enumerate(zip(m1, m2)) if a != b), min(len(m1), len(m2)))
                print(f"differs: {key[0]} at ply {n + 1}: {m1[n:n+3]} vs {m2[n:n+3]}")
    print(f"{len(all_games)} games, {same} identical pairs, {differ} differing pairs, {incomplete} incomplete")
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
