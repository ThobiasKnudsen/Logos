/*  tklog.c – implementation for the tklogging facility declared in tklog.h
 *
 *  Compile‑time‑only configuration: the header decides everything through
 *  macros.  This implementation simply honours TKLOG_ACTIVE_FLAGS, the selected
 *  TKLOG_OUTPUT_FN, and TKLOG_OUTPUT_USERPTR.
 *
 *  Requires SDL 3.2.0+ (mutexes, rwlocks, TLS, timing).
 */

#include <stdlib.h>
#include "tklog.h"
#include <SDL3/SDL.h>
#include <stdio.h>
#include <string.h>
#include <stdarg.h>

/* -------------------------------------------------------------------------
 *  Internal helpers / globals
 * ------------------------------------------------------------------------- */
static SDL_Mutex *g_tklog_mutex   = NULL;   /* serialises _tklog() and memory db */
static SDL_RWLock *g_mem_rwlock = NULL;   /* guards the allocation map       */
static uint64_t    g_start_ms   = 0;      /* program start in µs             */

/* ==============================  PATH TLS  ============================== */
typedef struct PathStack {
    char   *buf;        /* formatted "a.c:12 → b.c:88 → …" string */
    size_t  len;        /* current length                        */
    size_t  cap;        /* allocated capacity                    */
    int     depth;      /* number of entries                     */
} PathStack;

static SDL_TLSID g_tls_path = {0};  /* auto‑initialized by SDL */

static void pathstack_free(void *ptr)
{
    PathStack *ps = (PathStack*)ptr;
    if (ps) {
        SDL_free(ps->buf);
        SDL_free(ps);
    }
}

static PathStack *pathstack_get(void)
{
    PathStack *ps = SDL_GetTLS(&g_tls_path);
    if (!ps) {
        ps = (PathStack*)SDL_calloc(1, sizeof *ps);
        if (!ps) return NULL; /* out‑of‑mem: just skip tracing */
        ps->cap = 256;
        ps->buf = (char*)SDL_malloc(ps->cap);
        if (!ps->buf) { SDL_free(ps); return NULL; }
        ps->buf[0] = '\0';
        SDL_SetTLS(&g_tls_path, ps, pathstack_free);
    }
    return ps;
}

static void pathstack_push(const char *file, int line)
{
    PathStack *ps = pathstack_get();
    if (!ps) return;

    char tmp[128];
    int  n = SDL_snprintf(tmp, sizeof tmp, "%s:%d", file, line);
    if (n < 0) return;

    /* ensure space: existing + sep + n + NUL */
    size_t need = ps->len + (ps->depth ? 3 : 0) + n + 1;
    if (need > ps->cap) {
        size_t newcap = ps->cap * 2;
        while (newcap < need) newcap *= 2;
        char *newbuf = (char*)SDL_realloc(ps->buf, newcap);
        if (!newbuf) return; /* OOM */
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
}

static void pathstack_pop(void)
{
    PathStack *ps = SDL_GetTLS(&g_tls_path);
    if (!ps || ps->depth == 0) return;

    /* remove last element */
    char *last_sep = NULL;
    if (ps->depth > 1) {
        /* find the penultimate " → " */
        last_sep = (char*)SDL_strrchr(ps->buf, '\x20'); /* space before arrow */
        if (last_sep) {
            /* we had space UTF‑8 arrow space; step before that */
            *last_sep = '\0';
            /* now find prev arrow; we need the last arrow occurrence */
            last_sep = SDL_strrchr(ps->buf, '\xE2'); /* begin arrow UTF‑8 */
            if (last_sep) last_sep -= 1; /* step to space before arrow */
        }
    }
    if (!last_sep) {
        /* first element */
        ps->buf[0] = '\0';
        ps->len   = 0;
    } else {
        *last_sep = '\0';
        ps->len = SDL_strlen(ps->buf);
    }
    ps->depth -= 1;
}

/* =============================  MEM TRACK  ============================== */
typedef struct MemEntry {
    void           *ptr;
    size_t          size;
    uint64_t        t_ms;
    SDL_ThreadID    tid;
    char            path[128];
    struct MemEntry *next;
} MemEntry;

static MemEntry *g_mem_head = NULL;

static void mem_add(void *ptr, size_t size, const char *file, int line)
{
    if (!ptr) return;
    SDL_LockRWLockForWriting(g_mem_rwlock);
    MemEntry *e = (MemEntry*)SDL_malloc(sizeof *e);
    if (e) {
        e->ptr  = ptr;
        e->size = size;
        e->t_ms = SDL_GetTicks();
        e->tid  = SDL_GetCurrentThreadID();
        SDL_snprintf(e->path, sizeof e->path, "%s:%d", file, line);
        e->next = g_mem_head;
        g_mem_head = e;
    }
    SDL_UnlockRWLock(g_mem_rwlock);
}

static void mem_update(void *oldptr, void *newptr, size_t newsize)
{
    SDL_LockRWLockForWriting(g_mem_rwlock);
    for (MemEntry *e = g_mem_head; e; e = e->next) {
        if (e->ptr == oldptr) {
            e->ptr  = newptr;
            e->size = newsize;
            break;
        }
    }
    SDL_UnlockRWLock(g_mem_rwlock);
}

static void mem_remove(void *ptr)
{
    SDL_LockRWLockForWriting(g_mem_rwlock);
    MemEntry **pp = &g_mem_head;
    while (*pp) {
        if ((*pp)->ptr == ptr) {
            MemEntry *dead = *pp;
            *pp = dead->next;
            SDL_free(dead);
            break;
        }
        pp = &(*pp)->next;
    }
    SDL_UnlockRWLock(g_mem_rwlock);
}

/* =============================  API impl  ============================== */

bool tklog_output_stdio(const char *msg, void *user)
{
    (void)user;
    printf(msg);
    return true;
}

static void tklog_init_once(void)
{
    if (g_tklog_mutex) return; /* already done */
    g_tklog_mutex   = SDL_CreateMutex();
    g_mem_rwlock  = SDL_CreateRWLock();
    g_start_ms    = SDL_GetTicks();
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
    if (flags & TKLOG_SHOW_LOG_LEVEL) {
        n = SDL_snprintf(p, sizeof msgbuf, "%s | ", levelstr[level]);
        p += n;
    }

    /* time */
    if (flags & TKLOG_SHOW_TIME) {
        uint64_t t_ms = SDL_GetTicks() - g_start_ms;
        n = SDL_snprintf(p, sizeof msgbuf - (p - msgbuf), "%" PRIu64 "ms | ", (uint64_t)t_ms);
        p += n;
    }

    /* thread */
    if (flags & TKLOG_SHOW_THREAD) {
        SDL_ThreadID tid = SDL_GetCurrentThreadID();
        n = SDL_snprintf(p, sizeof msgbuf - (p - msgbuf), "Thread %" PRIu64 " | ", (uint64_t)tid);
        p += n;
    }

    /* path */
    if (flags & TKLOG_SHOW_PATH) {
        PathStack *ps = pathstack_get();
        if (ps && ps->buf[0]) {
            n = SDL_snprintf(p, sizeof msgbuf - (p - msgbuf), "%s → %s:%d | ", ps->buf, file, line);
            p += n;
        }
    }

    /* user message */
    va_list ap;
    va_start(ap, fmt);
    SDL_vsnprintf(p, sizeof msgbuf - (p - msgbuf), fmt, ap);
    va_end(ap);

    /* ensure newline */
    size_t len = SDL_strlen(msgbuf);
    if (len + 1 < sizeof msgbuf && msgbuf[len - 1] != '\n') {
        msgbuf[len]   = '\n';
        msgbuf[len+1] = '\0';
    }

    /* lock, write, unlock */
    SDL_LockMutex(g_tklog_mutex);
    TKLOG_OUTPUT_FN(msgbuf, TKLOG_OUTPUT_USERPTR);
    SDL_UnlockMutex(g_tklog_mutex);
}

/* -------------------------  Scope tracing  ----------------------------- */
#ifdef TKLOG_SCOPE
    void _tklog_scope_start(int line, const char *file) { pathstack_push(file, line); }
    void _tklog_scope_end  (void)                       { pathstack_pop();            }
#endif

/* -------------------------  Memory API  -------------------------------- */
#ifdef TKLOG_MEMORY
    void *tklog_malloc(size_t size, const char *file, int line)
    {
        void *p = SDL_malloc(size);
        mem_add(p, size, file, line);
        return p;
    }

    void *tklog_realloc(void *ptr, size_t size, const char *file, int line)
    {
        void *newp = SDL_realloc(ptr, size);
        mem_update(ptr, newp, size);
        return newp;
    }

    void tklog_free(void *ptr, const char *file, int line)
    {
        (void)file; (void)line;
        mem_remove(ptr);
        SDL_free(ptr);
    }

    void tklog_memory_dump(void)
    {
        SDL_LockRWLockForReading(g_mem_rwlock);
        for (MemEntry *e = g_mem_head; e; e = e->next) {
            char linebuf[256];
            SDL_snprintf(linebuf, sizeof linebuf, "%p | %zu | %" PRIu64 "ms | %" PRIu64 " | %s\n",
                          e->ptr, e->size, e->t_ms, (uint64_t)e->tid, e->path);
            SDL_LockMutex(g_tklog_mutex);
            TKLOG_OUTPUT_FN(linebuf, TKLOG_OUTPUT_USERPTR);
            SDL_UnlockMutex(g_tklog_mutex);
        }
        SDL_UnlockRWLock(g_mem_rwlock);
    }
#endif /* TKLOG_MEMORY */