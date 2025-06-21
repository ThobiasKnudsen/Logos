#ifndef CPI_DEBUG_H
#define CPI_DEBUG_H

#include <stddef.h>  // For size_t
#include <stdio.h>   // For printf and snprintf
#include <string.h>

#define __FILENAME__ (strrchr(__FILE__, '/') ? strrchr(__FILE__, '/') + 1 : __FILE__)


// Function declarations
void    debug_Printf(size_t line, const char* file, const char* fmt, ...);
void*   debug_Malloc(size_t size, size_t line, const char* file);
void*   debug_Realloc(void* ptr, size_t size, size_t line, char* file);
void    debug_Free(void* ptr, size_t line, const char* file);
void    debug_StartScope(size_t line, const char* file);
void    debug_EndScope(void);
void    debug_PrintMemory(void);

#ifdef DEBUG

#include <stdlib.h>

#define malloc(size)       debug_Malloc (size, __LINE__, __FILENAME__)
#define realloc(ptr, size) debug_Realloc(ptr, size, __LINE__, __FILENAME__)
#define free(ptr)          debug_Free   (ptr, __LINE__, __FILENAME__)
#define printf(fmt, ...)   debug_Printf(__LINE__, __FILENAME__, fmt, ##__VA_ARGS__)

#define TRACK(code)             debug_StartScope(__LINE__, __FILENAME__); code; debug_EndScope();
#define DEBUG_SCOPE(code)       debug_StartScope(__LINE__, __FILENAME__); code; debug_EndScope();
#define DEBUG_ASSERT(expr, fmt, ...) \
    do { \
        if (!(expr)) { \
            debug_Printf(__LINE__, __FILENAME__, "ASSERT exiting | expr is false | %s\n", #expr); \
            debug_Printf(__LINE__, __FILENAME__, "ASSERT exiting | exit message  | " fmt, ##__VA_ARGS__); \
            exit(-1); \
        } \
    } while (0)
    
#else
    
#define TRACK(code) code 
#define DEBUG_SCOPE(code) code 
#define DEBUG_ASSERT(expr, fmt, ...)  

#endif

#define ASSERT(expr, fmt, ...) \
    do { \
        if (!(expr)) { \
            debug_Printf(__LINE__, __FILENAME__, "ASSERT exiting | expr is false | %s\n", #expr); \
            debug_Printf(__LINE__, __FILENAME__, "ASSERT exiting | exit message  | " fmt, ##__VA_ARGS__); \
            exit(-1); \
        } \
    } while (0)


void* alloc(void* ptr, size_t size);

#endif // CPI_DEBUG_H