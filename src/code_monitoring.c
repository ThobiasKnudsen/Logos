/*  cm.c – implementation for the cmging facility declared in cm.h
 *
 *  Compile‑time‑only configuration: the header decides everything through
 *  macros.  This implementation simply honours CM_FLAGS, the selected
 *  CM_OUTPUT_FN, and CM_OUTPUT_USERPTR.
 *
 *  Cross-platform support:
 *  - Uses pthread for threading (available on Linux, macOS, Windows via MinGW-w64)
 *  - Uses platform-specific high-resolution timing:
 *    - Windows: QueryPerformanceCounter (with GetTickCount64 fallback)
 *    - POSIX: clock_gettime(CLOCK_MONOTONIC) (with gettimeofday fallback)
 *  - Uses standard C memory functions (malloc, free, etc.)
 */

#include "code_monitoring.h"
#include <stdlib.h>
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

#define PATHSTACK_CAPACITY 512
#define PATH_CAPACITY 256

/* -------------------------------------------------------------------------
 *  Internal helpers / globals
 * ------------------------------------------------------------------------- */
static pthread_mutex_t g_cm_mutex = PTHREAD_MUTEX_INITIALIZER;   /* serialises _cm() and memory db */
static pthread_rwlock_t g_mem_rwlock = PTHREAD_RWLOCK_INITIALIZER;  /* guards the allocation map       */
static uint64_t         g_start_ms   = 0;                           /* program start in ms             */

/* Store original memory functions to avoid recursion */
#ifdef CM_SHOW_MEMORY
static void *(*original_malloc)(size_t) = NULL;
static void *(*original_calloc)(size_t, size_t) = NULL;
static void *(*original_realloc)(void *, size_t) = NULL;
static char *(*original_strdup)(const char *) = NULL;
static void (*original_free)(void *) = NULL;
#endif

/* Forward declarations */
static void cm_init_once_impl(void);
static void cm_init_once(void);

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
#ifdef CM_SHOW_MEMORY
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
#ifdef CM_SHOW_MEMORY
        ps = (PathStack*)original_calloc(1, sizeof *ps);
        if (!ps) return NULL; /* out‑of‑mem: just skip tracing */
        ps->cap = PATHSTACK_CAPACITY;
        ps->buf = (char*)original_malloc(ps->cap);
        if (!ps->buf) { original_free(ps); return NULL; }
#else
        ps = (PathStack*)calloc(1, sizeof *ps);
        if (!ps) return NULL; /* out‑of‑mem: just skip tracing */
        ps->cap = PATHSTACK_CAPACITY;
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

    char tmp[PATH_CAPACITY];
    int  n = snprintf(tmp, sizeof tmp, "%s:%d", file, line);
    if (n < 0) {
        return;
    }

    /* ensure space: existing + sep + n + NUL */
    size_t need = ps->len + (ps->depth ? 3 : 0) + n + 1;
    if (need > ps->cap) {
        size_t newcap = ps->cap * 2;
        while (newcap < need) newcap *= 2;
#ifdef CM_SHOW_MEMORY
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
        memcpy(ps->buf + ps->len, " -> ", 4);
        ps->len += 4;
    }
    memcpy(ps->buf + ps->len, tmp, n);
    ps->len += n;
    ps->buf[ps->len] = '\0';
    ps->depth += 1;
    // printf("push %s\n", ps->buf);
}

static void pathstack_pop(void) {
    PathStack *ps = pthread_getspecific(g_tls_path);
    if (!ps || ps->depth == 0) {
        return;
    }

    char *last_sep = NULL;
    if (ps->depth > 1) {
        /* find the penultimate " -> " */
        last_sep = (char*)strrchr(ps->buf, ' '); /* space after -> */
        if (last_sep) {
            *last_sep = '\0';
            /* now find prev arrow; we need the last arrow occurrence */
            last_sep = strrchr(ps->buf, '-'); /* start of -> (the '-') */
            if (last_sep) last_sep -= 1; /* step to space before -> */
        }
    }
    if (!last_sep) {
        /* first element */
        ps->buf[0] = '\0';
        ps->len = 0;
    } else {
        *last_sep = '\0';
        ps->len = strlen(ps->buf);
    }
    ps->depth -= 1;
}

/* =============================  MEM TRACK  ============================== */
typedef struct MemEntry {
    void           *ptr;
    size_t          size;
    uint64_t        t_ms;
    pthread_t       tid;
    char            path[PATH_CAPACITY];
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

static uint64_t get_time_us(void)
{
#ifdef _WIN32
    /* Windows-specific high-resolution timing */
    LARGE_INTEGER freq, count;
    if (QueryPerformanceFrequency(&freq) && QueryPerformanceCounter(&count)) {
        return (uint64_t)(count.QuadPart * 1000000ULL / freq.QuadPart)
    }
    /* Fallback to GetTickCount64 if QueryPerformanceCounter fails */
    return (uint64_t)GetTickCount64();
#else
    /* POSIX timing - try clock_gettime first, fallback to gettimeofday */
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) == 0) {
        return (uint64_t)ts.tv_sec * 1000000ULL + (uint64_t)ts.tv_nsec / 1000;
    }
    /* Fallback to gettimeofday if clock_gettime fails */
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (uint64_t)tv.tv_sec * 1000000ULL + (uint64_t)tv.tv_usec;
#endif
}

static void mem_add(void *ptr, size_t size, const char *file, int line)
{
    if (!ptr) return;
    pthread_rwlock_wrlock(&g_mem_rwlock);
#ifdef CM_SHOW_MEMORY
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
            // Show call stack path + current file:line (same format as _cm)
            snprintf(e->path, sizeof e->path, "%s -> %s:%d", ps->buf, file, line);
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
#ifdef CM_SHOW_MEMORY
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

/* ------------------------- Time tracing ----------------------------- */
#ifdef CM_SHOW_TIMER
    static void vt_free_str(char *str) { free(str); }
    typedef struct CallPathTime {
        uint64_t total_time_us;
        uint64_t count;
    } CallPathTime;
    #define NAME call_path_time_table
    #define KEY_TY char*
    #define VAL_TY CallPathTime
    #define HASH_FN vt_hash_string
    #define CMPR_FN vt_cmpr_string
    #define KEY_DTOR_FN vt_free_str
    #include "verstable.h"
    typedef struct TimeTracker {
        uint64_t total_time_us;
        uint64_t count;
        call_path_time_table call_paths;
    } TimeTracker;
    static void vt_time_tracker_dtor(TimeTracker tracker) { call_path_time_table_cleanup(&tracker.call_paths); }
    #define NAME time_tracker_table
    #define KEY_TY char*
    #define VAL_TY TimeTracker
    #define HASH_FN vt_hash_string
    #define CMPR_FN vt_cmpr_string
    #define KEY_DTOR_FN vt_free_str
    #define VAL_DTOR_FN vt_time_tracker_dtor
    #include "verstable.h"
    typedef struct TimerEntry {
        char* location;
        uint64_t start_time;
        struct TimerEntry* next;
    } TimerEntry;
    typedef struct TimerState {
        time_tracker_table table;
        TimerEntry* stack;
    } TimerState;
    static pthread_key_t g_tls_timer_state;
    static void timer_state_free(void *ptr) {
        TimerState *ts = (TimerState*)ptr;
        if (ts) {
            time_tracker_table_cleanup(&ts->table);
            while (ts->stack) {
                TimerEntry *top = ts->stack;
                ts->stack = top->next;
                free(top->location);
                free(top);
            }
            free(ts);
        }
    }
    static TimerState *get_timer_state(void) {
        TimerState *ts = pthread_getspecific(g_tls_timer_state);
        if (!ts) {
            ts = calloc(1, sizeof(TimerState));
            if (!ts) {
                printf("code_monitoring: out of memory for TimerState\n");
                return NULL;
            }
            time_tracker_table_init(&ts->table);
            ts->stack = NULL;
            pthread_setspecific(g_tls_timer_state, ts);
        }
        return ts;
    }
    void _cm_timer_init() {
        cm_init_once();
        TimerState *ts = get_timer_state();
        if (ts) {
            time_tracker_table_cleanup(&ts->table);
            while (ts->stack) {
                TimerEntry *top = ts->stack;
                ts->stack = top->next;
                free(top->location);
                free(top);
            }
        }
    }
    void _cm_timer_start(int line, const char* file) {
        cm_init_once();
        TimerState *ts = get_timer_state();
        if (!ts) return;
        char temp_location[PATH_CAPACITY];
        int n = snprintf(temp_location, sizeof temp_location, "%s:%d", file, line);
        if (n < 0) {
            printf("code_monitoring: internal error. snprintf failed\n");
            return;
        }
        time_tracker_table_itr itr = time_tracker_table_get(&ts->table, temp_location);
        if (time_tracker_table_is_end(itr)) {
            char* key = strdup(temp_location);
            if (!key) {
                printf("code_monitoring: out of memory for strdup\n");
                return;
            }
            TimeTracker tracker = {0};
            tracker.total_time_us = 0;
            tracker.count = 0;
            call_path_time_table_init(&tracker.call_paths);
            itr = time_tracker_table_insert(&ts->table, key, tracker);
            if (time_tracker_table_is_end(itr)) {
                printf("code_monitoring: time_table is out of memory because time_tracker_table_insert failed\n");
                free(key);
                return;
            }
        }
        TimerEntry* entry = malloc(sizeof(TimerEntry));
        if (!entry) {
            printf("code_monitoring: out of memory for TimerEntry\n");
            return;
        }
        entry->location = strdup(temp_location);
        if (!entry->location) {
            printf("code_monitoring: out of memory for strdup\n");
            free(entry);
            return;
        }
        entry->start_time = get_time_us();
        entry->next = ts->stack;
        ts->stack = entry;
    }
    void _cm_timer_stop(int line, const char* file) {
        cm_init_once();
        uint64_t end_time = get_time_us();
        TimerState *ts = get_timer_state();
        if (!ts) return;
        if (!ts->stack) {
            printf("code_monitoring: cm_timer_stop is called without a corresponding cm_timer_start beforehand\n");
            return;
        }
        TimerEntry *top = ts->stack;
        ts->stack = top->next;
        uint64_t delta = end_time - top->start_time;
        char stop_location[PATH_CAPACITY];
        int n = snprintf(stop_location, sizeof stop_location, "%s:%d", file, line);
        if (n < 0) {
            printf("code_monitoring: internal error. snprintf failed\n");
            free(top->location);
            free(top);
            return;
        }
        size_t needed = strlen(top->location) + strlen(stop_location) + 5;
        char* call_path = malloc(needed);
        if (!call_path) {
            printf("code_monitoring: out of memory for call_path\n");
            free(top->location);
            free(top);
            return;
        }
        snprintf(call_path, needed, "%s to %s", top->location, stop_location);
        time_tracker_table_itr itr = time_tracker_table_get(&ts->table, top->location);
        if (time_tracker_table_is_end(itr)) {
            printf("code_monitoring: internal error. no tracker for start location\n");
            free(call_path);
            free(top->location);
            free(top);
            return;
        }
        TimeTracker* tracker = &itr.data->val;
        tracker->total_time_us += delta;
        tracker->count++;
        call_path_time_table_itr cp_itr = call_path_time_table_get(&tracker->call_paths, call_path);
        if (call_path_time_table_is_end(cp_itr)) {
            CallPathTime cpt = {0};
            cpt.total_time_us = delta;
            cpt.count = 1;
            cp_itr = call_path_time_table_insert(&tracker->call_paths, call_path, cpt);
            if (call_path_time_table_is_end(cp_itr)) {
                printf("code_monitoring: call_paths is out of memory because call_path_time_table_insert failed\n");
                free(call_path);
            }
        } else {
            CallPathTime* cpt = &cp_itr.data->val;
            cpt->total_time_us += delta;
            cpt->count++;
            free(call_path);
        }
        free(top->location);
        free(top);
    }
    void _cm_timer_print() {
        TimerState *ts = get_timer_state();
        if (!ts) return;
        uint64_t max_time_ms = 0;
        uint64_t max_calls = 0;
        for (time_tracker_table_itr itr = time_tracker_table_first(&ts->table);
             !time_tracker_table_is_end(itr);
             itr = time_tracker_table_next(itr)) {
            TimeTracker* tracker = &itr.data->val;
            uint64_t time_ms = tracker->total_time_us / 1000;
            if (time_ms > max_time_ms) max_time_ms = time_ms;
            if (tracker->count > max_calls) max_calls = tracker->count;
            for (call_path_time_table_itr cp_itr = call_path_time_table_first(&tracker->call_paths);
                 !call_path_time_table_is_end(cp_itr);
                 cp_itr = call_path_time_table_next(cp_itr)) {
                CallPathTime* cpt = &cp_itr.data->val;
                uint64_t cp_time_ms = cpt->total_time_us / 1000;
                if (cp_time_ms > max_time_ms) max_time_ms = cp_time_ms;
                if (cpt->count > max_calls) max_calls = cpt->count;
            }
        }
        int time_digits = snprintf(NULL, 0, "%" PRIu64, max_time_ms);
        int calls_digits = snprintf(NULL, 0, "%" PRIu64, max_calls);
        for (time_tracker_table_itr itr = time_tracker_table_first(&ts->table);
             !time_tracker_table_is_end(itr);
             itr = time_tracker_table_next(itr)) 
        {
            const char* start_loc = itr.data->key;
            TimeTracker* tracker = &itr.data->val;
            double avg = tracker->count > 0 ? (double)tracker->total_time_us / tracker->count : 0.0;
            printf("%*" PRIu64 "ms | %*" PRIu64 " calls | %.3fms avg | %s\n",
                   time_digits, tracker->total_time_us/1000, calls_digits, tracker->count, avg/1000, start_loc);
            for (call_path_time_table_itr cp_itr = call_path_time_table_first(&tracker->call_paths);
                 !call_path_time_table_is_end(cp_itr);
                 cp_itr = call_path_time_table_next(cp_itr)) {
                const char* path = cp_itr.data->key;
                CallPathTime* cpt = &cp_itr.data->val;
                double cp_avg = cpt->count > 0 ? (double)cpt->total_time_us / cpt->count : 0.0;
                printf("%*" PRIu64 "ms | %*" PRIu64 " calls | %.3fms avg |     %s\n",
                       time_digits, cpt->total_time_us/1000, calls_digits, cpt->count, cp_avg/1000, path);
            }
        }
    }
    void _cm_timer_clear() {
        cm_init_once();
        TimerState *ts = get_timer_state();
        if (ts) {
            time_tracker_table_cleanup(&ts->table);
            while (ts->stack) {
                TimerEntry *top = ts->stack;
                ts->stack = top->next;
                free(top->location);
                free(top);
            }
        }
        free(ts);
        pthread_setspecific(g_tls_timer_state, NULL);
    }
#endif

/* =============================  API impl  ============================== */


static void signal_handler(int sig) {
    (void)sig;
    // Minimal message (async-safe: use write() instead of printf)
    const char *msg = "\nCaught signal. dumping memory before exit:\n";
    write(STDERR_FILENO, msg, strlen(msg));

    #ifdef CM_SHOW_TIMER
        _cm_timer_print();
        _cm_timer_clear();
    #endif

    #ifdef CM_SHOW_MEMORY_PRINT_ON_EXIT
        cm_memory_dump();  // Call dump (note: not fully async-safe, but useful for dev)
    #endif

    // Re-raise signal or exit to avoid infinite loops
    _exit(EXIT_FAILURE);
}

bool cm_output_stdio(const char *msg, void *user)
{
    (void)user;
    printf("%s", msg);
    return true;
}

static void cm_init_once(void)
{
    (void)pathstack_push;
    static pthread_once_t once_control = PTHREAD_ONCE_INIT;
    pthread_once(&once_control, cm_init_once_impl);
}

static void cm_init_once_impl(void)
{
    (void)get_time_us;
    (void)pathstack_push;
    (void)pathstack_pop;
    pthread_key_create(&g_tls_path, pathstack_free);
    g_start_ms = get_time_ms();
    
#ifdef CM_SHOW_MEMORY
    /* Store original memory functions before they get redefined */
    original_malloc = malloc;
    original_calloc = calloc;
    original_realloc = realloc;
    original_strdup = strdup;
    original_free = free;
#endif
    
    signal(SIGSEGV, signal_handler);
    signal(SIGABRT, signal_handler);
    signal(SIGINT, signal_handler);

#ifdef CM_SHOW_MEMORY_PRINT_ON_EXIT
    atexit(cm_memory_dump);
#endif

#ifdef CM_SHOW_TIMER
    pthread_key_create(&g_tls_timer_state, timer_state_free);
    atexit(_cm_timer_clear);
#endif
}

void _cm_print(uint32_t flags, const char* identifyer, int line, const char *file, const char *fmt, ...)
{
    cm_init_once();

    char msgbuf[2048];
    char *p = msgbuf;
    size_t n = 0;

    /* section */
    if (flags & CM_INIT_F_LEVEL) {
        n = snprintf(p, sizeof msgbuf, "%s | ", identifyer);
        p += n;
    }

    /* time */
    if (flags & CM_INIT_F_TIME) {
        uint64_t t_ms = get_time_ms() - g_start_ms;
        n = snprintf(p, sizeof msgbuf - (p - msgbuf), "%" PRIu64 "ms | ", (uint64_t)t_ms);
        p += n;
    }

    /* thread */
    if (flags & CM_INIT_F_THREAD) {
        pthread_t tid = pthread_self();
        n = snprintf(p, sizeof msgbuf - (p - msgbuf), "tid %lld | ", (unsigned long long)tid);
        p += n;
    }

    /* path */
    if (flags & CM_INIT_F_PATH) {
        PathStack *ps = pathstack_get();
        if (ps && ps->buf[0]) {
            /* Show call stack path + current file:line */
            n = snprintf(p, sizeof msgbuf - (p - msgbuf), "%s -> %s:%d | ", ps->buf, file, line);
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
    pthread_mutex_lock(&g_cm_mutex);
    CM_OUTPUT_FN(msgbuf, CM_OUTPUT_USERPTR);
    pthread_mutex_unlock(&g_cm_mutex);
}

/* -------------------------  Scope tracing  ----------------------------- */
#ifdef CM_SHOW_SCOPE
    void _cm_scope_start(int line, const char *file) { cm_init_once(); pathstack_push(file, line); }
    void _cm_scope_end  (void)                       { pathstack_pop(); }
#endif

/* -------------------------  Memory API  -------------------------------- */
#ifdef CM_SHOW_MEMORY
    void *cm_malloc(size_t size, const char *file, int line)
    {
        void *p = original_malloc(size);
        mem_add(p, size, file, line);
        return p;
    }

    void *cm_calloc(size_t nmemb, size_t size, const char *file, int line)
    {
        void *p = original_calloc(nmemb, size);
        mem_add(p, nmemb * size, file, line);
        return p;
    }
    void *cm_realloc(void *ptr, size_t size, const char *file, int line)
    {
        if (size == 0) {
            cm_free(ptr, file, line);
            return NULL;
        }
        if (ptr == NULL) {
            return cm_malloc(size, file, line);
        }
        void *newp = original_realloc(ptr, size);
        if (newp != NULL) {
            mem_update(ptr, newp, size);
        }
        return newp;
    }
    char *cm_strdup(const char *str, const char *file, int line) {
        if (!str) str = "";
        size_t len = strlen(str) + 1;
        char *p = (char *)original_malloc(len);
        if (p) {
            memcpy(p, str, len);
            mem_add(p, len, file, line);
        }
        return p;
    }
    void cm_free(void *ptr, const char *file, int line)
    {
        (void)line;
        (void)file;
        // Add NULL pointer check
        if (!ptr) {
            CM_LOG_ERROR("you tried to free NULL ptr");
            exit(EXIT_FAILURE);
        }
        
        // Check if pointer exists in tracking (detects double-free)
        bool found = mem_remove(ptr);
        if (!found) {
            CM_LOG_ERROR("you tried to free a pointer that was not allocated");
            exit(EXIT_FAILURE);
        }
        
        original_free(ptr);
    }
#endif 
#if defined(CM_SHOW_MEMORY) && defined(CM_SHOW_MEMORY_PRINT_ON_EXIT)
    void cm_memory_dump(void)
    {
        pthread_rwlock_rdlock(&g_mem_rwlock);
        
        // Print header like debug.c does
        char header[] = "\nunfreed memory:\n";
        pthread_mutex_lock(&g_cm_mutex);
        CM_OUTPUT_FN(header, CM_OUTPUT_USERPTR);
        pthread_mutex_unlock(&g_cm_mutex);
        
        // Print each memory entry with time, thread, and full path
        for (MemEntry *e = g_mem_head; e; e = e->next) {
            char linebuf[512]; // Increased buffer size for longer paths
            uint64_t t_ms = e->t_ms - g_start_ms;
            snprintf(linebuf, sizeof linebuf, "\t%" PRIu64 "ms | tid %lu | address %p | %zu bytes | at %s\n",
                                               t_ms, (unsigned long)e->tid, e->ptr, e->size, e->path);
            pthread_mutex_lock(&g_cm_mutex);
            CM_OUTPUT_FN(linebuf, CM_OUTPUT_USERPTR);
            pthread_mutex_unlock(&g_cm_mutex);
        }
        
        // Print trailing newline like debug.c does
        char footer[] = "\n";
        pthread_mutex_lock(&g_cm_mutex);
        CM_OUTPUT_FN(footer, CM_OUTPUT_USERPTR);
        pthread_mutex_unlock(&g_cm_mutex);
        
        pthread_rwlock_unlock(&g_mem_rwlock);
    }
#else /* CM_SHOW_MEMORY AND CM_SHOW_MEMORY_PRINT_ON_EXIT not defined */
    void cm_memory_dump(void)
    {
        (void)mem_add;
        (void)mem_remove;
        (void)mem_update;
        printf("cm_memory_dump: CM_SHOW_MEMORY_PRINT_ON_EXIT must be defined to track and dump memory allocations\n");
    }
#endif /* CM_SHOW_MEMORY */