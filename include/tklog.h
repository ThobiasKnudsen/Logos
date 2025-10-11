#ifndef TKLOG_H_
#define TKLOG_H_

/**
 * tklog will be replaced by code_monitoring
 */

/* -------------------------------------------------------------------------
 *  Dependencies
 * ------------------------------------------------------------------------- */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

typedef enum {
    CM_RES_SUCCESS,                         // General success (e.g., operation completed without issues)

    CM_RES_NULL_ARGUMENT,                   // NULL pointer for required argument (common, e.g., p_base, p_tsm_base, rcu_head)
    CM_RES_NULL_FUNCTION_POINTER,           // Required function pointer NULL (e.g., fn_free_callback, fn_is_valid)
    CM_RES_NULL_ITER_NODE,                  // Iterator node is NULL (warning in some cases)

    CM_RES_TSM_KEY_IS_VALID,                    // key is valid
    CM_RES_TSM_KEY_NOT_VALID,                   // Invalid key (number=0, string empty/NULL/too long >63 chars)
    CM_RES_TSM_KEY_STRING_EMPTY,                // empty string
    CM_RES_TSM_KEY_STRING_TOO_LARGE,            // key string too large
    CM_RES_TSM_KEY_STRING_COPY_FAILURE,         // String key copy failed (strncpy)
    CM_RES_TSM_KEY_STRING_IS_NULL,              // key string should not be NULL
    CM_RES_TSM_KEY_UINT64_IS_ZERO,              // key uint64 should not be 0
    CM_RES_TSM_KEYS_MATCH,                      // keys match
    CM_RES_TSM_KEYS_DONT_MATCH,                 // keys dont match

    CM_RES_TSM_NODE_IS_VALID,                   // node is valid. used in *is_valid functions
    CM_RES_TSM_NODE_NOT_VALID,                  // Node invalid per type validation (fn_is_valid false or base checks)
    CM_RES_TSM_NODE_IS_TSM,                     // node is a TSM node
    CM_RES_TSM_NODE_NOT_TSM,                    // Given node is not a TSM (tsm_node_is_tsm false)
    CM_RES_TSM_NODE_IS_TYPE,                    // node is a type 
    CM_RES_TSM_NODE_NOT_TYPE,                   // Given node is not a type (tsm_node_is_type false, rare log)
    CM_RES_TSM_NODE_NOT_FOUND,                  // Node/type not found (e.g., tsm_node_get returns NULL)
    CM_RES_TSM_NODE_EXISTS,                     // Node already exists (cds_lfht_add_unique fails)
    CM_RES_TSM_NODE_SIZE_MISMATCH,              // Size mismatch (this_size_bytes != type_size_bytes or < min size)
    CM_RES_TSM_NODE_SIZE_TO_SMALL,              // Size is to small
    CM_RES_TSM_NODE_REPLACING_SAME,             // Replacing node with itself in update
    CM_RES_TSM_NODE_IS_REMOVED,                 // Node is removed/deleted
    CM_RES_TSM_NODE_NOT_REMOVED,                // Not removed from TSM even though it should be
    CM_RES_TSM_NODE_CREATION_FAILURE,           // failed to create a node
    CM_RES_TSM_NODE_INSERTION_FAILURE,          // failed to insert node
    CM_RES_TSM_NODE_REPLACEMENT_FAILURE,        // failed to replace node
    CM_RES_TSM_NODE_NOT_FOUND_SELF,             // using keys to a node to get itself and doesnt get itself

    CM_RES_TSM_ITER_IS_NULL,                    // iter becomes NULL after cds_lfht_next
    CM_RES_TSM_ITER_END,                        // iter is ended

    CM_RES_TSM_CYCLICAL_TYPES,              // Cyclical types in TSM
    CM_RES_TSM_NOT_EMPTY,                   // TSM has nodes before freeing
    CM_RES_TSM_TOO_MANY_TYPES,              // should never actually receive this error but there is a static limit which can be easily increased
    CM_RES_TSM_NON_TYPES_STILL_REMAINING,   // in removing all children after all non-types should have been removed there was registered a non-type

    CM_RES_TSM_PATH_VALID,                      // path is valid
    CM_RES_TSM_PATH_INVALID,                    // Invalid path
    CM_RES_TSM_PATH_NOTHING_TO_REMOVE,          // Remove from empty path
    CM_RES_TSM_PATH_INCONSISTENT,               // Path length inconsistent with pointer (length==0 XOR pointer==NULL)
    CM_RES_TSM_PATH_INSERT_KEY_FAILURE,         // Failed to insert key into path
    CM_RES_TSM_PATH_INTERMEDIARY_NODE_NOT_TSM,  // when getting a node through a path a node on the way is not a TSM

    CM_RES_TSM_TYPE_NOT_FOUND,                  // type node found
    CM_RES_TSM_TYPE_STILL_USED,                 // Trying to free type used in other nodes (DEBUG only)
    CM_RES_TSM_TYPE_MISMATCH,                   // Type key mismatch (e.g., old vs new in update)

    CM_RES_GTSM_ALREADY_INITIALIZED,        // GTSM already initialized
    CM_RES_GTSM_NOT_INITIALIZED,            // GTSM not initialized

    CM_RES_ALLOCATION_FAILURE,              // Memory allocation failed (calloc, malloc, realloc)
    CM_RES_CMPXCHG_FAILURE,                 // rcu_cmpxchg_pointer failed
    CM_RES_PRINT_FAILURE,                   // Print operation failed (e.g., base_node_print)
    CM_RES_OUTSIDE_BOUNDS,                  // Out of bounds
    CM_RES_BUFFER_OVERFLOW,                 // Buffer overflow
    CM_RES_CDS_LFHT_NEW_FAILURE,            // cds_lfht_new failed
    CM_RES_OS_NOT_SUPPORTER,                // OS not supported

    CM_RES_SDL3_CORE_INITIALIZED,
    CM_RES_SDL3_CORE_NOT_INITIALIZED,
    CM_RES_SDL3_GPU_DEVICE_INITIALIZED,
    CM_RES_SDL3_GPU_DEVICE_NOT_INITIALIZED,

    CM_RES_UNKNOWN                          // Generic/uncaught failure
} CM_RES;

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

#define TKLOG_FLAGS (TKLOG_INIT_F_LEVEL  | \
                    TKLOG_INIT_F_TIME    | \
                    TKLOG_INIT_F_THREAD  | \
                    TKLOG_INIT_F_PATH)

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
#define TKLOG_CALL(level, fmt, ...) _tklog(TKLOG_FLAGS, (level), __LINE__, __TKLOG_FILE_NAME__, fmt, ##__VA_ARGS__)

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
        _tklog(TKLOG_FLAGS, (lvl), __LINE__, __TKLOG_FILE_NAME__, label " | " fmt, ##__VA_ARGS__); \
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
    #define tklog_timer_start() tklog_scope(_tklog_timer_start(__LINE__, __TKLOG_FILE_NAME__))
    #define tklog_timer_stop() tklog_scope(_tklog_timer_stop(__LINE__, __TKLOG_FILE_NAME__))
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