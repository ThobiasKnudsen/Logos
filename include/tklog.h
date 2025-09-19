#ifndef TKLOG_H_
#define TKLOG_H_

/* -------------------------------------------------------------------------
 *  Dependencies
 * ------------------------------------------------------------------------- */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

/* -------------------------------------------------------------------------
 *  Short file name helper ------------------------------------------------- */
#if defined(__FILE_NAME__)
    #define __TKLOG_FILE_NAME__  __FILE_NAME__
#else
    #define __TKLOG_FILE_NAME__  (strrchr(__FILE__, '/') ? strrchr(__FILE__, '/') + 1 : __FILE__)
#endif

/* -------------------------------------------------------------------------
 *  Output callback (compile‑time selection) ------------------------------ */
typedef bool (*tklog_output_fn_t)(const char *msg, void *user);

#ifndef TKLOG_OUTPUT_FN
    /* Default fallback writes to stderr; provided by log.c */
    bool tklog_output_stdio(const char *msg, void *user);
    #define TKLOG_OUTPUT_FN tklog_output_stdio
#endif

#ifndef TKLOG_OUTPUT_USERPTR
    #define TKLOG_OUTPUT_USERPTR  NULL
#endif

/* ---- Compose default flag word (compile‑time only) --------------------- */
#ifdef TKLOG_SHOW_LOG_LEVEL
    #define TKLOG_INIT_F_LEVEL   1u << 0
#else
    #define TKLOG_INIT_F_LEVEL   0u
#endif

#ifdef TKLOG_SHOW_TIME
    #define TKLOG_INIT_F_TIME    1u << 1
#else
    #define TKLOG_INIT_F_TIME    0u
#endif

#ifdef TKLOG_SHOW_THREAD
    #define TKLOG_INIT_F_THREAD  1u << 2
#else
    #define TKLOG_INIT_F_THREAD  0u
#endif

#ifdef TKLOG_SHOW_PATH
    #define TKLOG_INIT_F_PATH    1u << 3
#else
    #define TKLOG_INIT_F_PATH    0u
#endif

#define TKLOG_DEFAULT_FLAGS  (TKLOG_INIT_F_LEVEL  | \
                            TKLOG_INIT_F_TIME   | \
                            TKLOG_INIT_F_THREAD | \
                            TKLOG_INIT_F_PATH)

/* -------------------------------------------------------------------------
 *  ACTIVE FLAG WORD (pure compile‑time)                                   */
/*  Supply TKLOG_STATIC_FLAGS on the compiler command line or in CMake        */
/*  to override the built‑in defaults, e.g.                                 */
/*      add_compile_definitions(TKLOG_STATIC_FLAGS=(TKLOG_SHOW_LOG_LEVEL|TKLOG_SHOW_TIME))  */
/* ------------------------------------------------------------------------- */
#ifdef TKLOG_STATIC_FLAGS
    #define TKLOG_ACTIVE_FLAGS (TKLOG_STATIC_FLAGS)
#else
    #define TKLOG_ACTIVE_FLAGS (TKLOG_DEFAULT_FLAGS)
#endif

/* -------------------------------------------------------------------------
 *  Log levels
 * ------------------------------------------------------------------------- */
typedef enum {
    TKLOG_LEVEL_DEBUG,
    TKLOG_LEVEL_INFO,
    TKLOG_LEVEL_NOTICE,
    TKLOG_LEVEL_WARNING,
    TKLOG_LEVEL_ERROR,
    TKLOG_LEVEL_CRITICAL,
    TKLOG_LEVEL_ALERT,
    TKLOG_LEVEL_EMERGENCY
} tklog_level_t;

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------
 *  Core logger implementation
 * ------------------------------------------------------------------------- */
void _tklog(uint32_t    flags,
          tklog_level_t level,
          int         line,
          const char *file,
          const char *fmt,
          ...) __attribute__((format(printf, 5, 6)));

/* -------------------------------------------------------------------------
 *  Memory tracking (optional) -------------------------------------------- */
#ifdef TKLOG_MEMORY
    void *tklog_malloc (size_t size, const char *file, int line);
    void *tklog_calloc (size_t nmemb, size_t size, const char *file, int line);
    void *tklog_realloc(void *ptr, size_t size, const char *file, int line);
    char *tklog_strdup(const char *str, const char *file, int line);
    void  tklog_free   (void *ptr, const char *file, int line);

    #define malloc(sz)       tklog_malloc ((sz), __TKLOG_FILE_NAME__, __LINE__)
    #define calloc(n, sz)    tklog_calloc((n), (sz), __TKLOG_FILE_NAME__, __LINE__)
    #define realloc(p, sz)   tklog_realloc((p), (sz), __TKLOG_FILE_NAME__, __LINE__)
    #define strdup(str)      tklog_strdup((str), __TKLOG_FILE_NAME__, __LINE__)
    #define free(p)          tklog_free   ((p), __TKLOG_FILE_NAME__, __LINE__)
#endif /* TKLOG_MEMORY */
    void tklog_memory_dump(void);

/* -------------------------------------------------------------------------
 *  Helper macro: common call site (compile‑time flags only) -------------- */
#define TKLOG_CALL(level, fmt, ...) _tklog(TKLOG_ACTIVE_FLAGS, (level), __LINE__, __TKLOG_FILE_NAME__, fmt, ##__VA_ARGS__)

/* -------------------------------------------------------------------------
 *  Per‑level wrappers (enable/disable via TKLOG_<LEVEL>) ------------------- */
#ifdef TKLOG_DEBUG
    #define tklog_debug(fmt, ...)   TKLOG_CALL(TKLOG_LEVEL_DEBUG, fmt, ##__VA_ARGS__)
#else
    #define tklog_debug(fmt, ...)   ((void)0)
#endif

#ifdef TKLOG_INFO
    #define tklog_info(fmt, ...)    TKLOG_CALL(TKLOG_LEVEL_INFO, fmt, ##__VA_ARGS__)
#else
    #define tklog_info(fmt, ...)    ((void)0)
#endif

#ifdef TKLOG_NOTICE
    #define tklog_notice(fmt, ...)  TKLOG_CALL(TKLOG_LEVEL_NOTICE, fmt, ##__VA_ARGS__)
#else
    #define tklog_notice(fmt, ...)  ((void)0)
#endif

/* -------------------------------------------------------------------------
 *  Exit‑on‑level helpers (TKLOG_EXIT_ON_<LEVEL>) ------------------------------ */
#define TKLOG_EXIT_ON_TEMPLATE(lvl, label, code, fmt, ...)              \
    do {                                                              \
        _tklog(TKLOG_ACTIVE_FLAGS, (lvl), __LINE__, __TKLOG_FILE_NAME__, label " | " fmt, ##__VA_ARGS__); \
        abort();                                                  \
    } while (0)

/* Warning */
#ifdef TKLOG_EXIT_ON_WARNING
    #define tklog_warning(fmt, ...) TKLOG_EXIT_ON_TEMPLATE(TKLOG_LEVEL_WARNING, "TKLOG_EXIT_ON_WARNING", -1, fmt, ##__VA_ARGS__)
#elif defined(TKLOG_WARNING)
    #define tklog_warning(fmt, ...) TKLOG_CALL(TKLOG_LEVEL_WARNING, fmt, ##__VA_ARGS__)
#else
    #define tklog_warning(fmt, ...) ((void)0)
#endif

/* Error */
#ifdef TKLOG_EXIT_ON_ERROR
    #define tklog_error(fmt, ...)   TKLOG_EXIT_ON_TEMPLATE(TKLOG_LEVEL_ERROR, "TKLOG_EXIT_ON_ERROR", -1, fmt, ##__VA_ARGS__)
#elif defined(TKLOG_ERROR)
    #define tklog_error(fmt, ...)   TKLOG_CALL(TKLOG_LEVEL_ERROR, fmt, ##__VA_ARGS__)
#else
    #define tklog_error(fmt, ...)   ((void)0)
#endif

/* Critical */
#ifdef TKLOG_EXIT_ON_CRITICAL
    #define tklog_critical(fmt, ...) TKLOG_EXIT_ON_TEMPLATE(TKLOG_LEVEL_CRITICAL, "TKLOG_EXIT_ON_CRITICAL", -1, fmt, ##__VA_ARGS__)
#elif defined(TKLOG_CRITICAL)
    #define tklog_critical(fmt, ...) TKLOG_CALL(TKLOG_LEVEL_CRITICAL, fmt, ##__VA_ARGS__)
#else
    #define tklog_critical(fmt, ...) ((void)0)
#endif

/* Alert */
#ifdef TKLOG_EXIT_ON_ALERT
    #define tklog_alert(fmt, ...)   TKLOG_EXIT_ON_TEMPLATE(TKLOG_LEVEL_ALERT, "TKLOG_EXIT_ON_ALERT", -1, fmt, ##__VA_ARGS__)
#elif defined(TKLOG_ALERT)
    #define tklog_alert(fmt, ...)   TKLOG_CALL(TKLOG_LEVEL_ALERT, fmt, ##__VA_ARGS__)
#else
    #define tklog_alert(fmt, ...)   ((void)0)
#endif

/* Emergency */
#ifdef TKLOG_EXIT_ON_EMERGENCY
    #define tklog_emergency(fmt, ...) TKLOG_EXIT_ON_TEMPLATE(TKLOG_LEVEL_EMERGENCY, "TKLOG_EXIT_ON_EMERGENCY", -1, fmt, ##__VA_ARGS__)
#elif defined(TKLOG_EMERGENCY)
    #define tklog_emergency(fmt, ...) TKLOG_CALL(TKLOG_LEVEL_EMERGENCY, fmt, ##__VA_ARGS__)
#else
    #define tklog_emergency(fmt, ...) ((void)0)
#endif

/* -------------------------------------------------------------------------
 *  Optional scope tracing ------------------------------------------------- */
#ifdef TKLOG_SCOPE
    void _tklog_scope_start(int line, const char *file);
    void _tklog_scope_end(void);
    #define tklog_scope(code)  _tklog_scope_start(__LINE__, __TKLOG_FILE_NAME__); code; _tklog_scope_end()
#else
    #define tklog_scope(code)  code
#endif /* TKLOG_SCOPE */

#ifdef TKLOG_TIMER
    void _tklog_timer_init(void);
    void _tklog_timer_start(int line, const char* file);
    void _tklog_timer_stop(int line, const char* file);
    void _tklog_timer_print();
    void _tklog_timer_clear();
    #define tklog_timer_init() _tklog_timer_init()
    #define tklog_timer_start() _tklog_timer_start(__LINE__, __TKLOG_FILE_NAME__)
    #define tklog_timer_stop() _tklog_timer_stop(__LINE__, __TKLOG_FILE_NAME__)
    #define tklog_timer_print() _tklog_timer_print()
    #define tklog_timer_clear() _tklog_timer_clear()
#else
    #define tklog_timer_init() 
    #define tklog_timer_start() 
    #define tklog_timer_stop() 
    #define tklog_timer_print() 
    #define tklog_timer_clear()
#endif // TKLOG_TIMER

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* TKLOG_H_ */