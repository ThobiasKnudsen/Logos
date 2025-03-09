#include "vec_path.h"
#include "debug.h"
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <stdarg.h>

char* _vec_Path_Combine(const char* path_1, const char* path_2) {
    DEBUG_ASSERT(path_1, "NULL pointer");
    DEBUG_ASSERT(path_2, "NULL pointer");

    size_t len1 = strlen(path_1);
    size_t len2 = strlen(path_2);
    /* Allocate enough space: +2 for a possible '/' and the null terminator */
    DEBUG_SCOPE(char* combined = malloc(len1 + len2 + 2));
    DEBUG_ASSERT(combined, "malloc failed");

    /* Copy the first path */
    strcpy(combined, path_1);

    /* If needed, insert a '/' between path_1 and path_2 */
    if (len1 && path_1[len1 - 1] != '/' && len2 && path_2[0] != '/')
        strcat(combined, "/");
    strcat(combined, path_2);

    /* Now simplify the combined path.
       We split on '/' and use a “stack” to hold tokens.
       Every non-empty token (aside from a possible leading empty token for an absolute path)
       is assumed to be either a numeric string or the literal "..".
       When we see a ".." token, if the previous token (if any) is numeric (i.e. not ".."),
       we remove it. */

    int n = (int)strlen(combined);
    /* Worst-case: every character is a token so we need n+1 pointers */
    DEBUG_SCOPE(char **stack = malloc(sizeof(char*) * (n + 1)));
    DEBUG_ASSERT(stack, "malloc failed");
    int sp = 0;  // stack pointer
    int start = 0;
    for (int i = 0; i <= n; i++) {
        if (i == n || combined[i] == '/') {
            combined[i] = '\0';  // terminate token
            char* token = combined + start;
            /* Always push the first token—even if empty—to handle absolute paths */
            if (sp == 0) {
                stack[sp++] = token;
            } else if (token[0] != '\0') {
                if (strcmp(token, "..") == 0) {
                    char* top = stack[sp - 1];
                    /* If top isn’t empty and isn’t ".." then it must be a numeric token */
                    if (top[0] != '\0' && strcmp(top, "..") != 0) {
                        sp--;  // pop the numeric token (cancel it with "..")
                    } else {
                        stack[sp++] = token;
                    }
                } else {
                    /* Otherwise, token is numeric */
                    stack[sp++] = token;
                }
            }
            start = i + 1;
        }
    }

    /* Now build the result string.
       If the very first token is empty then the path is absolute (it must start with '/'),
       otherwise it’s relative. */
    size_t resLen = 0;
    if (sp == 0) {
        resLen = 1;
    } else if (stack[0][0] == '\0') {
        /* Absolute path: reserve one for the leading '/' */
        resLen++;
        for (int j = 1; j < sp; j++) {
            if (j > 1)
                resLen++;  // for '/' between tokens
            resLen += strlen(stack[j]);
        }
    } else {
        for (int j = 0; j < sp; j++) {
            if (j > 0)
                resLen++;  // for '/'
            resLen += strlen(stack[j]);
        }
    }
    
    DEBUG_SCOPE(char* result = malloc(resLen + 1));
    DEBUG_ASSERT(result, "malloc failed");
    
    /* Build the result string in one pass */
    char* p = result;
    if (sp > 0 && stack[0][0] == '\0') {
        /* Absolute path: write the leading '/' */
        *p++ = '/';
        for (int j = 1; j < sp; j++) {
            if (j > 1)
                *p++ = '/';
            size_t l = strlen(stack[j]);
            memcpy(p, stack[j], l);
            p += l;
        }
    } else if (sp > 0) {
        /* Relative path */
        for (int j = 0; j < sp; j++) {
            if (j > 0)
                *p++ = '/';
            size_t l = strlen(stack[j]);
            memcpy(p, stack[j], l);
            p += l;
        }
    } else {
        *p++ = '.';
    }
    *p = '\0';

    free(stack);
    free(combined);
    return result;
}
int* vec_Path_ToIndices(const char* path, size_t* const out_indices_count) {
    DEBUG_ASSERT(path, "NULL pointer");
    if (!path || !out_indices_count) {
        return NULL; // Handle invalid inputs
    }

    size_t path_length = strlen(path);
    if (path_length == 0) {
        *out_indices_count = 0;
        DEBUG_SCOPE(int* indices = alloc(NULL, sizeof(int)));
        if (!indices) {
            return NULL;
        }
        return indices; // Empty path returns empty array
    }

    // Initial capacity: estimate max number of tokens
    size_t capacity = (path_length / 2) + 1;
    DEBUG_SCOPE(int* indices = alloc(NULL, sizeof(int) * capacity));
    if (!indices) {
        return NULL; // Handle allocation failure
    }
    size_t count = 0;

    const char* p = path;
    const char* end = path + path_length;

    while (p < end) {
        // Skip extra slashes
        while (p < end && *p == '/') {
            p++;
        }
        if (p >= end) {
            break;
        }

        // Handle ".." component
        if (p + 1 < end && p[0] == '.' && p[1] == '.' &&
            (p + 2 == end || p[2] == '/')) {
            if (count > 0 && indices[count - 1] != -1) {
                count--; // Pop previous token
            } else {
                if (count >= capacity) {
                    capacity *= 2;
                    DEBUG_SCOPE(int* new_indices = realloc(indices, sizeof(int) * capacity));
                    if (!new_indices) {
                        DEBUG_SCOPE(free(indices));
                        return NULL;
                    }
                    indices = new_indices;
                }
                indices[count++] = -1;
            }
            p += 2;
            if (p < end && *p == '/') {
                p++;
            }
            continue;
        }

        // Handle "." component
        if (p[0] == '.' && (p + 1 == end || p[1] == '/')) {
            p++;
            if (p < end && *p == '/') {
                p++;
            }
            continue;
        }

        // Parse number token
        DEBUG_ASSERT(*p != '-', "Invalid negative number; use '..' for -1");
        DEBUG_ASSERT(isdigit((unsigned char)*p), "Expected digit");
        int number = 0;
        while (p < end && *p != '/') {
            DEBUG_ASSERT(isdigit((unsigned char)*p), "Invalid character in number");
            number = number * 10 + (*p - '0');
            p++;
        }
        if (count >= capacity) {
            capacity *= 2;
            DEBUG_SCOPE(int* new_indices = realloc(indices, sizeof(int) * capacity));
            if (!new_indices) {
                DEBUG_SCOPE(free(indices));
                return NULL;
            }
            indices = new_indices;
        }
        indices[count++] = number;
    }

    // Shrink to fit
    DEBUG_SCOPE(int* final_indices = realloc(indices, sizeof(int) * count));
    if (!final_indices) {
        DEBUG_SCOPE(free(indices));
        return NULL;
    }
    indices = final_indices;

    *out_indices_count = count;
    return indices;
}
char* vec_Path_FromVaArgs(size_t n_args, ...) {
    if (n_args == 0) {
        char* empty = alloc(NULL, 1);
        if (empty) { empty[0] = '\0'; }
        return empty;
    }

    va_list args;
    va_start(args, n_args);
    int* p_indices = alloc(NULL, n_args * sizeof(int));
    if (!p_indices) {
        va_end(args);
        return NULL;
    }
    for (size_t i = 0; i < n_args; i++) {
        p_indices[i] = va_arg(args, int);
    }
    va_end(args);

    // Calculate required length.
    size_t total_len = 1; // Start with a '/'
    for (size_t i = 0; i < n_args; i++) {
        if (p_indices[i] == -1) {
            total_len += 2; // ".."
        } else {
            int num = p_indices[i];
            int digits = (num == 0) ? 1 : 0;
            while (num != 0) { digits++; num /= 10; }
            total_len += digits;
        }
        if (i < n_args - 1) { total_len += 1; } // Separator
    }
    total_len += 1; // Null terminator

    char* ptr = alloc(NULL, total_len);
    if (!ptr) { free(p_indices); return NULL; }
    char* p = ptr;
    *p++ = '/';
    for (size_t i = 0; i < n_args; i++) {
        if (p_indices[i] == -1) {
            *p++ = '.';
            *p++ = '.';
        } else {
            char temp[12]; // Buffer for number conversion.
            int len = snprintf(temp, sizeof(temp), "%d", p_indices[i]);
            memcpy(p, temp, len);
            p += len;
        }
        if (i < n_args - 1) {
            *p++ = '/';
        }
    }
    *p = '\0';
    free(p_indices);
    return ptr;
}
