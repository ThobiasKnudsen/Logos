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
int* vec_Path_ToIndices(const char* path, size_t path_length, size_t* const out_indices_count) {
    DEBUG_ASSERT(path, "NULL pointer");
    DEBUG_ASSERT(path_length, "provided path length is zero");
    DEBUG_ASSERT(path[0] == '/', "Path must start with '/'");
    DEBUG_ASSERT(path[path_length - 1] == '/', "Path must end with '/'");
    
    // Allocate an initial array (worst-case every other character starts a token).
    size_t capacity = (path_length / 2) + 1;
    DEBUG_SCOPE(int* indices = malloc(sizeof(int) * capacity));
    DEBUG_ASSERT(indices, "malloc failed");
    if (!indices) return NULL;
    
    size_t count = 0;
    const char* p = path;
    const char* end = path + path_length;
    
    while (p < end) {
        // Skip any extra slashes.
        while (p < end && *p == '/')
            p++;
        if (p >= end)
            break;
        
        // Check for ".." component.
        if ((p + 1 < end) &&
            (p[0] == '.') && (p[1] == '.') &&
            ((p + 2 == end) || (p[2] == '/')))
        {
            // ".." should only appear when there's a previous valid token;
            // if not, treat it as a valid token representing moving up.
            if (count > 0 && indices[count - 1] != -1) {
                // Pop the last token.
                count--;
            } else {
                if (count >= capacity) {
                    capacity *= 2;
                    DEBUG_SCOPE(indices = realloc(indices, sizeof(int) * capacity));
                    DEBUG_ASSERT(indices, "realloc failed");
                }
                indices[count++] = -1;
            }
            p += 2;  // Skip ".."
            if (p < end && *p == '/')
                p++; // Skip trailing slash.
            continue;
        }
        
        // Check for a "." component.
        if ((p[0] == '.') && ((p + 1 == end) || (p[1] == '/'))) {
            p++;
            if (p < end && *p == '/')
                p++;
            continue;
        }
        
        // Parse a number token.
        // According to the design, only non-negative numbers are allowed.
        // An initial '-' is not allowed unless it forms the token "-1", which should have been
        // handled by the ".." check. We assert failure if we see a '-' here.
        DEBUG_ASSERT(*p != '-', "Invalid negative number token; only -1 (represented as '..') is allowed");
        
        DEBUG_ASSERT(isdigit((unsigned char)*p), "Expected digit in path component");
        int number = 0;
        while (p < end && *p != '/') {
            DEBUG_ASSERT(isdigit((unsigned char)*p), "Invalid character in number component");
            number = number * 10 + (*p - '0');
            p++;
        }
        
        if (count >= capacity) {
            capacity *= 2;
            DEBUG_SCOPE(indices = realloc(indices, sizeof(int) * capacity));
            DEBUG_ASSERT(indices, "realloc failed");
        }
        indices[count++] = number;
    }
    
    // Optionally shrink allocation.
    DEBUG_SCOPE(indices = realloc(indices, sizeof(int) * count));
    DEBUG_ASSERT(indices, "realloc failed");
    *out_indices_count = count;
    return indices;
}
char* vec_Path_FromVaArgs(size_t n_args, ...) {
    if (n_args == 0) { return ""; }
    va_list args;
    va_start(args, n_args);
    DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
    for (size_t i = 0; i < n_args; i++) {
        p_indices[i] = va_arg(args, int);
    }
    va_end(args);

    /*  First pass: calculate required length.
        For each element, we need:
          - 2 characters if the value is -1 (for ".."),
          - Otherwise, the number of digits in the integer (special-case 0 -> 1 digit).
        Also, every element gets a trailing '/' and the path starts with '/'.
        Finally, add one byte for the terminating '\0'. */
    size_t total_len = 0; // for initial '/'
    for (size_t i = 0; i < n_args; i++) {
        total_len += 1; // for the '/' before each element
        if (p_indices[i] == -1) {
            total_len += 2; // ".."
        } else {
            int x = p_indices[i];
            int digit_count = (x == 0) ? 1 : 0;
            for (int tmp = x; tmp > 0; tmp /= 10) {
                digit_count++;
            }
            total_len += digit_count;
        }
    }
    total_len += 1; // for '\0'

    DEBUG_SCOPE(char* ptr = alloc(NULL, total_len));
    char* p = ptr;

    // Begin with the starting slash.
    *p++ = '/';

    // Second pass: fill in each element.
    for (size_t i = 0; i < n_args; i++) {
        // Append slash before this element.
        *p++ = '/';
        int num = p_indices[i];
        if (num == -1) {
            // Append ".."
            *p++ = '.';
            *p++ = '.';
        } else if (num == 0) {
            *p++ = '0';
        } else {
            // Convert integer to string.
            // Since numbers are non-negative (other than -1) we can do this quickly.
            char temp[12]; // Enough for 32-bit integers.
            int len = 0;
            while (num > 0) {
                temp[len++] = '0' + (num % 10);
                num /= 10;
            }
            // The digits are in reverse order, so copy them in reverse.
            for (int j = len - 1; j >= 0; j--) {
                *p++ = temp[j];
            }
        }
    }
    // Null-terminate the string.
    *p = '\0';

    DEBUG_SCOPE(free(p_indices));
    return ptr;
}
