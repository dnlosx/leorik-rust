#!/usr/bin/env bash
# Play a fastchess match between two UCI engines and print the Elo summary.
#
#   scripts/match.sh ENGINE1 ENGINE2 [key=value ...] [-- extra fastchess args]
#
# keys (defaults in brackets):
#   games=N        total games, rounded up to even; each opening is played twice
#                  with colours swapped (-repeat) [100]
#   concurrency=N  games in parallel [4]
#   tc=TC          fastchess time control, e.g. 10+0.1, 60+0.6, or inf [10+0.1]
#   nodes=N        fixed nodes per move ("go nodes N"); use with tc=inf [unset]
#   depth=N        fixed depth per move; use with tc=inf [unset]
#   hash=MB        Hash for both engines [16]
#   name1/name2    engine names in the PGN [file name, -2 suffix if equal]
#   openings=FILE  EPD book [testdata/openings.epd]
#   adjudicate=0|1 draw/resign adjudication [1]
#   tag=TEXT       results file prefix [match]
#
# Results go to match-results/ (git-ignored): <tag>-<stamp>.pgn, .log, .out.
#
# Examples:
#   scripts/match.sh $CS $RS games=400 concurrency=8 tc=inf nodes=20000
#   scripts/match.sh $CS $RS games=200 concurrency=4 tc=10+0.1
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FASTCHESS="${FASTCHESS:-$HOME/development/tools/fastchess/fastchess}"

if [ $# -lt 2 ]; then
  sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

abspath() { (cd "$(dirname "$1")" && echo "$(pwd)/$(basename "$1")"); }
E1="$(abspath "$1")"; E2="$(abspath "$2")"; shift 2
for e in "$E1" "$E2"; do [ -x "$e" ] || { echo "not executable: $e" >&2; exit 2; }; done

games=100 concurrency=4 tc=10+0.1 nodes="" depth="" hash=16
name1="" name2="" openings="$ROOT/testdata/openings.epd" adjudicate=1 tag=match
EXTRA=()
while [ $# -gt 0 ]; do
  case "$1" in
    --) shift; EXTRA=("$@"); break ;;
    games=*|concurrency=*|tc=*|nodes=*|depth=*|hash=*|name1=*|name2=*|openings=*|adjudicate=*|tag=*)
      declare "${1%%=*}=${1#*=}" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

[ -n "$name1" ] || name1="$(basename "$E1")"
[ -n "$name2" ] || name2="$(basename "$E2")"
[ "$name1" != "$name2" ] || name2="$name2-2"
rounds=$(( (games + 1) / 2 ))

limit="tc=$tc"
[ -z "$nodes" ] || limit="$limit nodes=$nodes"
[ -z "$depth" ] || limit="$limit depth=$depth"

ADJ=()
if [ "$adjudicate" = 1 ]; then
  ADJ=(-draw movenumber=40 movecount=8 score=10 -resign movecount=3 score=1000 twosided=true -maxmoves 300)
fi

OUT="$ROOT/match-results"
mkdir -p "$OUT"
stamp="$(date +%Y%m%d-%H%M%S)"
base="$OUT/$tag-$stamp"

echo "== $name1 vs $name2: $((rounds * 2)) games, concurrency $concurrency, $limit, hash $hash"
echo "== results: $base.{pgn,log,out}"

# Leorik extends its PV from the TT past threefold repetitions, which makes
# fastchess print a 4-line "PV continues after threefold repetition" warning
# many times per game. Those go to the .out file but are hidden on screen.
set +e
# shellcheck disable=SC2086
"$FASTCHESS" \
  -engine cmd="$E1" name="$name1" dir="$(dirname "$E1")" \
  -engine cmd="$E2" name="$name2" dir="$(dirname "$E2")" \
  -each proto=uci $limit option.Hash="$hash" option.Threads=1 \
  -openings file="$openings" format=epd order=random \
  -srand "${SRAND:-$RANDOM}" \
  -rounds "$rounds" -repeat -recover \
  -concurrency "$concurrency" \
  ${ADJ[@]+"${ADJ[@]}"} \
  -pgnout file="$base.pgn" nodes=true \
  -log file="$base.log" level=warn engine=false \
  -ratinginterval 20 \
  ${EXTRA[@]+"${EXTRA[@]}"} 2>&1 | tee "$base.out" |
  grep --line-buffered -v -E '^(Warning; PV continues after threefold|Info; info|Position; fen|Moves; )|^$'
status=${PIPESTATUS[0]}
set -e
pv_warnings=$(grep -c 'PV continues after threefold' "$base.out" || true)

echo
echo "================ summary ================"
# the last "Results of" block printed by fastchess
awk '/^Results of /{buf=""; on=1} on{buf=buf $0 "\n"} END{printf "%s", buf}' "$base.out"
echo "pgn: $base.pgn   (hidden PV-after-repetition warnings: $pv_warnings)"
exit "$status"
