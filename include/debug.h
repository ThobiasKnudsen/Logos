#ifndef CPI_DEBUG_H
#define CPI_DEBUG_H

#include <stddef.h>  // For size_t
#include <stdio.h>   // For printf and snprintf
#include <string.h>

#define __FILENAME__ (strrchr(__FILE__, '/') ? strrchr(__FILE__, '/') + 1 : __FILE__)


// Function declarations
void debug_Printf(const char* message, size_t line, const char* file);
void* debug_Malloc(size_t size, size_t line, const char* file);
void* debug_Realloc(void* ptr, size_t size, size_t line, char* file);
void debug_Free(void* ptr, size_t line, const char* file);
void debug_StartScope(size_t line, const char* file);
void debug_EndScope();
void debug_PrintMemory();

#ifdef DEBUG

#include <stdlib.h>

#define malloc(size)       debug_Malloc (size, __LINE__, __FILENAME__)
#define realloc(ptr, size) debug_Realloc(ptr, size, __LINE__, __FILENAME__)
#define free(ptr)          debug_Free   (ptr, __LINE__, __FILENAME__)
#define printf(fmt, ...)   do {char buffer[2048]; snprintf(buffer, sizeof(buffer), fmt, ##__VA_ARGS__); debug_Printf(buffer, __LINE__, __FILENAME__); } while (0)

#define TRACK(code)             debug_StartScope(__LINE__, __FILENAME__); code; debug_EndScope();
#define DEBUG_SCOPE(code)       debug_StartScope(__LINE__, __FILENAME__); code; debug_EndScope();
#define DEBUG_ASSERT(expr, fmt, ...) \
    do { \
        if (!(expr)) { \
            char buffer_1[512]; \
            char buffer_2[1024]; \
            snprintf(buffer_1, sizeof(buffer_1), "ASSERT exiting | expr is false | %s\n", #expr); \
            debug_Printf(buffer_1, __LINE__, __FILENAME__); \
            snprintf(buffer_1, sizeof(buffer_1), fmt, ##__VA_ARGS__); \
            snprintf(buffer_2, sizeof(buffer_2), "ASSERT exiting | exit message  | %s", buffer_1); \
            debug_Printf(buffer_2, __LINE__, __FILENAME__); \
            exit(-1); \
        } \
    } while (0);
#define ASSERT(expr, fmt, ...) \
    do { \
        if (!(expr)) { \
            char buffer_1[512]; \
            char buffer_2[1024]; \
            snprintf(buffer_1, sizeof(buffer_1), "ASSERT exiting | expr is false | %s\n", #expr); \
            debug_Printf(buffer_1, __LINE__, __FILENAME__); \
            snprintf(buffer_1, sizeof(buffer_1), fmt, ##__VA_ARGS__); \
            snprintf(buffer_2, sizeof(buffer_2), "ASSERT exiting | exit message  | %s", buffer_1); \
            debug_Printf(buffer_2, __LINE__, __FILENAME__); \
            exit(-1); \
        } \
    } while (0);

#else
    
#define TRACK(code) code 
#define DEBUG_SCOPE(code) code 
#define DEBUG_ASSERT(expr, fmt, ...)  
#define ASSERT(expr, fmt, ...) \
    do { \
        if (!(expr)) { \
            char buffer_1[512]; \
            char buffer_2[1024]; \
            snprintf(buffer_1, sizeof(buffer_1), "ASSERT exiting | expr is false | %s\n", #expr); \
            printf(buffer_1, __LINE__, __FILENAME__); \
            snprintf(buffer_1, sizeof(buffer_1), fmt, ##__VA_ARGS__); \
            snprintf(buffer_2, sizeof(buffer_2), "ASSERT exiting | exit message  | %s", buffer_1); \
            printf(buffer_2, __LINE__, __FILENAME__); \
            exit(-1); \
        } \
    } while (0);

#endif

void* alloc(void* ptr, size_t size);

#endif // CPI_DEBUG_H