/*
 * test_map.c – Unit tests for the current map.* API
 *
 * Build example (adjust source list as needed):
 *   gcc -std=c11 -Wall -Wextra -DTKLOG_MEMORY \
 *       test_map.c map.c verstable.c -lSDL3 -pthread -o test_map
 *
 * If TKLOG_MEMORY is **not** needed, omit the -DTKLOG_MEMORY flag.
 *
 * Expected run-time output:
 *   All map tests passed ✅
 */

/*  ────────────────────────────────────────────────────────────────
 *  Include-order note when TKLOG_MEMORY is defined
 *  ------------------------------------------------
 *  <stdlib.h> must be included **before** tklog.h so the standard
 *  malloc/realloc/free prototypes are parsed *before* tklog.h re-
 *  defines them as tracking wrappers.
 *  ──────────────────────────────────────────────────────────────── */

#include <stdlib.h>      /* malloc / realloc / free prototypes */
#include <assert.h>
#include <stdio.h>
#include <stdbool.h>
#include <SDL3/SDL.h>

#include "map.h"
#include "tklog.h"

/* -------------------------------------------------------------------------
 *  Basic single-threaded CRUD round trip
 * ------------------------------------------------------------------------- */
static void basic_insert_get_erase(void)
{
    int *value = malloc(sizeof *value);
    *value = 42;

    /* Insert */
    map_id_t id = map_Insert("answer", value);
    assert(id != MAP_NULL_ID);

    /* Size should be 1 */
    assert(map_Size() == 1);

    /* Lookup by id */
    void **pp = map_Get(id);
    assert(pp && *pp == value && *(int *)(*pp) == 42);

    /* Lookup by string */
    void **pp2 = map_GetByString("answer");
    assert(pp2 && *pp2 == value);

    /* Existence helpers */
    assert(map_HasKeyID(id));
    assert(map_HasKeyString("answer"));

    /* Erase by id */
    assert(map_Erase(id));
    free(value);

    /* Size returns to 0 */
    assert(map_Size() == 0);
}

/* -------------------------------------------------------------------------
 *  Erase-by-string convenience wrapper
 * ------------------------------------------------------------------------- */
static void erase_by_string(void)
{
    int *value = malloc(sizeof *value);
    *value = 99;

    map_id_t id = map_Insert("ninety-nine", value);
    assert(id != MAP_NULL_ID);
    assert(map_Size() == 1);

    assert(map_EraseByString("ninety-nine"));
    free(value);

    assert(map_Size() == 0);
}

/* -------------------------------------------------------------------------
 *  Multi-threaded visibility test – reader just observes the map.
 *  (The map API itself is *not* thread-safe for concurrent writes,
 *   so this test performs *only* read-after-write.)
 * ------------------------------------------------------------------------- */
static int reader_thread(void *userdata)
{
    (void)userdata;
    return (int)map_Size();
}

static void multithread_read_visibility(void)
{
    int *value = malloc(sizeof *value);
    *value = 123;

    map_id_t id = map_Insert("one-two-three", value);
    assert(id != MAP_NULL_ID);

    /* Start reader thread after insertion is complete (no locks available) */
    SDL_Thread *t = SDL_CreateThread(reader_thread, "reader", NULL);
    assert(t != NULL);

    int thread_ret = 0;
    SDL_WaitThread(t, &thread_ret);

    /* Reader should have seen exactly one element */
    assert(thread_ret == 1);

    /* Clean-up */
    assert(map_Erase(id));
    free(value);
}

/* ===================================================================== */
int main(void)
{
    if (!SDL_Init(SDL_INIT_EVENTS)) {   /* SDL 3: 0 initialises all subsystems */
        fprintf(stderr, "SDL_Init failed: %s\n", SDL_GetError());
        return 1;
    }

    basic_insert_get_erase();
    erase_by_string();
    multithread_read_visibility();

    printf("All map tests passed ✅\n");

    SDL_Quit();
    return 0;
}