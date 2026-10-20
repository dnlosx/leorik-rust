/*
 * leorik.h - C API of the Leorik chess engine (Rust port of Leorik 3.2 by Thomas Jahn).
 * MIT License, see LICENSE.
 *
 * Threading: an engine handle may be used from any thread, but calls on the same handle are
 * serialized (a second call waits until the first returns). leorik_stop() is the exception: it
 * never blocks and makes a running leorik_best_move()/leorik_analyze() return early with the
 * best result found so far. It only affects a search that is already running.
 */
#ifndef LEORIK_H
#define LEORIK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return codes */
#define LEORIK_OK 0
#define LEORIK_ERR_ARGUMENT -1   /* NULL handle/pointer or invalid limits */
#define LEORIK_ERR_POSITION -2   /* invalid FEN or illegal move in the move list */
#define LEORIK_ERR_GAME_OVER -3  /* no legal move: checkmate or stalemate */
#define LEORIK_ERR_INTERNAL -4   /* unexpected internal error (Rust panic caught) */

#define LEORIK_MOVE_LEN 8        /* "e7e8q" + NUL, with room to spare */
#define LEORIK_PV_LEN 1024

typedef struct LeorikEngine LeorikEngine;

typedef struct LeorikAnalysis {
    int32_t depth;                 /* completed search depth */
    /* Side to move's point of view (UCI convention). */
    int32_t score_cp;              /* centipawns; 0 if mate != 0 */
    int32_t mate;                  /* mate in N moves (>0 side to move mates, <0 gets mated), 0 if none */
    /* White's point of view (positive = good for White). */
    int32_t white_score_cp;
    int32_t white_mate;            /* >0 White mates, <0 Black mates, 0 if none */
    int64_t nodes;
    int32_t time_ms;
    char best_move[LEORIK_MOVE_LEN]; /* UCI notation, e.g. "e2e4", "e7e8q"; castling as "e1g1" */
    char pv[LEORIK_PV_LEN];          /* principal variation, space separated UCI moves */
} LeorikAnalysis;

/* Called after every completed iteration of leorik_analyze(), on the calling thread. */
typedef void (*LeorikInfoCallback)(const LeorikAnalysis *info, void *user_data);

/* Engine version string, e.g. "Leorik 3.2.0 (Rust port)". Static, do not free. */
const char *leorik_version(void);

/* Creates an engine. hash_mb: transposition table size (<= 0 for the default of 50 MB).
 * threads: number of search threads (<= 0 for 1). Returns NULL on failure. */
LeorikEngine *leorik_new(int32_t hash_mb, int32_t threads);

/* Frees the engine. NULL is ignored. Must not be called while another call on it is running. */
void leorik_free(LeorikEngine *engine);

/* Sets the position: fen (NULL or "" = start position, "startpos" also accepted) followed by
 * moves (NULL or "" = none; space separated UCI moves). The game history is kept for repetition
 * detection. On error the previous position is kept. */
int32_t leorik_set_position(LeorikEngine *engine, const char *fen, const char *moves);

/* Clears the transposition table (call between unrelated games). */
void leorik_new_game(LeorikEngine *engine);

/* Searches for the best move. Search for time_ms milliseconds (<= 0: no time limit) and/or up to
 * depth plies (<= 0: no depth limit). At least one limit is required. Writes the move in UCI
 * notation to out_move (LEORIK_MOVE_LEN bytes). */
int32_t leorik_best_move(LeorikEngine *engine, int32_t time_ms, int32_t depth, char *out_move);

/* Like leorik_best_move() but reports score, mate and principal variation. The final result is
 * written to *out. callback (may be NULL) is invoked after every completed iteration. */
int32_t leorik_analyze(LeorikEngine *engine, int32_t time_ms, int32_t depth, LeorikAnalysis *out,
                       LeorikInfoCallback callback, void *user_data);

/* Stops a running search on this engine (thread safe, non-blocking). */
void leorik_stop(LeorikEngine *engine);

#ifdef __cplusplus
}
#endif

#endif /* LEORIK_H */
