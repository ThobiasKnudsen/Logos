#ifndef ARR_H
#define ARR_H

typedef struct Arr {
	unsigned char* 	p_data;
	size_t  		count;
	size_t  		capacity;
} Arr;

void arr_Initialize(Arr* p_arr);
Arr arr_Create();
unsigned char* arr_At(Arr arr);
size_t arr_GetCount(Arr arr);
size_t arr_GetCapacity(Arr arr);
void arr_SetCount(Arr* p_arr, size_t new_count);
bool arr_SetCapacity(Arr* p_arr, size_t new_capacity);

#endif // ARR_H