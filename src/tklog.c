/*  tklog.c – implementation for the tklogging facility declared in tklog.h
 *
 *  Compile‑time‑only configuration: the header decides everything through
 *  macros.  This implementation simply honours TKLOG_ACTIVE_FLAGS, the selected
 *  TKLOG_OUTPUT_FN, and TKLOG_OUTPUT_USERPTR.
 *
 *  Cross-platform support:
 *  - Uses pthread for threading (available on Linux, macOS, Windows via MinGW-w64)
 *  - Uses platform-specific high-resolution timing:
 *    - Windows: QueryPerformanceCounter (with GetTickCount64 fallback)
 *    - POSIX: clock_gettime(CLOCK_MONOTONIC) (with gettimeofday fallback)
 *  - Uses standard C memory functions (malloc, free, etc.)
 */

#include <stdlib.h>
#include "tklog.h"
#include <stdio.h>
#include <string.h>
#include <stdarg.h>
#include <pthread.h>
#include <stdbool.h>
#include <time.h>
#include <sys/time.h>
#include <inttypes.h>
#include <signal.h>
#include <unistd.h>

#ifdef _WIN32
#include <windows.h>
#endif

/* -------------------------------------------------------------------------
 *  Internal helpers / globals
 * ------------------------------------------------------------------------- */
static pthread_mutex_t g_tklog_mutex = PTHREAD_MUTEX_INITIALIZER;   /* serialises _tklog() and memory db */
static pthread_rwlock_t g_mem_rwlock = PTHREAD_RWLOCK_INITIALIZER;  /* guards the allocation map       */
static uint64_t         g_start_ms   = 0;                           /* program start in ms             */

/* Store original memory functions to avoid recursion */
#ifdef TKLOG_MEMORY
static void *(*original_malloc)(size_t) = NULL;
static void *(*original_calloc)(size_t, size_t) = NULL;
static void *(*original_realloc)(void *, size_t) = NULL;
static void (*original_free)(void *) = NULL;
#endif

/* Forward declarations */
static void tklog_init_once_impl(void);

/* ==============================  PATH TLS  ============================== */
typedef struct PathStack {
    char   *buf;        /* formatted "a.c:12 → b.c:88 → …" string */
    size_t  len;        /* current length                        */
    size_t  cap;        /* allocated capacity                    */
    int     depth;      /* number of entries                     */
} PathStack;

static pthread_key_t g_tls_path;  /* thread-local storage key */

static void pathstack_free(void *ptr)
{
    PathStack *ps = (PathStack*)ptr;
    if (ps) {
#ifdef TKLOG_MEMORY
        original_free(ps->buf);
        original_free(ps);
#else
        free(ps->buf);
        free(ps);
#endif
    }
}

static PathStack *pathstack_get(void)
{
    PathStack *ps = pthread_getspecific(g_tls_path);
    if (!ps) {
#ifdef TKLOG_MEMORY
        ps = (PathStack*)original_calloc(1, sizeof *ps);
        if (!ps) return NULL; /* out‑of‑mem: just skip tracing */
        ps->cap = 256;
        ps->buf = (char*)original_malloc(ps->cap);
        if (!ps->buf) { original_free(ps); return NULL; }
#else
        ps = (PathStack*)calloc(1, sizeof *ps);
        if (!ps) return NULL; /* out‑of‑mem: just skip tracing */
        ps->cap = 256;
        ps->buf = (char*)malloc(ps->cap);
        if (!ps->buf) { free(ps); return NULL; }
#endif
        ps->buf[0] = '\0';
        pthread_setspecific(g_tls_path, ps);
    }
    return ps;
}

static void pathstack_push(const char *file, int line)
{
    PathStack *ps = pathstack_get();
    if (!ps) {
        return;
    }

    char tmp[128];
    int  n = snprintf(tmp, sizeof tmp, "%s:%d", file, line);
    if (n < 0) {
        return;
    }

    /* ensure space: existing + sep + n + NUL */
    size_t need = ps->len + (ps->depth ? 3 : 0) + n + 1;
    if (need > ps->cap) {
        size_t newcap = ps->cap * 2;
        while (newcap < need) newcap *= 2;
#ifdef TKLOG_MEMORY
        char *newbuf = (char*)original_realloc(ps->buf, newcap);
#else
        char *newbuf = (char*)realloc(ps->buf, newcap);
#endif
        if (!newbuf) {
            return; /* OOM */
        }
        ps->buf = newbuf;
        ps->cap = newcap;
    }

    if (ps->depth) {
        memcpy(ps->buf + ps->len, " \xE2\x86\x92 ", 5);
        ps->len += 5;
    }
    memcpy(ps->buf + ps->len, tmp, n);
    ps->len += n;
    ps->buf[ps->len] = '\0';
    ps->depth += 1;
    // printf("push %s\n", ps->buf);
}

static void pathstack_pop(void)
{
    PathStack *ps = pthread_getspecific(g_tls_path);
    if (!ps || ps->depth == 0) {
        return;
    }

    /* remove last element */
    char *last_sep = NULL;
    if (ps->depth > 1) {
        /* find the penultimate " → " */
        last_sep = (char*)strrchr(ps->buf, '\x20'); /* space before arrow */
        if (last_sep) {
            /* we had space UTF‑8 arrow space; step before that */
            *last_sep = '\0';
            /* now find prev arrow; we need the last arrow occurrence */
            last_sep = strrchr(ps->buf, '\xE2'); /* begin arrow UTF‑8 */
            if (last_sep) last_sep -= 1; /* step to space before arrow */
        }
    }
    if (!last_sep) {
        /* first element */
        ps->buf[0] = '\0';
        ps->len   = 0;
    } else {
        *last_sep = '\0';
        ps->len = strlen(ps->buf);
    }
    ps->depth -= 1;

    // printf("pop  %s\n", ps->buf);
}

/* =============================  MEM TRACK  ============================== */
typedef struct MemEntry {
    void           *ptr;
    size_t          size;
    uint64_t        t_ms;
    pthread_t       tid;
    char            path[128];
    struct MemEntry *next;
} MemEntry;

static MemEntry *g_mem_head = NULL;

static uint64_t get_time_ms(void)
{
#ifdef _WIN32
    /* Windows-specific high-resolution timing */
    LARGE_INTEGER freq, count;
    if (QueryPerformanceFrequency(&freq) && QueryPerformanceCounter(&count)) {
        return (uint64_t)(count.QuadPart * 1000 / freq.QuadPart);
    }
    /* Fallback to GetTickCount64 if QueryPerformanceCounter fails */
    return (uint64_t)GetTickCount64();
#else
    /* POSIX timing - try clock_gettime first, fallback to gettimeofday */
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) == 0) {
        return (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
    }
    /* Fallback to gettimeofday if clock_gettime fails */
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (uint64_t)tv.tv_sec * 1000 + (uint64_t)tv.tv_usec / 1000;
#endif
}

static void mem_add(void *ptr, size_t size, const char *file, int line)
{
    if (!ptr) return;
    pthread_rwlock_wrlock(&g_mem_rwlock);
#ifdef TKLOG_MEMORY
    MemEntry *e = (MemEntry*)original_malloc(sizeof *e);
#else
    MemEntry *e = (MemEntry*)malloc(sizeof *e);
#endif
    if (e) {
        e->ptr  = ptr;
        e->size = size;
        e->t_ms = get_time_ms();
        e->tid  = pthread_self();
        
        // Get the full call path from PathStack
        PathStack *ps = pathstack_get();
        if (ps && ps->buf[0]) {
            // Show call stack path + current file:line (same format as _tklog)
            snprintf(e->path, sizeof e->path, "%s → %s:%d", ps->buf, file, line);
        } else {
            // Show just current file:line when no call stack
            snprintf(e->path, sizeof e->path, "%s:%d", file, line);
        }
        
        e->next = g_mem_head;
        g_mem_head = e;
    }
    pthread_rwlock_unlock(&g_mem_rwlock);
}

static void mem_update(void *oldptr, void *newptr, size_t newsize)
{
    pthread_rwlock_wrlock(&g_mem_rwlock);
    for (MemEntry *e = g_mem_head; e; e = e->next) {
        if (e->ptr == oldptr) {
            e->ptr  = newptr;
            e->size = newsize;
            break;
        }
    }
    pthread_rwlock_unlock(&g_mem_rwlock);
}

static bool mem_remove(void *ptr)
{
    pthread_rwlock_wrlock(&g_mem_rwlock);
    MemEntry **pp = &g_mem_head;
    while (*pp) {
        if ((*pp)->ptr == ptr) {
            MemEntry *dead = *pp;
            *pp = dead->next;
#ifdef TKLOG_MEMORY
            original_free(dead);
#else
            free(dead);
#endif
            pthread_rwlock_unlock(&g_mem_rwlock);
            return true;
        }
        pp = &(*pp)->next;
    }
    pthread_rwlock_unlock(&g_mem_rwlock);
    return false;
}

/* =============================  API impl  ============================== */


static void signal_handler(int sig) {
    // Minimal message (async-safe: use write() instead of printf)
    const char *msg = "\nCaught signal (likely segfault), dumping memory before exit:\n";
    write(STDERR_FILENO, msg, strlen(msg));

    tklog_memory_dump();  // Call dump (note: not fully async-safe, but useful for dev)

    // Re-raise signal or exit to avoid infinite loops
    _exit(EXIT_FAILURE);
}

bool tklog_output_stdio(const char *msg, void *user)
{
    (void)user;
    printf("%s", msg);
    return true;
}

static void tklog_init_once(void)
{
    static pthread_once_t once_control = PTHREAD_ONCE_INIT;
    pthread_once(&once_control, tklog_init_once_impl);
}

static void tklog_init_once_impl(void)
{
    pthread_key_create(&g_tls_path, pathstack_free);
    g_start_ms = get_time_ms();
    
#ifdef TKLOG_MEMORY
    /* Store original memory functions before they get redefined */
    original_malloc = malloc;
    original_calloc = calloc;
    original_realloc = realloc;
    original_free = free;
#endif
    
#ifdef TKLOG_MEMORY
    signal(SIGSEGV, signal_handler);
    signal(SIGABRT, signal_handler);
    signal(SIGINT, signal_handler);
    atexit(tklog_memory_dump);
#endif
}

void _tklog(uint32_t flags, tklog_level_t level, int line, const char *file, const char *fmt, ...)
{
    tklog_init_once();

    static const char *levelstr[] = {
        "DEBUG    ", "INFO     ", "NOTICE   ", "WARNING  ",
        "ERROR    ", "CRITICAL ", "ALERT    ", "EMERGENCY" };

    char msgbuf[2048];
    char *p = msgbuf;
    size_t n = 0;

    /* level */
    if (flags & TKLOG_INIT_F_LEVEL) {
        n = snprintf(p, sizeof msgbuf, "%s | ", levelstr[level]);
        p += n;
    }

    /* time */
    if (flags & TKLOG_INIT_F_TIME) {
        uint64_t t_ms = get_time_ms() - g_start_ms;
        n = snprintf(p, sizeof msgbuf - (p - msgbuf), "%" PRIu64 "ms | ", (uint64_t)t_ms);
        p += n;
    }

    /* thread */
    if (flags & TKLOG_INIT_F_THREAD) {
        pthread_t tid = pthread_self();
        n = snprintf(p, sizeof msgbuf - (p - msgbuf), "tid 0x%lX | ", (unsigned long)tid);
        p += n;
    }

    /* path */
    if (flags & TKLOG_INIT_F_PATH) {
        PathStack *ps = pathstack_get();
        if (ps && ps->buf[0]) {
            /* Show call stack path + current file:line */
            n = snprintf(p, sizeof msgbuf - (p - msgbuf), "%s → %s:%d | ", ps->buf, file, line);
            p += n;
        } else {
            /* Show just current file:line when no call stack */
            n = snprintf(p, sizeof msgbuf - (p - msgbuf), "%s:%d | ", file, line);
            p += n;
        }
    }

    /* user message */
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(p, sizeof msgbuf - (p - msgbuf), fmt, ap);
    va_end(ap);

    /* ensure newline */
    size_t len = strlen(msgbuf);
    if (len + 1 < sizeof msgbuf && msgbuf[len - 1] != '\n') {
        msgbuf[len]   = '\n';
        msgbuf[len+1] = '\0';
    }

    /* lock, write, unlock */
    pthread_mutex_lock(&g_tklog_mutex);
    TKLOG_OUTPUT_FN(msgbuf, TKLOG_OUTPUT_USERPTR);
    pthread_mutex_unlock(&g_tklog_mutex);
}

/* -------------------------  Scope tracing  ----------------------------- */
#ifdef TKLOG_SCOPE
    void _tklog_scope_start(int line, const char *file) { tklog_init_once(); pathstack_push(file, line); }
    void _tklog_scope_end  (void)                       { pathstack_pop(); }
#endif

/* ------------------------- Time tracing ----------------------------- */
#ifdef TKLOG_TIMER
    typedef struct CallPathTime {
        uint64_t total_time;
        uint64_t count;
    } CallPathTime;
    #define NAME call_path_time_table
    #define KEY_TY char*
    #define VAL_TY CallPathTime
    #include "verstable.h"
    typedef struct TimeTracker {
        uint64_t total_time;
        uint64_t count;
        call_path_time_table call_paths;
    } TimeTracker;
    #define NAME time_tracker_table
    #define KEY_TY char*
    #define VAL_TY TimeTracker
    #include "verstable.h"

    typedef struct TimerState {
        time_tracker_table table;
        char* current_start_location;
        char* current_start_path;
        uint64_t start_time;
    } TimerState;

    static pthread_key_t g_tls_timer_state;

    static void timer_state_free(void *ptr) {
        TimerState *ts = (TimerState*)ptr;
        if (ts) {
            // Free all allocated keys in call_paths and the outer table
            for (time_tracker_table_itr itr = time_tracker_table_begin(&ts->table);
                 !time_tracker_table_is_end(itr);
                 itr = time_tracker_table_next(itr)) {
                TimeTracker *tracker = &itr.data->val;
                for (call_path_time_table_itr cp_itr = call_path_time_table_begin(&tracker->call_paths);
                     !call_path_time_table_is_end(cp_itr);
                     cp_itr = call_path_time_table_next(cp_itr)) {
                    free((char*)cp_itr.data->key);
                }
                // Assuming verstable has _clear; otherwise, it's handled by iteration above
                call_path_time_table_clear(&tracker->call_paths);
                free((char*)itr.data->key);
            }
            time_tracker_table_clear(&ts->table);
            free(ts->current_start_location);
            free(ts->current_start_path);
            free(ts);
        }
    }

    static TimerState *get_timer_state(void) {
        TimerState *ts = pthread_getspecific(g_tls_timer_state);
        if (!ts) {
            ts = calloc(1, sizeof(TimerState));
            if (!ts) {
                printf("tklog: out of memory for TimerState\n");
                return NULL;
            }
            time_tracker_table_init(&ts->table);
            ts->current_start_location = NULL;
            ts->current_start_path = NULL;
            ts->start_time = 0;
            pthread_setspecific(g_tls_timer_state, ts);
        }
        return ts;
    }

    static void tklog_init_once_impl(void) {
        // ... (existing code)
        pthread_key_create(&g_tls_timer_state, timer_state_free);
    }

    void _tklog_timer_init() {
        TimerState *ts = get_timer_state();
        if (ts) {
            // Re-init if needed (clears existing data)
            time_tracker_table_clear(&ts->table);
            free(ts->current_start_location);
            ts->current_start_location = NULL;
            free(ts->current_start_path);
            ts->current_start_path = NULL;
            ts->start_time = 0;
        }
    }

    void _tklog_timer_start(int line, const char* file) {
        TimerState *ts = get_timer_state();
        if (!ts) return;
        if (ts->current_start_location != NULL) {
            printf("tklog: tklog_timer_start is called without a corresponding tklog_timer_stop beforehand\n");
            return;
        }
        PathStack *ps = pathstack_get();
        if (!ps) {
            printf("tklog: internal error. pathstack_get failed\n");
            return;
        }
        char temp_location[128];
        int n = snprintf(temp_location, sizeof temp_location, "%s:%d", file, line);
        if (n < 0) {
            printf("tklog: internal error. snprintf failed\n");
            return;
        }
        time_tracker_table_itr itr = time_tracker_table_get(&ts->table, temp_location);
        if (time_tracker_table_is_end(itr)) {
            char* key = strdup(temp_location);
            if (!key) {
                printf("tklog: out of memory for strdup\n");
                return;
            }
            TimeTracker tracker = {0};
            tracker.total_time = 0;
            tracker.count = 0;
            call_path_time_table_init(&tracker.call_paths);
            itr = time_tracker_table_insert(&ts->table, key, tracker);
            if (time_tracker_table_is_end(itr)) {
                printf("tklog: time_table is out of memory because time_tracker_table_insert failed\n");
                free(key);
                return;
            }
        }
        ts->current_start_location = strdup(temp_location);
        if (!ts->current_start_location) {
            printf("tklog: out of memory for strdup\n");
            return;
        }
        ts->current_start_path = strdup(ps->buf ? ps->buf : "");
        if (!ts->current_start_path) {
            printf("tklog: out of memory for strdup\n");
            free(ts->current_start_location);
            ts->current_start_location = NULL;
            return;
        }
        ts->start_time = get_time_ms();
    }

    void _tklog_timer_stop(int line, const char* file) {
        uint64_t end_time = get_time_ms();
        TimerState *ts = get_timer_state();
        if (!ts) return;
        if (ts->current_start_location == NULL) {
            printf("tklog: tklog_timer_stop is called without a corresponding tklog_timer_start beforehand\n");
            return;
        }
        PathStack *ps = pathstack_get();
        if (!ps) {
            printf("tklog: internal error. pathstack_get failed\n");
            free(ts->current_start_location);
            ts->current_start_location = NULL;
            free(ts->current_start_path);
            ts->current_start_path = NULL;
            return;
        }
        char stop_location[128];
        int n = snprintf(stop_location, sizeof stop_location, "%s:%d", file, line);
        if (n < 0) {
            printf("tklog: internal error. snprintf failed\n");
            free(ts->current_start_location);
            ts->current_start_location = NULL;
            free(ts->current_start_path);
            ts->current_start_path = NULL;
            return;
        }
        size_t needed = strlen(ts->current_start_path) + strlen(ts->current_start_location) + strlen(ps->buf) + strlen(stop_location) + 10;
        char* call_path = malloc(needed);
        if (!call_path) {
            printf("tklog: out of memory for call_path\n");
            free(ts->current_start_location);
            ts->current_start_location = NULL;
            free(ts->current_start_path);
            ts->current_start_path = NULL;
            return;
        }
        snprintf(call_path, needed, "%s → %s : %s → %s", ts->current_start_path, ts->current_start_location, ps->buf, stop_location);
        time_tracker_table_itr itr = time_tracker_table_get(&ts->table, ts->current_start_location);
        if (time_tracker_table_is_end(itr)) {
            printf("tklog: internal error. no tracker for start location\n");
            free(call_path);
            free(ts->current_start_location);
            ts->current_start_location = NULL;
            free(ts->current_start_path);
            ts->current_start_path = NULL;
            return;
        }
        TimeTracker* tracker = &itr.data->val;
        uint64_t delta = end_time - ts->start_time;
        tracker->total_time += delta;
        tracker->count++;
        call_path_time_table_itr cp_itr = call_path_time_table_get(&tracker->call_paths, call_path);
        if (call_path_time_table_is_end(cp_itr)) {
            CallPathTime cpt = {0};
            cpt.total_time = delta;
            cpt.count = 1;
            cp_itr = call_path_time_table_insert(&tracker->call_paths, call_path, cpt);
            if (call_path_time_table_is_end(cp_itr)) {
                printf("tklog: call_paths is out of memory because call_path_time_table_insert failed\n");
                free(call_path);
            }
        } else {
            CallPathTime* cpt = &cp_itr.data->val;
            cpt->total_time += delta;
            cpt->count++;
            free(call_path);  // Since key already exists, free the new one
        }
        free(ts->current_start_location);
        ts->current_start_location = NULL;
        free(ts->current_start_path);
        ts->current_start_path = NULL;
        ts->start_time = 0;
    }

    void tklog_timer_print(void) {
        TimerState *ts = get_timer_state();
        if (!ts) return;
        for (time_tracker_table_itr itr = time_tracker_table_begin(&ts->table);
             !time_tracker_table_is_end(itr);
             itr = time_tracker_table_next(itr)) {
            const char* start_loc = itr.data->key;
            TimeTracker* tracker = &itr.data->val;
            double avg = tracker->count > 0 ? (double)tracker->total_time / tracker->count : 0.0;
            printf("%s: %" PRIu64 " ms, %" PRIu64 " calls, avg %.2f ms\n",
                   start_loc, tracker->total_time, tracker->count, avg);
            for (call_path_time_table_itr cp_itr = call_path_time_table_begin(&tracker->call_paths);
                 !call_path_time_table_is_end(cp_itr);
                 cp_itr = call_path_time_table_next(cp_itr)) {
                const char* path = cp_itr.data->key;
                CallPathTime* cpt = &cp_itr.data->val;
                double cp_avg = cpt->count > 0 ? (double)cpt->total_time / cpt->count : 0.0;
                printf("    %s: %" PRIu64 " ms, %" PRIu64 " calls, avg %.2f ms\n",
                       path, cpt->total_time, cpt->count, cp_avg);
            }
        }
    }
#endif
/* -------------------------  Memory API  -------------------------------- */
#ifdef TKLOG_MEMORY
    void *tklog_malloc(size_t size, const char *file, int line)
    {
        void *p = original_malloc(size);
        mem_add(p, size, file, line);
        return p;
    }

    void *tklog_calloc(size_t nmemb, size_t size, const char *file, int line)
    {
        void *p = original_calloc(nmemb, size);
        mem_add(p, nmemb * size, file, line);
        return p;
    }
    void *tklog_realloc(void *ptr, size_t size, const char *file, int line)
    {
        if (size == 0) {
            tklog_free(ptr, file, line);
            return NULL;
        }
        if (ptr == NULL) {
            return tklog_malloc(size, file, line);
        }
        void *newp = original_realloc(ptr, size);
        if (newp != NULL) {
            mem_update(ptr, newp, size);
        }
        return newp;
    }

    void tklog_free(void *ptr, const char *file, int line)
    {
        // Add NULL pointer check
        if (!ptr) {
            _tklog(TKLOG_ACTIVE_FLAGS, TKLOG_LEVEL_ERROR, line, file, "you tried to free NULL ptr");
            exit(EXIT_FAILURE);
        }
        
        // Check if pointer exists in tracking (detects double-free)
        bool found = mem_remove(ptr);
        if (!found) {
            _tklog(TKLOG_ACTIVE_FLAGS, TKLOG_LEVEL_ERROR, line, file, "you tried to free a pointer that was not allocated");
            exit(EXIT_FAILURE);
        }
        
        original_free(ptr);
    }
#endif 
#if defined(TKLOG_MEMORY) && defined(TKLOG_MEMORY_PRINT_ON_EXIT)
    void tklog_memory_dump(void)
    {
        pthread_rwlock_rdlock(&g_mem_rwlock);
        
        // Print header like debug.c does
        char header[] = "\nunfreed memory:\n";
        pthread_mutex_lock(&g_tklog_mutex);
        TKLOG_OUTPUT_FN(header, TKLOG_OUTPUT_USERPTR);
        pthread_mutex_unlock(&g_tklog_mutex);
        
        // Print each memory entry with time, thread, and full path
        for (MemEntry *e = g_mem_head; e; e = e->next) {
            char linebuf[512]; // Increased buffer size for longer paths
            uint64_t t_ms = e->t_ms - g_start_ms;
            snprintf(linebuf, sizeof linebuf, "\t%" PRIu64 "ms | tid 0x%lX | address %p | %zu bytes | at %s\n",
                          t_ms, (unsigned long)e->tid, e->ptr, e->size, e->path);
            pthread_mutex_lock(&g_tklog_mutex);
            TKLOG_OUTPUT_FN(linebuf, TKLOG_OUTPUT_USERPTR);
            pthread_mutex_unlock(&g_tklog_mutex);
        }
        
        // Print trailing newline like debug.c does
        char footer[] = "\n";
        pthread_mutex_lock(&g_tklog_mutex);
        TKLOG_OUTPUT_FN(footer, TKLOG_OUTPUT_USERPTR);
        pthread_mutex_unlock(&g_tklog_mutex);
        
        pthread_rwlock_unlock(&g_mem_rwlock);
    }
#else /* TKLOG_MEMORY AND TKLOG_MEMORY_PRINT_ON_EXIT not defined */
    void tklog_memory_dump(void)
    {
        printf("tklog_memory_dump: TKLOG_MEMORY_PRINT_ON_EXIT must be defined to track and dump memory allocations\n");
    }
#endif /* TKLOG_MEMORY */