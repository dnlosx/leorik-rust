/* Links against the static library through the public header; built by scripts/test-c-api.sh */
#include "leorik.h"
#include <stdio.h>
#include <string.h>

#define CHECK(cond) do { if (!(cond)) { fprintf(stderr, "FAILED %s:%d: %s\n", __FILE__, __LINE__, #cond); return 1; } } while (0)

static int iterations = 0;
static void on_info(const LeorikAnalysis *info, void *user) {
    (void)user;
    iterations++;
    printf("  depth %d cp %d (white %d) mate %d nodes %lld pv %s\n", info->depth, info->score_cp,
           info->white_score_cp, info->mate, (long long)info->nodes, info->pv);
}

int main(void) {
    printf("%s\n", leorik_version());
    LeorikEngine *engine = leorik_new(0, 1);
    CHECK(engine != NULL);

    CHECK(leorik_set_position(engine, NULL, "e2e4 e7e5 g1f3") == LEORIK_OK);
    LeorikAnalysis a;
    CHECK(leorik_analyze(engine, 0, 12, &a, on_info, NULL) == LEORIK_OK);
    CHECK(iterations == 12);
    CHECK(strcmp(a.best_move, "g8f6") == 0);
    CHECK(a.nodes == 61874);

    char move[LEORIK_MOVE_LEN];
    CHECK(leorik_set_position(engine, "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", NULL) == LEORIK_OK);
    CHECK(leorik_best_move(engine, 200, 0, move) == LEORIK_OK);
    CHECK(strcmp(move, "a1a8") == 0);
    CHECK(leorik_set_position(engine, "garbage", NULL) == LEORIK_ERR_POSITION);

    leorik_new_game(engine);
    leorik_free(engine);
    printf("C API test passed\n");
    return 0;
}
