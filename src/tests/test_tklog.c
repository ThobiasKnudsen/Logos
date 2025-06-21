/*
 * tklog_full_test.c — Comprehensive self-test for the tklog logging library
 *
 * Build (GCC / Clang, POSIX):
 *   gcc -std=c11 -Wall -Wextra -I. \
 *       -DTKLOG_MEMORY -DTKLOG_SCOPE \
 *       -DTKLOG_SHOW_LOG_LEVEL -DTKLOG_SHOW_TIME -DTKLOG_SHOW_THREAD -DTKLOG_SHOW_PATH \
 *       -DTKLOG_DEBUG    -DTKLOG_INFO    -DTKLOG_NOTICE \
 *       -DTKLOG_WARNING  -DTKLOG_ERROR   -DTKLOG_CRITICAL \
 *       -DTKLOG_ALERT    -DTKLOG_EMERGENCY \
 *       -DTKLOG_EXIT_ON_WARNING  -DTKLOG_EXIT_ON_ERROR  -DTKLOG_EXIT_ON_CRITICAL \
 *       -DTKLOG_EXIT_ON_ALERT    -DTKLOG_EXIT_ON_EMERGENCY \
 *       -DTKLOG_OUTPUT_FN=buffer_output \
 *       tklog_full_test.c tklog.c -lSDL3 -pthread -o tklog_full_test
 *
 * On Windows/MSVC remove the _EXIT_ON_* flags (because fork() isn’t available)
 * or #define TKLOG_SKIP_EXIT_TESTS before compiling.
 *
 * Expected:
 *   • One line for every severity level, with level/time/thread/path prefixes.
 *   • Nested scope-paths printed.
 *   • Worker threads interleave their log messages.
 *   • A controlled realloc/free pair plus one intentional leak.
 *   • For POSIX, five fork() sub-tests; each child dies via the matching
 *     TKLOG_EXIT_ON_* macro and the parent reports its exit status.
 *   • Final call to tklog_memory_dump() shows exactly one live allocation.
 */

#include <SDL3/SDL.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "tklog.h"

/* -------------------------------------------------------------------------
 *  Optional fork()/wait helpers (POSIX only)                               */
#if defined(__unix__) || defined(__APPLE__) || defined(__linux__)
    #include <sys/wait.h>
    #include <unistd.h>
    #define TKLOG_HAS_FORK 1
#else
    #define TKLOG_HAS_FORK 0
#endif

/* -------------------------------------------------------------------------
 *  Custom output callback — wired in with -DTKLOG_OUTPUT_FN=buffer_output  */
bool buffer_output(const char *msg, void *user)
{
    (void)user;                /* not used */
    /* Prefix to make it obvious we’re in the custom sink. */
    fprintf(stderr, "[buffer] %s\n", msg);
    /* Returning false would tell tklog the write failed. */
    return true;
}

/* -------------------------------------------------------------------------
 *  Worker thread — showcases TKLOG_SHOW_THREAD                            */
static int worker_thread(void *userdata)
{
    int id = (int)(intptr_t)userdata;
    tklog_info("Worker thread #%d started", id);
    for (int i = 0; i < 4; ++i) {
        tklog_debug("Thread %d — iteration %d", id, i);
        SDL_Delay(40);
    }
    tklog_notice("Worker thread #%d finished", id);
    return 0;
}

/* -------------------------------------------------------------------------
 *  Scope demo — nested & recursive                                        */
static void inner_scope(int depth)
{
    tklog_scope({
        tklog_notice("inner_scope depth=%d", depth);
        if (depth > 0)  inner_scope(depth - 1);
    });
}

static void demo_scopes(void)
{
    tklog_scope({
        tklog_notice("Entered outer scope");
        inner_scope(2);
        tklog_notice("Leaving outer scope");
    });
}

/* -------------------------------------------------------------------------
 *  Level-by-level smoke test                                              */
static void demo_all_levels(void)
{
    tklog_debug     ("Debug message");
    tklog_info      ("Info message");
    tklog_notice    ("Notice message");
    tklog_warning   ("Warning message");
    tklog_error     ("Error message");
    tklog_critical  ("Critical message");
    tklog_alert     ("Alert message");
    tklog_emergency ("Emergency message");
}

/* -------------------------------------------------------------------------
 *  Memory-tracker demo                                                    */
static void demo_memory(void)
{
    /* 1) realloc dance */
    char *dyn = malloc(32);
    strcpy(dyn, "grow-me");
    dyn = realloc(dyn, 128);
    strcat(dyn, " (bigger now)");
    free(dyn);

    /* 2) Intentional leak */
    char *leak = malloc(64);
    char *leak_1 = malloc(63);
    strcpy(leak, "This block is intentionally leaked");
    strcpy(leak_1, "This block is intentionally leaked");
    (void)leak;
}

/* -------------------------------------------------------------------------
 *  Exit-on-level macro tests (POSIX)                                      */
#if TKLOG_HAS_FORK && !defined(TKLOG_SKIP_EXIT_TESTS)

/* --- 1. one wrapper-function per macro --------------------------------- */
static void trigger_warning  (void) { tklog_warning  ("EXIT test"); }
static void trigger_error    (void) { tklog_error    ("EXIT test"); }
static void trigger_critical (void) { tklog_critical ("EXIT test"); }
static void trigger_alert    (void) { tklog_alert    ("EXIT test"); }
static void trigger_emergency(void) { tklog_emergency("EXIT test"); }

/* --- 2. generic fork/wait helper --------------------------------------- */
typedef void (*trigger_fn_t)(void);

static void run_exit_case(const char *label, trigger_fn_t fn)
{
    pid_t pid = fork();
    if (pid == 0) {                 /* child */
        fn();                       /* macro calls _Exit() → never returns */
        _Exit(99);                  /* safety net */
    }
    else if (pid > 0) {             /* parent */
        int status = 0;
        waitpid(pid, &status, 0);
        if (WIFEXITED(status))
            printf("[exit-test] %s: child exited with %d\n",
                   label, WEXITSTATUS(status));
        else
            printf("[exit-test] %s: child terminated abnormally\n", label);
    }
    else {
        perror("fork");
    }
}

/* --- 3. drive all five EXIT_ON_* macros -------------------------------- */
static void demo_exit_macros(void)
{
    puts("[exit-test] Running EXIT_ON_* cases (each child should exit)");
    run_exit_case("TKLOG_EXIT_ON_WARNING",   trigger_warning);
    run_exit_case("TKLOG_EXIT_ON_ERROR",     trigger_error);
    run_exit_case("TKLOG_EXIT_ON_CRITICAL",  trigger_critical);
    run_exit_case("TKLOG_EXIT_ON_ALERT",     trigger_alert);
    run_exit_case("TKLOG_EXIT_ON_EMERGENCY", trigger_emergency);
}

#else   /* fork unavailable / skipped */
static void demo_exit_macros(void)
{
    puts("[exit-test] EXIT_ON_* tests skipped on this platform/build.");
}
#endif  /* TKLOG_HAS_FORK */

/* -------------------------------------------------------------------------
 *  Main                                                                   */
int main(int argc, char **argv)
{
    (void)argc; (void)argv;

    if (!SDL_Init(SDL_INIT_EVENTS)) {
        fprintf(stderr, "SDL_Init failed: %s\n", SDL_GetError());
        return 1;
    }

    tklog_info("tklog FULL self-test started");

    /* Threaded logging demo (two workers). */
    SDL_Thread *t1 = SDL_CreateThread(worker_thread, "worker-1",
                                      (void *)(intptr_t)1);
    SDL_Thread *t2 = SDL_CreateThread(worker_thread, "worker-2",
                                      (void *)(intptr_t)2);

    /* Core feature demos (run in main thread). */
    demo_all_levels();
    demo_scopes();
    demo_memory();
    demo_exit_macros();

    SDL_WaitThread(t1, NULL);
    SDL_WaitThread(t2, NULL);

    /* Dump outstanding allocations — expect exactly one leak. */
    tklog_memory_dump();

    tklog_info("Test complete — expect one intentional leak above");

    SDL_Quit();
    return 0;
}
