#include "arr.h"
#include <stdio.h>
#include <stdlib.h>
#include "debug.h"

void arr_Initialize(Arr* p_arr) {
    p_arr->p_data = NULL;
    p_arr->count = 0;
    p_arr->capacity = 0;
}
Arr arr_Create() {
    Arr arr;
    arr_Initialize(&arr);
    return arr;
}
unsigned char* arr_At(Arr arr) {
    return arr.p_data;
}
size_t arr_GetCount(Arr arr) {
    return arr.count;
}
size_t arr_GetCapacity(Arr arr) {
    return arr.capacity;
}
void arr_SetCount(Arr* p_arr, size_t new_count) {
    if (p_arr->capacity == 0) {
        p_arr->capacity = 1;
    }
    while (new_count > p_arr->capacity) {
        p_arr->capacity = p_arr->capacity * 2;
    }
    DEBUG_SCOPE(unsigned char* p_arr->p_data = (unsigned char*)alloc(p_arr->p_data, p_arr->capacity * sizeof(unsigned char)));
    p_arr->count = new_count;
    return true;
}
bool arr_SetCapacity(Arr* p_arr, size_t new_capacity) {
    ASSERT(new_capacity < p_arr->count, "cant sett capacity that is less than count");
    DEBUG_SCOPE(unsigned char* new_data = alloc(p_arr->p_data, new_capacity * sizeof(unsigned char)));
    p_arr->p_data = new_data;
    p_arr->capacity = new_capacity;
    return true;
}