#ifndef CODE_MONITORING_H
#define CODE_MONITORING_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

/**
 * cm stands for "code monitoring" and is a powerful tool for logging, timing, and project wide result codes
 */	

typedef enum {
    CM_RES_SUCCESS,                         // General success (e.g., operation completed without issues)

    CM_RES_NULL_ARGUMENT,                   // NULL pointer for required argument (common, e.g., p_base, p_tsm_base, rcu_head)
    CM_RES_NULL_FIELDS, 
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

    // 31
    CM_RES_BENIGN_RACE_NOT_FOUND,
    CM_RES_BENIGN_RACE_EXISTS,
    CM_RES_BENIGN_RACE_NOT_REMOVED,
    CM_RES_BENIGN_RACE_REMOVED,

    CM_RES_TSM_ITER_IS_NULL,                    // iter becomes NULL after cds_lfht_next
    CM_RES_TSM_ITER_END,                        // iter is ended

    CM_RES_TSM_CYCLICAL_TYPES,              // Cyclical types in TSM
    CM_RES_TSM_NOT_EMPTY,                   // TSM has nodes before freeing
    CM_RES_TSM_TOO_MANY_TYPES,              // should never actually receive this error but there is a static limit which can be easily increased
    CM_RES_TSM_NON_TYPES_STILL_REMAINING,   // in removing all children after all non-types should have been removed there was registered a non-type

    // 41
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
    CM_RES_SDL3_UNKOWN_SHADER_KIND,
    CM_RES_SDL3_TOO_MANY_VERTEX_BUFFERS,

    CM_RES_HTRIE_INVALID_KEY,
    CM_RES_HTRIE_NODE_NOT_FOUND,
    CM_RES_HTRIE_NODE_FOUND,
    CM_RES_HTRIE_INTERNAL_ERROR,

    CM_RES_UNKNOWN                          // Generic/uncaught failure
} CM_RES;

/* -------------------------------------------------------------------------
 *  Short file name helper ------------------------------------------------- */
#if defined(__FILE_NAME__)
    #define __CM_FILE_NAME__  __FILE_NAME__
#else
    #define __CM_FILE_NAME__  (strrchr(__FILE__, '/') ? strrchr(__FILE__, '/') + 1 : __FILE__)
#endif

/* -------------------------------------------------------------------------
 *  Output callback (compile‑time selection) ------------------------------ */
typedef bool (*cm_output_fn_t)(const char *msg, void *user);

#ifndef CM_OUTPUT_FN
    /* Default fallback writes to stderr; provided by log.c */
    bool cm_output_stdio(const char *msg, void *user);
    #define CM_OUTPUT_FN cm_output_stdio
#endif

#ifndef CM_OUTPUT_USERPTR
    #define CM_OUTPUT_USERPTR  NULL
#endif

/* ---- Compose default flag word (compile‑time only) --------------------- */
#ifdef CM_SHOW_LOG_LEVEL
    #define CM_INIT_F_LEVEL   1u << 0
#else
    #define CM_INIT_F_LEVEL   0u
#endif

#ifdef CM_SHOW_TIME
    #define CM_INIT_F_TIME    1u << 1
#else
    #define CM_INIT_F_TIME    0u
#endif

#ifdef CM_SHOW_THREAD
    #define CM_INIT_F_THREAD  1u << 2
#else
    #define CM_INIT_F_THREAD  0u
#endif

#ifdef CM_SHOW_PATH
    #define CM_INIT_F_PATH    1u << 3
#else
    #define CM_INIT_F_PATH    0u
#endif

#define CM_FLAGS (CM_INIT_F_LEVEL  | \
                    CM_INIT_F_TIME    | \
                    CM_INIT_F_THREAD  | \
                    CM_INIT_F_PATH)


#ifdef __cplusplus
extern "C" {
#endif

/**
 * Code Section logging and printing
 */
void _cm_print( uint32_t    flags,
                const char *identifyer,
                int         line,
                const char *file,
                const char *fmt,
                ...) __attribute__((format(printf, 5, 6)));

#define CM_PRINT(identifyer, fmt, ...)          _cm_print(CM_FLAGS, identifyer, __LINE__, __CM_FILE_NAME__, fmt, ##__VA_ARGS__)

#define CM_LOG_DEBUG(fmt, ...)      //CM_PRINT("DEBUG    ", fmt, ##__VA_ARGS__)
#define CM_LOG_INFO(fmt, ...)       //CM_PRINT("INFO     ", fmt, ##__VA_ARGS__)
#define CM_LOG_NOTICE(fmt, ...)     CM_PRINT("NOTICE   ", fmt, ##__VA_ARGS__)
#define CM_LOG_WARNING(fmt, ...)    CM_PRINT("WARNING  ", fmt, ##__VA_ARGS__)
#define CM_LOG_ERROR(fmt, ...) do { CM_PRINT("ERROR    ", fmt, ##__VA_ARGS__); abort(); } while(0);
#define CM_LOG_TSM_PRINT(fmt, ...)  //CM_PRINT("TSM PRINT", fmt, ##__VA_ARGS__)

/**
 * Memory tracking
 */
#ifdef CM_SHOW_MEMORY
    void *cm_malloc (size_t size, const char *file, int line);
    void *cm_calloc (size_t nmemb, size_t size, const char *file, int line);
    void *cm_realloc(void *ptr, size_t size, const char *file, int line);
    char *cm_strdup(const char *str, const char *file, int line);
    void  cm_free   (void *ptr, const char *file, int line);

    #define malloc(sz)       cm_malloc ((sz), __CM_FILE_NAME__, __LINE__)
    #define calloc(n, sz)    cm_calloc((n), (sz), __CM_FILE_NAME__, __LINE__)
    #define realloc(p, sz)   cm_realloc((p), (sz), __CM_FILE_NAME__, __LINE__)
    #define strdup(str)      cm_strdup((str), __CM_FILE_NAME__, __LINE__)
    #define free(p)          cm_free   ((p), __CM_FILE_NAME__, __LINE__)
#endif /* CM_MEMORY */
    void cm_memory_dump(void);

/**
 * Scope tracking
 */
#ifdef CM_SHOW_SCOPE
    void _cm_scope_start(int line, const char *file);
    void _cm_scope_end(void);
    #define CM_SCOPE(code)  _cm_scope_start(__LINE__, __CM_FILE_NAME__); code; _cm_scope_end()
#else
    #define CM_SCOPE(code)  code
#endif /* CM_SCOPE */

/**
 * Timer
 */
#ifdef CM_SHOW_TIMER
    void _cm_timer_init(void);
    void _cm_timer_start(int line, const char* file);
    void _cm_timer_stop(int line, const char* file);
    void _cm_timer_print();
    void _cm_timer_clear();
    #define CM_TIMER_INIT() _cm_timer_init()
    #define CM_TIMER_START() CM_SCOPE(_cm_timer_start(__LINE__, __CM_FILE_NAME__))
    #define CM_TIMER_STOP() CM_SCOPE(_cm_timer_stop(__LINE__, __CM_FILE_NAME__))
    #define CM_TIMER_PRINT() _cm_timer_print()
    #define CM_TIMER_CLEAR() _cm_timer_clear()
#else
    #define CM_TIMER_INIT() 
    #define CM_TIMER_START() 
    #define CM_TIMER_STOP() 
    #define CM_TIMER_PRINT() 
    #define CM_TIMER_CLEAR() 
#endif // CM_TIMER

/**
 * If this function gets a false value then it will log an error. Your definition of the CM_LOG_ERROR should abort
 */
#define CM_ASSERT(bool_expression) do { \
    CM_SCOPE(bool bool_1703961674309579167853 = bool_expression); \
    if (!bool_1703961674309579167853) { \
        CM_LOG_ERROR("expression is false: '%s'\n", #bool_expression); \
    } \
} while(0);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif // CODE_MONITORING_H