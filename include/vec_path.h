#ifndef VEC_PATH_H
#define VEC_PATH_H

#include "vec_path.h"
#include "debug.h"
#include <stdlib.h>
#include <string.h>
#include <ctype.h>

char* vec_Path_Combine(const char* path_1, const char* path_2);
int* vec_Path_ToIndices(const char* path, size_t path_length, size_t* const out_indices_count);
char* vec_Path_FromVaArgs(size_t n_args, ...);

#endif // VEC_PATH_H