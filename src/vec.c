#include "vec.h"
#include "vec_path.h"
#include "type.h"
#include "debug.h"

#include <ctype.h>
#include <stdlib.h>
#include <stdarg.h>

Type vec_type = 0;

// ================================================================================================================================
// Fundamental
// ================================================================================================================================
    __attribute__((constructor(103)))
    void _vec_Constructor() {
        vec_type = type_Create("Vec", sizeof(Vec), vec_Destroy);
    }
    void _vec_Initialize(Vec* p_vec, Vec* p_parent, Type type) {
    	DEBUG_ASSERT(p_vec, "NULL pointer");
        DEBUG_ASSERT(vec_IsNull_UnsafeRead(p_vec), "vec should be Null before initializing it");
    	DEBUG_ASSERT(type_IsValid(type), "type is invlaid");
        DEBUG_SCOPE(Type_Info type_info = type_GetTypeInfo(type));
        #ifdef DEBUG
            p_vec->p_debug_mutex = SDL_CreateMutex();
        #endif
    	p_vec->p_write_mutex = SDL_CreateMutex();
    	p_vec->p_read_mutex = SDL_CreateMutex();
        p_vec->p_parent = p_parent;
    	p_vec->p_data = NULL;
    	p_vec->element_size = type_info.size;
    	p_vec->element_type = type;
    	p_vec->this_type = vec_type;
    	p_vec->count = 0;
    	p_vec->capacity = 0;
    	p_vec->reading_count = 0;
    }
    Vec vec_Create(Vec* p_parent, Type type) {
    	Vec vec = {0};
    	DEBUG_SCOPE(_vec_Initialize(&vec, p_parent, type));
    	return vec;
    }
    void vec_Destroy(void* p_vec) {
        Vec* p_vec_cast = (Vec*)p_vec;
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec_cast), "Vec is invalid");
        DEBUG_SCOPE(vec_LockRead(p_vec_cast));
        DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec_cast));
        DEBUG_SCOPE(Type_Info type_info = type_GetTypeInfo(p_vec_cast->element_type));
        DEBUG_SCOPE(Type_Destructor type_destructor = type_info.destructor);
        for (unsigned int i = 0; i < count; ++i) {
            type_destructor(vec_GetAtVaArgs_LockRead(p_vec_cast, 1, i));
        }
        DEBUG_SCOPE(vec_UnlockRead(p_vec_cast));
        #ifdef DEBUG
            DEBUG_SCOPE(SDL_DestroyMutex(p_vec_cast->p_debug_mutex));
        #endif
        DEBUG_SCOPE(SDL_DestroyMutex(p_vec_cast->p_read_mutex));
        DEBUG_SCOPE(SDL_DestroyMutex(p_vec_cast->p_write_mutex));
        memset(p_vec_cast, 0, sizeof(Vec));
    }

// ================================================================================================================================
// Debugging
// ================================================================================================================================
    bool vec_IsNull_UnsafeRead(Vec* p_vec) {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        if (p_vec->p_write_mutex != NULL) {printf("p_vec->p_write_mutex != NULL\n"); return false;}
        if (p_vec->p_read_mutex != NULL) {printf("p_vec->p_read_mutex != NULL\n"); return false;}
        if (p_vec->p_parent != NULL) {printf("p_vec->p_parent != NULL\n"); return false;}
        if (p_vec->p_data != NULL) {printf("p_vec->p_data != NULL\n"); return false;}
        if (p_vec->reading_count != 0) {printf("p_vec->reading_count != 0\n"); return false;}
        if (p_vec->element_size != 0) {printf("p_vec->element_size != 0\n"); return false;}
        if (p_vec->element_type != 0) {printf("p_vec->element_type != 0\n"); return false;}
        if (p_vec->this_type != 0) {printf("p_vec->this_type == 0\n"); return false;}
        if (p_vec->count != 0) {printf("p_vec->this_type == 0\n"); return false;}
        if (p_vec->capacity != 0) {printf("p_vec->this_type == 0\n"); return false;}
        return true;
    }
    bool vec_IsNullAtVaArgs_SafeRead(Vec* p_vec, size_t n_args, ...) {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        if (n_args == 0) {
            return vec_IsNull_UnsafeRead(p_vec);
        }

        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (int i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);

        DEBUG_SCOPE(Vec* p_parent = vec_GetAtIndices_LockRead(p_vec, n_args-1, p_indices));
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_parent), "NULL pointer");
        DEBUG_SCOPE(Vec* p_element = vec_GetAt_Unsafe(p_parent, p_indices[n_args-1]));
        if (vec_IsValid_UnsafeRead(p_element))
        DEBUG_SCOPE(unsigned char* p_element = vec_GetAtVaArgs_LockRead(p_parent, 1, p_indices[n_args-1]));
        DEBUG_ASSERT(p_element, "NULL pointer");

        for (unsigned int i = 0; i < p_parent->element_size; ++i) {
            if (p_element[i] != 0) {
                return false;
            }
        }

        return true;
    }
    void vec_Print_UnsafeRead(Vec* p_vec, const unsigned int n_spaces, const unsigned int n_layers) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");

        vec_LockRead(p_vec);

        DEBUG_SCOPE(char* spaces = alloc(NULL, n_spaces));
        memset(spaces, ' ', n_spaces+1);
        spaces[n_spaces] = '\0';
        printf("%sVec at %p:\n", spaces, (const void*)p_vec);
        printf("%s    p_write_mutex:  %p\n", spaces, (void*)p_vec->p_write_mutex);
        printf("%s    p_write_mutex:  %p\n", spaces, (void*)p_vec->p_read_mutex);
        printf("%s    p_data:         %p\n", spaces, p_vec->p_data);
        printf("%s    element_size:   %u\n", spaces, p_vec->element_size);
        printf("%s    element_type:   %hu\n", spaces, p_vec->element_type);
        printf("%s    this_type:      %hu\n", spaces, p_vec->this_type);
        printf("%s    count:          %u\n", spaces, p_vec->count);
        printf("%s    capacity:       %u\n", spaces, p_vec->capacity);

        if (n_layers >= 1) {
    		if (p_vec->count >= 1) {
    			printf("%s    elements:\n", spaces);
    			if (p_vec->this_type == p_vec->element_type) {
    				for (unsigned int i = 0; i < p_vec->count; ++i) {
    					vec_Print_UnsafeRead((Vec*)&p_vec->p_data[i*p_vec->element_size], 4+n_spaces, n_layers-1);
    				}
    			}
    			else {
    				for (unsigned short i = 0; i < p_vec->count; ++i) {
    					for (unsigned int j = 0; j < p_vec->element_size/8; j++) {
    						printf("%s        0x%X\n", spaces, ((unsigned long long*)p_vec->p_data)[i*p_vec->element_size/8+j]);
    					}
    					printf("\n");
    				}
    			}
    		}
    	}
    	free(spaces);
    	vec_UnlockRead(p_vec);
    }
    unsigned int vec_GetReadingLocksCount(Vec* p_vec) {
        SDL_LockMutex(p_vec->p_read_mutex);
        unsigned int count = p_vec->reading_count;
        SDL_UnlockMutex(p_vec->p_read_mutex);
        return count;
    }
    bool vec_TryGetReadingLocksCount(Vec* p_vec, unsigned int* const p_count) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
        if (SDL_TryLockMutex(p_vec->p_read_mutex)) {
            *p_count = p_vec->reading_count;
            SDL_UnlockMutex(p_vec->p_read_mutex);
            return true;
        }
        return false;
    }

// ================================================================================================================================
// Custom locking
// ================================================================================================================================
    void vec_LockRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
        SDL_LockMutex(p_vec->p_read_mutex);
        #ifdef DEBUG
            SDL_LockMutex(p_vec->p_debug_mutex);
        #endif 
        if (p_vec->reading_count == 0) {
            SDL_LockMutex(p_vec->p_write_mutex);
        }
        p_vec->reading_count++;
        #ifdef DEBUG
            SDL_UnlockMutex(p_vec->p_debug_mutex);
        #endif 
        SDL_UnlockMutex(p_vec->p_read_mutex);
    }
    void vec_LockWrite(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
        SDL_LockMutex(p_vec->p_write_mutex);
        #ifdef DEBUG
            SDL_LockMutex(p_vec->p_debug_mutex);
            DEBUG_ASSERT(p_vec->reading_count == 0, "reading_count is creater than 0 even though write was just locked");
            SDL_UnlockMutex(p_vec->p_debug_mutex);
        #endif
    }
    void vec_UnlockRead(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	SDL_LockMutex(p_vec->p_read_mutex);
        #ifdef DEBUG
            SDL_LockMutex(p_vec->p_debug_mutex);
        #endif 
    	ASSERT(p_vec->reading_count!=0, "ptr = %p | youre trying to unlock read vec when there is no registered reading\n", p_vec->p_write_mutex);
    	p_vec->reading_count--;
    	// printf("ptr = %p | reading_count = %d\n", p_vec->p_write_mutex, p_vec->reading_count);
    	if (p_vec->reading_count==0) {
    		SDL_UnlockMutex(p_vec->p_write_mutex);
    	}
        #ifdef DEBUG
            SDL_UnlockMutex(p_vec->p_debug_mutex);
        #endif 
    	SDL_UnlockMutex(p_vec->p_read_mutex);
    }
    void vec_UnlockWrite(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	SDL_UnlockMutex(p_vec->p_write_mutex);
    }

// ================================================================================================================================
// IsValid Locking
// ================================================================================================================================
    bool vec_IsValid_UnsafeRead(Vec* p_vec) {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        if (p_vec->p_write_mutex == NULL) {printf("p_vec->p_write_mutex == NULL\n"); return false;}
        if (p_vec->p_read_mutex == NULL) {printf("p_vec->p_read_mutex == NULL\n"); return false;}
        if (SDL_TryLockMutex(p_vec->p_write_mutex)) { SDL_UnlockMutex(p_vec->p_write_mutex); }
        if (SDL_TryLockMutex(p_vec->p_read_mutex)) { SDL_UnlockMutex(p_vec->p_read_mutex); }
        if (p_vec->element_size == 0) {printf("p_vec->element_size == 0\n"); return false;}
        if (p_vec->element_type == 0) {printf("p_vec->element_type == 0\n"); return false;}
        if (p_vec->this_type == 0) {printf("p_vec->this_type == 0\n"); return false;}
        if (p_vec->count > p_vec->capacity) {printf("p_vec->count > p_vec->capacity\n"); return false;}
        return true;
    }
    bool vec_IsValidAtIndices_SafeRead(Vec* p_root_vec, Type type, size_t indices_count, const int* p_indices) {
        if (!vec_IsValid_UnsafeRead(p_root_vec)) {
            return false;
        }
        if (indices_count == 0) {
            return true;
        }

        Vec* p_current = p_root_vec;
        bool last_is_vec = false;
        Type last_type = null_type;
        bool is_valid = true;
        DEBUG_SCOPE(vec_LockRead(p_current));

        for (int i = 0; i < indices_count; ++i) {
            if (i < indices_count-1) {
                DEBUG_ASSERT(!(p_indices[i] >= 0 && p_indices[i+1] == -1), "positive number cannot come before -1. at index %d and %d\n", i, i+1);
            }
            int index = p_indices[i];
            DEBUG_ASSERT(-1 <= index, "index at index %d is %d is less than -1 %d\n", i+1, index, p_current->count);
            last_is_vec = p_current->this_type == p_current->element_type;
            if (index >= p_current->count) {
                is_valid = false; // index out of bounds
            }
            if (index == -1) {
                last_type = vec_type;
                if (!p_current->p_parent) {
                    is_valid = false; // parent doesnt exist
                }
                p_current = p_current->p_parent;
            } 
            else {
                last_type = p_current->element_type;
                if (!(last_is_vec || i+1 == indices_count)) {
                    is_valid = false; // only the last index is allowed to not be vec type
                }
                DEBUG_SCOPE(vec_LockRead(p_current));
                p_current = (Vec*)&p_current->p_data + index*p_current->element_size;
            }
            if (last_is_vec) {
                if (!vec_IsValid_UnsafeRead(p_current)) {
                    is_valid = false; // vec is invalid
                }
            }
        }
        if (last_is_vec) {
            DEBUG_SCOPE(vec_LockRead(p_current));
        }
        if (type != last_type) {
            is_valid = false; // last type is not the provided type
        }
        DEBUG_ASSERT(p_current == vec_GetAtIndices_UnlockRead(p_root_vec, indices_count, p_indices), "unlocking doesnt result in same pointer");
        return is_valid;
    }
    bool vec_IsValidAtVaArgs_SafeRead(Vec* p_root_vec, Type type, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(bool return_bool = vec_IsValidAtIndices_SafeRead(p_root_vec, type, n_args, p_indices));
        DEBUG_SCOPE(free(p_indices));
        return return_bool;
    }
    bool vec_IsValidAtPath_SafeRead(Vec* p_root_vec,  Type type, const char* path) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, strlen(path), &indices_count));
        DEBUG_ASSERT(p_indices, "Failed to get p_indices from path\n");
        DEBUG_SCOPE(bool return_bool = vec_IsValidAtIndices_SafeRead(p_root_vec, type, indices_count, p_indices));
        free(p_indices);
        return return_bool;
    }

// ================================================================================================================================
// GetAt Locking
// ================================================================================================================================
    void* _vec_GetAtIndices_Lock(Vec* p_root_vec, bool read_lock, size_t indices_count, const int* p_indices) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");

        if (indices_count == 0) {
            if (read_lock) {DEBUG_SCOPE(vec_LockRead(p_root_vec));}
            else {DEBUG_SCOPE(vec_LockWrite(p_root_vec));}
            return p_root_vec;
        }

        Vec* p_current = p_root_vec;
        bool last_is_vec = false;

        for (int i = 0; i < indices_count; ++i) {
            if (i < indices_count-1) {
                DEBUG_ASSERT(!(p_indices[i] >= 0 && p_indices[i+1] == -1), "positive number cannot come before -1. at index %d and %d\n", i, i+1);
            }
            int index = p_indices[i];
            DEBUG_ASSERT(-1 <= index && index < p_current->count, "index at index %d is %d and is out of bounds for count %d\n", i+1, index, p_current->count);
            if (index == -1) {
                DEBUG_ASSERT(!(i+1 == indices_count && !read_lock), "last index cannot be -1 when locking write\n");
                DEBUG_ASSERT(p_current->p_parent, "NULL pointer");
                p_current = p_current->p_parent;
                DEBUG_ASSERT(p_current->element_type == p_current->this_type, "type of parent and child is not the same when backpropagating from child to parent. Child depth %d\n", i+1);
            } else {
                last_is_vec = p_current->this_type == p_current->element_type;
                DEBUG_ASSERT(last_is_vec || i+1 == indices_count, "element at depth %d out of %d is not a Vec type. only the last provided depth is allowed to not be a Vec type", i+1, indices_count);
                DEBUG_SCOPE(vec_LockRead(p_current));
                p_current = (Vec*)&p_current->p_data[index * p_current->element_size];
            }
        }

        if (last_is_vec) {
            if (read_lock) {DEBUG_SCOPE(vec_LockRead(p_current));}
            else {DEBUG_SCOPE(vec_LockWrite(p_current));}
        }
        return (void*)p_current;
    }
    void* vec_GetAtIndices_LockRead(Vec* p_root_vec, size_t indices_count, const int* p_indices) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        DEBUG_ASSERT(p_indices, "NULL pointer\n");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Lock(p_root_vec, true, indices_count, p_indices));
        return ptr;
    }
    void* vec_GetAtIndices_LockWrite(Vec* p_root_vec, size_t indices_count, const int* p_indices) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        DEBUG_ASSERT(p_indices, "NULL pointer\n");
        DEBUG_ASSERT(indices_count, "provided indices_count == 0\n");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Lock(p_root_vec, false, indices_count, p_indices));
        return ptr;
    }
    void* vec_GetAtVaArgs_LockRead(Vec* p_root_vec, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Lock(p_root_vec, true, n_args, p_indices));
        DEBUG_SCOPE(free(p_indices));
        return ptr;
    }
    void* vec_GetAtVaArgs_LockWrite(Vec* p_root_vec, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Lock(p_root_vec, false, n_args, p_indices));
        free(p_indices);
        return ptr;
    }
    void* vec_GetAtPath_LockRead(Vec* p_root_vec, const char* path) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, strlen(path), &indices_count));
        DEBUG_ASSERT(p_indices, "Failed to get p_indices from path\n");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Lock(p_root_vec, true, indices_count, p_indices));
        free(p_indices);
        return ptr;
    }
    void* vec_GetAtPath_LockWrite(Vec* p_root_vec, const char* path) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, strlen(path), &indices_count));
        DEBUG_ASSERT(p_indices, "Failed to get p_indices from path\n");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Lock(p_root_vec, false, indices_count, p_indices));
        free(p_indices);
        return ptr;
    }

// ================================================================================================================================
// UpsertIndexOfNullElement Locking
// ================================================================================================================================
    int vec_UpsertIndexOfNullElement_SafeWrite(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");

        DEBUG_SCOPE(vec_LockRead(p_vec));
        DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
        DEBUG_SCOPE(Type element_type = vec_GetElementType_UnsafeRead(p_vec));
        
        for (unsigned int i = 0; i < count; ++i) {
            DEBUG_SCOPE(bool is_null = vec_IsNullAtVaArgs_SafeRead(p_vec, 1, i));
            if (is_null) {
                DEBUG_SCOPE(vec_UnlockRead(p_vec));
                return i;
            }
        }

        DEBUG_SCOPE(vec_UnlockRead(p_vec));
        DEBUG_SCOPE(vec_LockWrite(p_vec));
        DEBUG_SCOPE(vec_SetCount_UnsafeWrite(p_vec, count + 1));
        DEBUG_SCOPE(vec_UnlockWrite(p_vec));
        DEBUG_SCOPE(ASSERT(vec_IsNullAtVaArgs_SafeRead(p_vec, 1, count), "newly created element is not null"));
        return count;
    }

// ================================================================================================================================
// GetIndexOfFirstVecWithType Locking
// ================================================================================================================================
    int _vec_GetIndexOfFirstVecWithTypeFromIndex_SafeRead(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(p_vec->element_type == vec_type, "type of provided vec has to be vec_type");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");

        DEBUG_SCOPE(vec_LockRead(p_vec));
        DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
        int type_index = -1;
        for (unsigned int i = index; i < count; ++i) {
            void* element = (char*)p_vec->p_data + i * p_vec->element_size;
            DEBUG_SCOPE(vec_LockRead((Vec*)element));
            DEBUG_SCOPE(Type element_type = vec_GetElementType_UnsafeRead((Vec*)element));
            if (element_type == type) {
                type_index = i;
                DEBUG_SCOPE(vec_UnlockRead((Vec*)element));
                break; 
            }
            DEBUG_SCOPE(vec_UnlockRead((Vec*)element));
        }
        DEBUG_SCOPE(vec_UnlockRead(p_vec));
        return type_index;
    }
    int vec_GetIndexOfFirstVecWithType_SafeRead(Vec* p_vec, Type type) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_SCOPE(int return_index = _vec_GetIndexOfFirstVecWithTypeFromIndex_SafeRead(p_vec, type, 0));
        return return_index;
    }
    int vec_GetIndexOfFirstVecWithTypeFromIndex_SafeRead(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");
        DEBUG_SCOPE(int return_index = _vec_GetIndexOfFirstVecWithTypeFromIndex_SafeRead(p_vec, type, index));
        return return_index;
    }

// ================================================================================================================================
// UpsertIndexOfFirstVecWithType Locking
// ================================================================================================================================
    int _vec_UpsertIndexOfFirstVecWithTypeFromIndex_SafeWrite(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");

        DEBUG_SCOPE(int type_index = vec_GetIndexOfFirstVecWithTypeFromIndex_SafeRead(p_vec, type, index));

        if (type_index == -1) {
            DEBUG_SCOPE(vec_LockWrite(p_vec));
            DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
            DEBUG_SCOPE(vec_SetCount_UnsafeWrite(p_vec, count + 1));
            DEBUG_SCOPE(ASSERT(vec_IsNullAtVaArgs_SafeRead(p_vec, 1, count), "newly created element is not null"));
            Vec* new_element = (Vec*)((char*)p_vec->p_data + count * p_vec->element_size);
            DEBUG_SCOPE(_vec_Initialize(new_element, p_vec, type));
            DEBUG_SCOPE(vec_UnlockWrite(p_vec));
            return count;
        }
        return type_index;
    }
    int vec_UpsertIndexOfFirstVecWithType_SafeWrite(Vec* p_vec, Type type) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_SCOPE(int return_index = _vec_UpsertIndexOfFirstVecWithTypeFromIndex_SafeWrite(p_vec, type, 0));
        return return_index;
    }
    int vec_UpsertIndexOfFirstVecWithTypeFromIndex_SafeWrite(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");
        DEBUG_SCOPE(int return_index = _vec_UpsertIndexOfFirstVecWithTypeFromIndex_SafeWrite(p_vec, type, index));
        return return_index;
    }

// ================================================================================================================================
// UpsertNullElement Locking
// ================================================================================================================================
    void* _vec_UpsertNullElement_Lock(Vec* p_vec, bool lock_read) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");

        DEBUG_SCOPE(vec_LockRead(p_vec));
        DEBUG_SCOPE(Type element_type = vec_GetElementType_UnsafeRead(p_vec));
        DEBUG_SCOPE(vec_UnlockRead(p_vec));
        DEBUG_SCOPE(int index = vec_UpsertIndexOfNullElement_SafeWrite(p_vec));

        void* new_element = (char*)p_vec->p_data + index * p_vec->element_size;
        if (element_type == vec_type) {
            if (lock_read) {
                DEBUG_SCOPE(vec_LockRead((Vec*)new_element));
            } else {
                DEBUG_SCOPE(vec_LockWrite((Vec*)new_element));
            }
        }
        return new_element;
    }
    void* vec_UpsertNullElement_LockRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_SCOPE(void* ptr = _vec_UpsertNullElement_Lock(p_vec, true));
        return ptr;
    }
    void* vec_UpsertNullElement_LockWrite(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_SCOPE(void* ptr = _vec_UpsertNullElement_Lock(p_vec, false));
        return ptr;
    }

// ================================================================================================================================
// UpsertFirstVecWithType Locking
// ================================================================================================================================
    Vec* _vec_UpsertFirstVecWithTypeFromIndex_Lock(Vec* p_vec, Type type, int index, bool lock_read) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");

        DEBUG_SCOPE(int type_index = vec_GetIndexOfFirstVecWithTypeFromIndex_SafeRead(p_vec, type, index));

        if (type_index == -1) {
            DEBUG_SCOPE(vec_UnlockRead(p_vec));
            DEBUG_SCOPE(vec_LockWrite(p_vec));
            DEBUG_SCOPE(unsigned int current_count = vec_GetCount_UnsafeRead(p_vec));
            DEBUG_SCOPE(vec_SetCount_UnsafeWrite(p_vec, current_count + 1));
            DEBUG_SCOPE(ASSERT(vec_IsNullAtVaArgs_SafeRead(p_vec, 1, current_count), "newly created element is not null"));
            void* new_element = (char*)p_vec->p_data + current_count * p_vec->element_size;
            DEBUG_SCOPE(_vec_Initialize((Vec*)new_element, p_vec, type));
            DEBUG_SCOPE(vec_UnlockWrite(p_vec));
            DEBUG_SCOPE(vec_LockRead(p_vec));
            if (lock_read) {
                DEBUG_SCOPE(vec_LockRead((Vec*)new_element));
            } else {
                DEBUG_SCOPE(vec_LockWrite((Vec*)new_element));
            }
            return new_element;
        } else {
            return (Vec*)((char*)p_vec->p_data + type_index * p_vec->element_size);
        }
    }
    Vec* vec_UpsertFirstVecWithType_LockRead(Vec* p_vec, Type type) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_SCOPE(void* ptr = _vec_UpsertFirstVecWithTypeFromIndex_Lock(p_vec, type, 0, true));
        return ptr;
    }
    Vec* vec_UpsertFirstVecWithType_LockWrite(Vec* p_vec, Type type) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_SCOPE(void* ptr = _vec_UpsertFirstVecWithTypeFromIndex_Lock(p_vec, type, 0, false));
        return ptr;
    }
    Vec* vec_UpsertFirstVecWithTypeFromIndex_LockRead(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");
        DEBUG_SCOPE(void* ptr = _vec_UpsertFirstVecWithTypeFromIndex_Lock(p_vec, type, index, true));
        return ptr;
    }
    Vec* vec_UpsertFirstVecWithTypeFromIndex_LockWrite(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "vec is invalid");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");
        DEBUG_SCOPE(void* ptr = _vec_UpsertFirstVecWithTypeFromIndex_Lock(p_vec, type, index, false));
        return ptr;
    }

// ================================================================================================================================
// Create Locking
// ================================================================================================================================
    void* _vec_CreateAtIndices_Lock(Vec* p_root_vec, Type type, bool read_lock, size_t indices_count, int* p_indices) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid\n");
        DEBUG_ASSERT(indices_count >= 1, "root vec is already initialized. You cannot initialize it again\n");

        Vec* p_current = p_root_vec;
        Vec* p_prev = NULL;
        bool last_is_vec = false;

        for (int i = 0; i < indices_count; ++i) {
            if (i < indices_count-1) {
                DEBUG_ASSERT(!(p_indices[i] >= 0 && p_indices[i+1] == -1), "positive number cannot come before -1. at index %d and %d\n", i, i+1);
            }
            int index = p_indices[i];
            DEBUG_ASSERT(-1 <= index, "index at index %d is less than -1 %d\n", i+1, index);
            if (index == -1) {
                DEBUG_ASSERT(i+1 != indices_count, "last index cannot be -1 when creating\n");
                DEBUG_ASSERT(p_current->p_parent, "NULL pointer");
                p_current = p_current->p_parent;
                DEBUG_ASSERT(p_current->element_type == p_current->this_type, "type of parent and child is not the same when backpropagating from child to parent. Child depth %d\n", i+1);
            } else {
                last_is_vec = vec_type == p_current->element_type;
                DEBUG_ASSERT(last_is_vec || i+1 == indices_count, "element at depth %d out of %d is not a Vec type. only the last provided depth is allowed to not be a Vec type", i+1, indices_count);
                if (index >= p_current->count) {
                    DEBUG_SCOPE(vec_LockWrite(p_current));
                    DEBUG_SCOPE(vec_SetCount_UnsafeWrite(p_current, index));
                    if (last_is_vec) {
                        DEBUG_SCOPE(_vec_Initialize((Vec*)&p_current->p_data[index], p_current, type));
                    }
                    DEBUG_SCOPE(vec_UnlockWrite(p_current));
                }
                DEBUG_SCOPE(vec_LockRead(p_current));
                if (last_is_vec) {
                    DEBUG_ASSERT(vec_IsValid_UnsafeRead((Vec*)&p_current->p_data[index]), "element is vector and is not valid");
                } else {
                    DEBUG_ASSERT(vec_IsNullAtVaArgs_SafeRead(p_current, 1, index), "element is null");
                }
                p_current = (Vec*)(p_current->p_data + index * p_current->element_size);
                if (last_is_vec) {
                    DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_current), "vec at depth %d is not valid\n", i+1);    
                }
            }
        }

        if (last_is_vec) {
            if (read_lock) {DEBUG_SCOPE(vec_LockRead(p_current));}
            else {DEBUG_SCOPE(vec_LockWrite(p_current));}
        }
        return (void*)p_current;
    }
    void* vec_CreateAtVaArgs_LockRead(Vec* p_root_vec, Type type, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        DEBUG_ASSERT(type_IsValid(type), "type is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(void* ptr = _vec_CreateAtIndices_Lock(p_root_vec, type, true, n_args, p_indices));
        DEBUG_SCOPE(free(p_indices));
        return ptr;
    }
    void* vec_CreateAtVaArgs_LockWrite(Vec* p_root_vec, Type type, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(void* ptr = _vec_CreateAtIndices_Lock(p_root_vec, type, false, n_args, p_indices));
        DEBUG_SCOPE(free(p_indices));
        return ptr;
    }

// ================================================================================================================================
// Unlocking
// ================================================================================================================================
    void* _vec_GetAtIndices_Unlock(Vec* p_root_vec, bool read_lock, size_t indices_count, const int* p_indices) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        DEBUG_ASSERT(p_indices, "NULL pointer");
        
        if (indices_count == 0) {
            if (read_lock) { DEBUG_SCOPE(vec_UnlockRead(p_root_vec)); } 
            else { DEBUG_SCOPE(vec_UnlockWrite(p_root_vec)); }
            return p_root_vec;
        }
        
        DEBUG_SCOPE(Vec** pp_vec = alloc(NULL, (indices_count + 1) * sizeof(Vec*)));
        memset(pp_vec, 0, (indices_count + 1) * sizeof(Vec*));
        pp_vec[0] = p_root_vec;
        bool last_is_vec = false;
      
        for (size_t i = 0; i < indices_count; i++) {
            int index = p_indices[i];
            index = (index < 0) ? (int)(pp_vec[i]->count) + index : index;
            last_is_vec = pp_vec[i]->this_type == pp_vec[i]->element_type;
            DEBUG_ASSERT(last_is_vec || i+1 == indices_count, "element at depth %d out of %d is not a Vec type. only the last provided depth is allowed to not be a Vec type", i+1, indices_count);
            DEBUG_ASSERT(0 <= index && index < pp_vec[i]->count, "index at depth %zu is %d and is out of bounds, which is %d\n", i, index, pp_vec[i]->count);
            pp_vec[i+1] = (Vec*)&pp_vec[i]->p_data[index * pp_vec[i]->element_size];
        	if (last_is_vec) {
            	DEBUG_ASSERT(vec_IsValid_UnsafeRead(pp_vec[i+1]), "vec at depth %d is not valid\n", i+1);
            }
        }

        if (last_is_vec) {
        	if (read_lock) {DEBUG_SCOPE(vec_UnlockRead(pp_vec[indices_count]));}
            else {DEBUG_SCOPE(vec_UnlockWrite(pp_vec[indices_count]));}
        }
      
        for (int i = (int)indices_count - 1; i >= 0; i--) {
            DEBUG_SCOPE(vec_UnlockRead(pp_vec[i]));
        }
        
        void* result = pp_vec[indices_count];
        free(pp_vec);
        return result;
    }
    void* vec_GetAtIndices_UnlockRead(Vec* p_root_vec, size_t indices_count, const int* p_indices) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        DEBUG_ASSERT(p_indices, "NULL pointer");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Unlock(p_root_vec, true, indices_count, p_indices));
        return ptr;
    }
    void* vec_GetAtIndices_UnlockWrite(Vec* p_root_vec, size_t indices_count, const int* p_indices) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        DEBUG_ASSERT(p_indices, "NULL pointer");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Unlock(p_root_vec, false, indices_count, p_indices));
        return ptr;
    }
    void* vec_GetAtVaArgs_UnlockRead(Vec* p_root_vec, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Unlock(p_root_vec, true, n_args, p_indices));
        free(p_indices);
        return ptr;
    }
    void* vec_GetAtVaArgs_UnlockWrite(Vec* p_root_vec, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Unlock(p_root_vec, false, n_args, p_indices));
        free(p_indices);
        return ptr;
    }
    void* vec_GetAtPath_UnlockRead(Vec* p_root_vec, const char* path) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, strlen(path), &indices_count));
        DEBUG_ASSERT(p_indices, "Failed to get p_indices from path\n");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Unlock(p_root_vec, true, indices_count, p_indices));
        free(p_indices);
        return ptr;
    }
    void* vec_GetAtPath_UnlockWrite(Vec* p_root_vec, const char* path) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_root_vec), "p_root_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, strlen(path), &indices_count));
        DEBUG_ASSERT(p_indices, "Failed to get p_indices from path\n");
        DEBUG_SCOPE(void* ptr = _vec_GetAtIndices_Unlock(p_root_vec, false, indices_count, p_indices));
        free(p_indices);
        return ptr;
    }

// ================================================================================================================================
// Get
// ================================================================================================================================
    void* vec_GetAt_Unsafe(Vec* p_vec, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "NULL pointer");
        DEBUG_ASSERT(-1 <= index && index < p_vec->count, "index out of bounds");
        if (index == -1) {
            DEBUG_ASSERT(p_vec->parent, "NULL pointer");
            return p_vec->parent;
        } else {
            return p_vec->p_data + p_vec->count * index;
        }
    }
    unsigned int vec_GetElementSize_UnsafeRead(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	return p_vec->element_size;
    }
    unsigned short vec_GetElementType_UnsafeRead(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	return p_vec->element_type;
    }
    unsigned short vec_GetThisType_UnsafeRead(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	return p_vec->this_type;
    }
    unsigned int vec_GetCount_UnsafeRead(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	return p_vec->count;
    }
    unsigned int vec_GetCapacity_UnsafeRead(Vec* p_vec) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	return p_vec->capacity;
    }

// ================================================================================================================================
// Set
// ================================================================================================================================
    void vec_SetElementSize_UnsafeWrite(Vec* p_vec, unsigned int size){
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	p_vec->element_size = size;
    }
    void vec_SetElementType_UnsafeWrite(Vec* p_vec, Type type){
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	p_vec->element_type = type;
    }
    void vec_SetThisType_UnsafeWrite(Vec* p_vec, Type type){
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	p_vec->this_type = type;
    }
    void vec_SetCount_UnsafeWrite(Vec* p_vec, unsigned int count) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	if (p_vec->count < count) {
    		if (count > p_vec->capacity) {
    			unsigned int new_capacity = 1;
    			while (count >= new_capacity) {
    				new_capacity*=2;
    			}
    			DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, new_capacity*p_vec->element_size));
    			p_vec->capacity = new_capacity;
    		}
    		memset((unsigned char*)(p_vec->p_data)+p_vec->count*p_vec->element_size, 0, (count - p_vec->count) * p_vec->element_size);
    	}
    	p_vec->count = count;
    }
    void vec_SetCapacity_UnsafeWrite(Vec* p_vec, unsigned int capacity) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	if (p_vec->capacity != capacity) {
    		DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, capacity*p_vec->element_size));
    		p_vec->capacity = capacity;
    	}
    	if (p_vec->count > capacity) {
    		p_vec->count = capacity;
    	}
    }

/*
void vec_CopyElement_SafeRead(
	Vec* p_vec, 
	unsigned int index, 
	void** const pp_dst_data, 
	unsigned int* p_dst_data_size) 
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	SDL_LockRWLockForReading(p_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_vec->count <= p_vec->capacity, "Count is greater than capacity");
	DEBUG_ASSERT(p_vec->count > index, "Index out of bounds");
	DEBUG_ASSERT(pp_dst_data, "NULL pointer");
	if (*p_dst_data_size < p_vec->element_size || *pp_dst_data == NULL) {
		*p_dst_data_size = p_vec->element_size;
		DEBUG_SCOPE(*pp_dst_data = alloc(*pp_dst_data, p_vec->element_size));
	}
	if (p_vec->this_type == p_vec->element_type) {
		SDL_RWLock* p_sub_rwlock = ((Vec*)&p_vec->p_data[index*p_vec->element_size])->p_rwlock;
		DEBUG_ASSERT(p_sub_rwlock, "NULL pointer");
		SDL_LockRWLockForReading(p_sub_rwlock);
		memcpy(*pp_dst_data, (unsigned char*)(p_vec->p_data)+index*p_vec->element_size, p_vec->element_size);
		SDL_UnlockRWLock(p_sub_rwlock);
	} else {
		memcpy(*pp_dst_data, (unsigned char*)(p_vec->p_data)+index*p_vec->element_size, p_vec->element_size);
	}
	SDL_UnlockRWLock(p_vec->p_rwlock);
}
void vec_CopyElements_SafeRead(
	Vec* p_vec, 
	unsigned int index, 
	unsigned int count, 
	void** const pp_dst_data, 
	unsigned int* p_dst_data_size) 
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	SDL_LockRWLockForReading(p_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_vec->count <= p_vec->capacity, "Count is greater than capacity");
	DEBUG_ASSERT(p_vec->count >= index*count, "Index out of bounds");
	DEBUG_ASSERT(pp_dst_data, "NULL pointer");
	if (*p_dst_data_size < count*p_vec->element_size || *pp_dst_data == NULL) {
		*p_dst_data_size = count*p_vec->element_size;
		DEBUG_SCOPE(*pp_dst_data = alloc(*pp_dst_data, count*p_vec->element_size));
	}
	if (p_vec->this_type == p_vec->element_type) {
		void* p_sub_rwlock = (unsigned char*)(p_vec->p_data)+index*p_vec->element_size;
		for (unsigned int i = 0; i < count; ++i) {
			DEBUG_ASSERT((SDL_RWLock*)p_sub_rwlock, "NULL pointer");
			SDL_LockRWLockForReading((SDL_RWLock*)p_sub_rwlock);
			p_sub_rwlock += p_vec->element_size;
		}
		memcpy(*pp_dst_data, (unsigned char*)(p_vec->p_data)+index*p_vec->element_size, count*p_vec->element_size);
		p_sub_rwlock -= count * p_vec->element_size;
		for (unsigned int i = 0; i < count; ++i) {
			DEBUG_ASSERT((SDL_RWLock*)p_sub_rwlock, "NULL pointer");
			SDL_UnlockRWLock((SDL_RWLock*)p_sub_rwlock);
			p_sub_rwlock += p_vec->element_size;
		}
	} else {
		memcpy(*pp_dst_data, (unsigned char*)(p_vec->p_data)+index*p_vec->element_size, count*p_vec->element_size);
	}
	SDL_UnlockRWLock(p_vec->p_rwlock);
}

// ================================================================================================================================
// Locks writing
// ================================================================================================================================
void vec_SetCount_SafeWrite(
	Vec* p_vec, 
	unsigned int count) 
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	if (p_vec->count < count) {
		if (count > p_vec->capacity) {
			unsigned int new_capacity = 1;
			while (count >= new_capacity) {
				new_capacity*=2;
			}
			DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, new_capacity*p_vec->element_size));
			p_vec->capacity = new_capacity;
		}
		memset(&((unsigned char*)p_vec->p_data)[p_vec->count*p_vec->element_size], 0, (count-p_vec->count)*p_vec->element_size);
		// if elements are Vec as well then rwlock needs to be created
		if (p_vec->this_type == p_vec->element_type) {
			Vec* p_sub_vec = (Vec*)(p_vec->p_data);
			for (unsigned int i = p_vec->count; i < count; ++i) {
				p_sub_vec[i].p_rwlock = SDL_CreateRWLock();
			}
		}
	}
	p_vec->count = count;
	SDL_UnlockRWLock(p_vec->p_rwlock);
}
void vec_SetCapacity_SafeWrite(
	Vec* p_vec, 
	unsigned int capacity) 
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	if (p_vec->capacity != capacity) {
		DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, capacity*p_vec->element_size));
		p_vec->capacity = capacity;
		if (p_vec->count > capacity) {
			p_vec->capacity = capacity;
		}
	}
	SDL_UnlockRWLock(p_vec->p_rwlock);	
}
void vec_OverwriteElement_SafeWrite(
	Vec* p_vec, 
	unsigned int index,
	const void * const p_src_data) 
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	DEBUG_ASSERT(p_src_data, "NULL pointer");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->count > index, "index is out of bounds");
	DEBUG_ASSERT(p_vec->count <= p_vec->capacity, "Count is greater than capacity");
	memcpy((unsigned char*)(p_vec->p_data)+index*p_vec->element_size, p_src_data, p_vec->element_size);
	SDL_UnlockRWLock(p_vec->p_rwlock);
}
void vec_OverwriteElements_SafeWrite(
	Vec* p_vec, 
	unsigned int index, 
	unsigned int count, 
	const void * const p_src_data) 
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	DEBUG_ASSERT(p_src_data, "NULL pointer");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->count >= index+count, "index is out of bounds");
	DEBUG_ASSERT(p_vec->count <= p_vec->capacity, "Count is greater than capacity");
	memcpy((unsigned char*)(p_vec->p_data)+index*p_vec->element_size, p_src_data, count*p_vec->element_size);
	SDL_UnlockRWLock(p_vec->p_rwlock);
}
void vec_OverwriteFromOther_SafeWrite(
	Vec* p_vec, 
	unsigned int index, 
	Vec* p_other_vec, 
	unsigned int other_index, 
	unsigned int count) 
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	DEBUG_ASSERT(p_other_vec, "NULL pointer");
	DEBUG_ASSERT(p_other_vec->p_rwlock, "NULL pointer");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	SDL_LockRWLockForReading(p_other_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_other_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_vec->element_type == p_other_vec->element_type, "element types are not the same");
	DEBUG_ASSERT(p_vec->element_size == p_other_vec->element_size, "element sizes are not the same");
	DEBUG_ASSERT(p_vec->count >= index+count, "index+count is out of dst array bounds");
	DEBUG_ASSERT(p_other_vec->count >= other_index+count, "index+count is out of src array bounds");
	DEBUG_ASSERT(p_vec->count <= p_vec->capacity, "count is greater than capacity in dst");
	DEBUG_ASSERT(p_other_vec->count <= p_other_vec->capacity, "count is greater than capacity in src");
	unsigned int size = p_vec->element_size;
	memcpy((unsigned char*)(p_vec->p_data)+index*size, p_other_vec->p_data + other_index*size, count*size);
	SDL_UnlockRWLock(p_other_vec->p_rwlock);
	SDL_UnlockRWLock(p_vec->p_rwlock);
}
void vec_AppendElement_SafeWrite(
	Vec* p_vec,
	const void * const p_src_data)
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	DEBUG_ASSERT(p_src_data, "NULL pointer");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	if (p_vec->count >= p_vec->capacity) {
		unsigned int new_capacity = 1;
		while(p_vec->count >= new_capacity) {
			new_capacity*=2;
		}
		DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, new_capacity*p_vec->element_size));
	}
	memcpy((unsigned char*)(p_vec->p_data)+p_vec->count*p_vec->element_size, p_src_data, p_vec->element_size);
	p_vec->count++;
	SDL_UnlockRWLock(p_vec->p_rwlock);
}
void vec_AppendElements_SafeWrite(
	Vec* p_vec, 
	unsigned int count, 
	const void * const p_src_data)
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	DEBUG_ASSERT(p_src_data, "NULL pointer");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	if (p_vec->count + count >= p_vec->capacity) {
		unsigned int new_capacity = 1;
		while(p_vec->count + count >= new_capacity) {
			new_capacity*=2;
		}
		DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, new_capacity*p_vec->element_size));
	}
	memcpy((unsigned char*)(p_vec->p_data)+p_vec->count*p_vec->element_size, p_src_data, count*p_vec->element_size);
	p_vec->count += count;
	SDL_UnlockRWLock(p_vec->p_rwlock);
}
void vec_InsertElement_SafeWrite(
	Vec* p_vec, 
	unsigned int index, 
	const void * const p_src_data) 
{

}
void vec_InsertElements_SafeWrite(
	Vec* p_vec, 
	unsigned int index, 
	unsigned int count, 
	const void * const p_src_data) 
{

}
void vec_RemoveElementBack_SafeWrite(
	Vec* p_vec)
{

}
void vec_RemoveElementsBack_SafeWrite(
	Vec* p_vec, 
	unsigned int count) 
{

}
void vec_RemoveElement_SafeWrite(
	Vec* p_vec, 
	unsigned int index)
{

}
void vec_RemoveElements_SafeWrite(
	Vec* p_vec, 
	unsigned int index, 
	unsigned int count)
{

}
void vec_Destroy_SafeWrite(
	Vec* p_vec)
{
	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
	SDL_DestroyRWLock(p_vec->p_rwlock);
}
*/