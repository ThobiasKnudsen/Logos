#include "vec.h"
#include "vec_path.h"
#include "type.h"
#include "debug.h"

#include <ctype.h>
#include <stdlib.h>
#include <stdarg.h>
#include <assert.h>

Type vec_type = 0;

// ================================================================================================================================
// Fundamental
// ================================================================================================================================
    __attribute__((constructor(103)))
    void _vec_Constructor() {
        vec_type = type_Create_Safe("Vec", sizeof(Vec), vec_Destroy);
    }
    void vec_Initialize(Vec* p_vec, Vec* p_parent, Type type) {
    	DEBUG_ASSERT(p_vec, "NULL pointer");
        DEBUG_ASSERT(vec_IsNull_UnsafeRead(p_vec), "vec should be Null before initializing it");
    	DEBUG_ASSERT(type_IsValid_Safe(type), "type is invlaid");
        DEBUG_SCOPE(Type_Info type_info = type_GetTypeInfo_Safe(type));
        p_vec->p_internal_lock = SDL_CreateMutex();
    	p_vec->p_rw_lock = SDL_CreateRWLock();
    	p_vec->p_read_lock = SDL_CreateMutex();
        p_vec->p_parent = p_parent;
    	p_vec->p_data = NULL;
        p_vec->reading_count = 0;
        p_vec->writing_locks = 0;
    	p_vec->type = type;
    	p_vec->count = 0;
    	p_vec->capacity = 0;
        ASSERT(vec_IsValid_SafeRead(p_vec), "newly created vec is invalid");
        printf("initialized new vec %p\n", p_vec);
    }
    Vec vec_Create(Vec* p_parent, Type type) {
    	Vec vec = {0};
    	DEBUG_SCOPE(vec_Initialize(&vec, p_parent, type));
    	return vec;
    }
    void vec_Destroy(void* p_vec) {
        Vec* p_vec_cast = (Vec*)p_vec;
        DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec_cast), "Vec is invalid");
        DEBUG_SCOPE(vec_LockWrite(p_vec_cast));
        DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec_cast));
        DEBUG_SCOPE(Type_Info type_info = type_GetTypeInfo_Safe(p_vec_cast->type));
        DEBUG_SCOPE(Type_Destructor type_destructor = type_info.destructor);
        for (unsigned int i = 0; i < count; ++i) {
            type_destructor(vec_GetElement_UnsafeRead(p_vec_cast, i, p_vec_cast->type));
        }
        DEBUG_SCOPE(vec_UnlockWrite(p_vec_cast));
        DEBUG_SCOPE(SDL_DestroyMutex(p_vec_cast->p_internal_lock));
        DEBUG_SCOPE(SDL_DestroyMutex(p_vec_cast->p_read_lock));
        DEBUG_SCOPE(SDL_DestroyRWLock(p_vec_cast->p_rw_lock));
        memset(p_vec_cast, 0, sizeof(Vec));
    }

// ================================================================================================================================
// Debugging
// ================================================================================================================================
    bool vec_IsNull_UnsafeRead(Vec* p_vec) {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        if (p_vec->p_rw_lock != NULL) {printf("p_vec->p_rw_lock != NULL. %p\n", p_vec); return false;}
        if (p_vec->p_read_lock != NULL) {printf("p_vec->p_read_lock != NULL. %p\n", p_vec); return false;}
        if (p_vec->p_internal_lock != NULL) {printf("p_vec->p_internal_lock != NULL. %p\n", p_vec); return false;}
        if (p_vec->p_parent != NULL) {printf("p_vec->p_parent != NULL. %p\n", p_vec); return false;}
        if (p_vec->p_data != NULL) {printf("p_vec->p_data != NULL. %p\n", p_vec); return false;}
        if (p_vec->reading_count != 0) {printf("p_vec->reading_count != 0. %p\n", p_vec); return false;}
        if (p_vec->writing_locks != 0) {printf("p_vec->writing_locks != 0. %p\n", p_vec); return false;}
        if (p_vec->type != 0) {printf("p_vec->type == 0. %p\n", p_vec); return false;}
        if (p_vec->count != 0) {printf("p_vec->count == 0. %p\n", p_vec); return false;}
        if (p_vec->capacity != 0) {printf("p_vec->capacity == 0. %p\n", p_vec); return false;}
        return true;
    }
    bool vec_IsNullAtIndices_SafeRead(Vec* p_vec, size_t indices_count, const int* p_indices) {
        DEBUG_SCOPE(DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "vec invalid"));
        if (indices_count == 0) {
            DEBUG_SCOPE(bool is_null = vec_IsNull_UnsafeRead(p_vec));
            return is_null;
        }

        bool is_null = false;
        Vec* p_current = p_vec;
        DEBUG_SCOPE(vec_LockRead(p_current));
        int i = 0;
        for (; i < indices_count; ++i) {
            DEBUG_SCOPE(unsigned char* p_tmp = (unsigned char*)vec_GetElement_UnsafeRead(p_current, p_indices[i], p_current->type));
            if (p_current->type != vec_type) {
                if (i < indices_count-1) {
                    is_null = true; // came to element which isnt vec type when not finished with p_indices
                } else {
                    bool tmp_is_null = true;
                    DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_current->type));
                    for (int j = 0; j < element_size; ++j) {
                        if (p_tmp[j] != 0) {
                            tmp_is_null = false; // as long as one byte is not null then the whole element is not null
                        }
                    }
                    if (tmp_is_null) {
                        is_null = true; // has arrived at the last index and its not a vec and its null
                    }
                }
                break;
            }
            DEBUG_SCOPE(bool tmp_is_null = vec_IsNull_UnsafeRead((Vec*)p_tmp));
            if (tmp_is_null) {
                is_null = true;
                break;
            }
            DEBUG_SCOPE(DEBUG_ASSERT(vec_IsValid_SafeRead((Vec*)p_tmp), "vec should be valid since its not null"));
            p_current = (Vec*)p_tmp;
            DEBUG_SCOPE(vec_LockRead(p_current));
        }
        for (; i > 0; --i) {
            DEBUG_SCOPE(vec_UnlockRead(p_current));
            p_current = p_current->p_parent;
            DEBUG_ASSERT(p_current, "should not be NULL because this vec should have been used in the previous loop");
        }
        DEBUG_SCOPE(vec_UnlockRead(p_current));

        return is_null;
    }
    bool vec_IsNullAtVaArgs_SafeRead(Vec* p_vec, size_t n_args, ...) {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (int i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(bool is_null = vec_IsNullAtIndices_SafeRead(p_vec, n_args, p_indices));
        free(p_indices);
        return is_null;
    }
    void vec_Print_UnsafeRead(Vec* p_vec, unsigned int n_layers) {
        DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
        DEBUG_SCOPE(vec_LockRead(p_vec));

        printf("Vec at %p:\n", (const void*)p_vec);
        printf("    p_rw_lock:       %p\n", (void*)p_vec->p_rw_lock);
        printf("    p_read_lock:     %p\n", (void*)p_vec->p_read_lock);
        printf("    p_internal_lock: %p\n", (void*)p_vec->p_internal_lock);
        printf("    reading_count:   %p\n", p_vec->reading_count);
        printf("    writing_locks:   %p\n", p_vec->writing_locks);
        printf("    p_data:          %p\n", p_vec->p_data);
        Type_Info info = type_GetTypeInfo_Safe(p_vec->type);
        printf("    type:            %s\n", info.name);
        printf("    type_size:       %hu\n", type_GetSize_Safe(p_vec->type));
        printf("    count:           %u\n", p_vec->count);
        printf("    capacity:        %u\n", p_vec->capacity);

        if (n_layers >= 1) {
    		if (p_vec->count >= 1) {
                DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_vec->type));
    			printf("    elements:\n");
    			if (p_vec->type == vec_type) {
    				for (unsigned int i = 0; i < p_vec->count; ++i) {
                        DEBUG_SCOPE(bool is_valid = vec_IsValid_SafeRead((Vec*)(p_vec->p_data + i*element_size)));
                        if (!is_valid) {
                            continue;
                        }
                        printf("    %d %p:\n", i, p_vec->p_data + i*element_size);
    					DEBUG_SCOPE(vec_Print_UnsafeRead((Vec*)(p_vec->p_data + i*element_size), n_layers-1));
    				}
    			}
    			else {
    				for (unsigned short i = 0; i < p_vec->count; ++i) {
                        printf("    %d non-vec at %p:\n",  i, p_vec->p_data + i*element_size);
    					for (unsigned int j = 0; j < element_size; j+=8) {
    						printf("        %p\n", *((unsigned long long*)(p_vec->p_data + i*element_size+j)));
    					}
    					printf("\n");
    				}
    			}
    		}
    	}
    	DEBUG_SCOPE(vec_UnlockRead(p_vec));
    }
    bool vec_MatchElement_SafeRead(
        Vec* p_vec, 
        unsigned char* p_data, 
        size_t data_size, 
        int** const pp_return_indices, 
        size_t* const p_return_indices_count) 
    {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        DEBUG_ASSERT(p_data, "NULL pointer");
        DEBUG_ASSERT(pp_return_indices, "NULL pointer");
        DEBUG_ASSERT(!(*pp_return_indices), "not NULL pointer");
        DEBUG_ASSERT(p_return_indices_count, "NULL pointer");

        DEBUG_SCOPE(Vec** pp_vec = vec_MoveStart(p_vec));

        bool found_match = false;

        do {
            DEBUG_SCOPE(Type type = vec_GetType_UnsafeRead(*pp_vec));
            DEBUG_SCOPE(Type_Size element_size = type_GetSize_Safe(type));
            DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(*pp_vec));

            for (unsigned int i = 0; i < count; ++i) {
                DEBUG_SCOPE(unsigned char* p_element = (unsigned char*)vec_GetElement_UnsafeRead(*pp_vec, i, (*pp_vec)->type));
                bool match = true;
                for (unsigned int j = 0; j < data_size && j < element_size; ++j) {
                    if (p_element[j] != p_data[j]) {
                        match = false;
                        break;
                    } 
                }
                if (match) {
                    for (unsigned int j = 0; j < data_size && j < element_size; ++j) {
                        printf("%d %d\n", p_element[j], p_data[j]);
                    }
                    found_match = true;
                    unsigned int depth = 1;
                    Vec* p_vec_tmp = *pp_vec;
                    while (p_vec_tmp != p_vec) {
                        DEBUG_ASSERT(p_vec_tmp->p_parent, "p_parent is NULL");
                        p_vec_tmp = p_vec_tmp->p_parent;
                        depth++;
                    }
                    DEBUG_SCOPE(*pp_return_indices = alloc(*pp_return_indices, sizeof(int) * depth));
                    for (int j = depth-2; j >= 0; --j) {
                        p_vec_tmp = *pp_vec;
                        printf("move backwards\n");
                        vec_MoveToIndex(pp_vec, -1, vec_type);
                        int index = -1;
                        DEBUG_SCOPE(count = vec_GetCount_UnsafeRead(*pp_vec));
                        for (unsigned int h = 0; h < count; ++h) {
                            DEBUG_SCOPE(Vec* p_vec_tmp_2 = (Vec*)vec_GetElement_UnsafeRead(*pp_vec, h, (*pp_vec)->type));
                            if (p_vec_tmp_2 == p_vec_tmp) {
                                index = h;
                                break;
                            }
                        }
                        DEBUG_ASSERT(index != -1, "didnt find index when recursively going backwards");
                        (*pp_return_indices)[j] = index;
                    }
                    DEBUG_ASSERT(*pp_vec == p_vec, "didnt backpropagate to correct Vec*");
                    (*pp_return_indices)[depth-1] = i;
                    *p_return_indices_count = depth;
                    found_match = true;
                    break;
                }
            } 
            if (!found_match) {
                if (type == vec_type) {
                    printf("new child = %d\n", 0);
                    DEBUG_SCOPE(vec_MoveToIndex(pp_vec, 0, ((Vec*)(*pp_vec)->p_data)->type));
                } else {
                    while (true) {
                        printf("one iteration\n");
                        Vec* p_vec_tmp = *pp_vec;
                        DEBUG_SCOPE(vec_MoveToIndex(pp_vec, -1, vec_type));
                        DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(*pp_vec));
                        int index = -1;
                        for (int i = 0; i < count-1; ++i) {
                            DEBUG_SCOPE(Vec* p_vec_tmp_2 = (Vec*)vec_GetElement_UnsafeRead(*pp_vec, i, (*pp_vec)->type));
                            if (p_vec_tmp_2 == p_vec_tmp) {
                                index = i;
                                printf("new child = %d\n", i+1);
                                DEBUG_SCOPE(vec_MoveToIndex(pp_vec, i+1, ((Vec*)(*pp_vec)->p_data)[(i+1)].type));
                                break;
                            }
                        }
                        if (index != -1) break;
                        if (*pp_vec == p_vec) break;
                    }
                }
            }

        } while (*pp_vec != p_vec);

        DEBUG_SCOPE(vec_MoveEnd(pp_vec));

        bool match = true;
        unsigned char* p_element = (unsigned char*)p_vec;
        for (unsigned int i = 0; i < data_size || i < sizeof(Vec); ++i) {
            if (p_element[i] != p_data[i]) {
                match = false;
                break;
            } 
        }
        if (match) {
            found_match = true;
            DEBUG_SCOPE(*pp_return_indices = alloc(*pp_return_indices, 0));
            *p_return_indices_count = 0;
        }

        if (found_match) {
            printf("path to match: ");
            for (unsigned int i = 0; i < *p_return_indices_count; ++i) {
                printf("%d ", (*pp_return_indices)[i]);
            }
            printf("\n");
        } else {
            DEBUG_SCOPE(free(*pp_return_indices));
        }
        return found_match;
    }

// ================================================================================================================================
// Custom locking
// ================================================================================================================================
    void vec_LockRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec)

        SDL_LockMutex(p_vec->p_read_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        DEBUG_ASSERT(p_vec->writing_locks == 0, "p_vec = %p | writing_locks is not 0. That should not be possible at this line", p_vec);
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_LockRWLockForReading(p_vec->p_rw_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        p_vec->reading_count++;
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_UnlockMutex(p_vec->p_read_lock);
    }
    void vec_LockWrite(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec);

        SDL_LockMutex(p_vec->p_read_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        DEBUG_ASSERT(p_vec->reading_count == 0, "p_vec = %p | reading_count is greater than 0. That should not be possible at this line", p_vec);
        DEBUG_ASSERT(p_vec->writing_locks == 0, "p_vec = %p | writing_locks is not 1. That should not be possible at this line", p_vec);
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_LockRWLockForWriting(p_vec->p_rw_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        p_vec->writing_locks++;
        SDL_UnlockMutex(p_vec->p_internal_lock);
    }
    void vec_UnlockRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec);

        SDL_LockMutex(p_vec->p_internal_lock);
        DEBUG_ASSERT(p_vec->reading_count >= 1, "p_vec = %p | youre trying to unlock read vec when there is no registered reading\n", p_vec);
        DEBUG_ASSERT(p_vec->writing_locks == 0, "p_vec = %p | writing_locks is not 0. That should not be possible at this line", p_vec);
        p_vec->reading_count--;
        SDL_UnlockMutex(p_vec->p_internal_lock);
        SDL_UnlockRWLock(p_vec->p_rw_lock);
    }
    void vec_UnlockWrite(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec);

        SDL_LockMutex(p_vec->p_internal_lock);
        DEBUG_ASSERT(p_vec->reading_count == 0, "p_vec = %p | reading_locks is greater than 0. That should not be possible at this line", p_vec);
        DEBUG_ASSERT(p_vec->writing_locks == 1, "p_vec = %p | writing_locks is not 1. That should not be possible at this line", p_vec);
        p_vec->writing_locks--;
        SDL_UnlockRWLock(p_vec->p_rw_lock);
        SDL_UnlockMutex(p_vec->p_read_lock);
        SDL_UnlockMutex(p_vec->p_internal_lock);
    }
    void vec_SwitchReadToWrite(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec);
        SDL_LockMutex(p_vec->p_read_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        DEBUG_ASSERT(p_vec->reading_count == 1, "p_vec = %p | reading_count(%d) is not 1 when switching from lock read to lock write. That should not happen here", p_vec, p_vec->reading_count);
        DEBUG_ASSERT(p_vec->writing_locks == 0, "p_vec = %p | writing_locks(%d) is not 0 when switching from lock read to lock write. That should not happen here", p_vec, p_vec->reading_count);
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_UnlockRWLock(p_vec->p_rw_lock);
        SDL_LockRWLockForWriting(p_vec->p_rw_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        p_vec->reading_count--;
        p_vec->writing_locks++;
        SDL_UnlockMutex(p_vec->p_internal_lock);
    }
    void vec_SwitchWriteToRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec);

        SDL_LockMutex(p_vec->p_internal_lock);
        DEBUG_ASSERT(p_vec->reading_count == 0, "p_vec = %p | reading_count(%d) is not 0 when switching from lock write to lock read. That should not happen here", p_vec, p_vec->reading_count);
        DEBUG_ASSERT(p_vec->writing_locks == 1, "p_vec = %p | writing_locks(%d) is not 1 when switching from lock write to lock read. That should not happen here", p_vec, p_vec->reading_count);
        p_vec->reading_count++;
        p_vec->writing_locks--;
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_UnlockRWLock(p_vec->p_rw_lock);
        SDL_LockRWLockForReading(p_vec->p_rw_lock);
        SDL_UnlockMutex(p_vec->p_read_lock);
    }
    unsigned short vec_GetReadingLocksCount(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec);
        SDL_LockMutex(p_vec->p_internal_lock);
        unsigned short reading_count = p_vec->reading_count;
        SDL_UnlockMutex(p_vec->p_internal_lock);
        return reading_count;
    }
    bool vec_IsWriteLocked(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec = %p is invalid\n", p_vec);
        SDL_LockMutex(p_vec->p_internal_lock);
        DEBUG_ASSERT(p_vec->writing_locks <= 1, "p_vec = %p | writing count is greater than 1");
        bool is_write_locked = p_vec->writing_locks == 1;
        SDL_UnlockMutex(p_vec->p_internal_lock);
        return is_write_locked;
    }

// ================================================================================================================================
// IsValid
// ================================================================================================================================
    bool vec_IsValid_SafeRead(Vec* p_vec) {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        if (p_vec->p_read_lock == NULL) {printf("p_vec->p_read_lock == NULL. %p\n", p_vec); return false;}
        if (p_vec->p_rw_lock == NULL) {printf("p_vec->p_rw_lock == NULL. %p\n", p_vec); return false;}
        if (p_vec->p_internal_lock == NULL) {printf("p_vec->p_internal_lock == NULL. %p\n", p_vec); return false;}
        DEBUG_SCOPE(vec_LockRead(p_vec));
        if (p_vec->type == null_type) {printf("p_vec->type == 0. %p\n", p_vec); return false;}
        if (p_vec->count > p_vec->capacity) {printf("p_vec->count > p_vec->capacity. %p\n", p_vec); return false;}
        DEBUG_SCOPE(vec_UnlockRead(p_vec));
        return true;
    }    
    bool vec_IsValid_UnsafeRead(Vec* p_vec) {
        DEBUG_ASSERT(p_vec, "NULL pointer");
        if (p_vec->p_read_lock == NULL) {printf("p_vec->p_read_lock == NULL. %p\n", p_vec); return false;}
        if (p_vec->p_rw_lock == NULL) {printf("p_vec->p_rw_lock == NULL. %p\n", p_vec); return false;}
        if (p_vec->p_internal_lock == NULL) {
            printf("p_vec->p_internal_lock == NULL. %p\n", p_vec); 
            return false;
        }
        if (p_vec->type == null_type) {printf("p_vec->type == 0. %p\n", p_vec); return false;}
        if (p_vec->count > p_vec->capacity) {printf("p_vec->count > p_vec->capacity. %p\n", p_vec); return false;}
        return true;
    }
    bool vec_IsValidAtIndices_SafeRead(Vec* p_vec, Type type, size_t indices_count, const int* p_indices) {
        DEBUG_SCOPE(bool is_valid = vec_IsValid_SafeRead(p_vec));
        if (!is_valid) {
            DEBUG_ASSERT(vec_IsNull_UnsafeRead(p_vec), "if vec is not valid then it should be null");
            return false;
        }

        bool is_null = false;
        Vec* p_current = p_vec;
        DEBUG_SCOPE(vec_LockRead(p_current));
        int i = 0;
        for (; i < indices_count; ++i) {
            DEBUG_SCOPE(unsigned char* p_tmp = (unsigned char*)vec_GetElement_UnsafeRead(p_current, p_indices[i], p_current->type));
            if (p_current->type != vec_type) {
                if (i < indices_count-1) {
                    printf("element is not vec type while also not the last depth which is %d\n", i+1);
                    is_null = true; // came to element which isnt vec type when not finished with p_indices
                } else {
                    bool tmp_is_null = true;
                    DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_current->type))
                    for (int j = 0; j < element_size; ++j) {
                        if (p_tmp[j] != 0) {
                            tmp_is_null = false; // as long as one byte is not null then the whole element is not null
                        }
                    }
                    if (tmp_is_null) {
                        printf("element is null\n");
                        is_null = true; // has arrived at the last index and its not a vec and its null
                    } else if (p_current->type != type) {
                        printf("element is not the given type\n");
                        is_valid = false; // came to last type and its not the given type
                    }
                }
                break;
            }
            DEBUG_SCOPE(bool tmp_is_valid = vec_IsValid_SafeRead((Vec*)p_tmp));
            if (!tmp_is_valid) {
                DEBUG_ASSERT(vec_IsNull_UnsafeRead((Vec*)p_tmp), "vec should be null if its not valid");
                printf("vec at depth %d is invalid\n", i+1);
                is_null = true;
                is_valid = false; 
                break;
            }
            p_current = (Vec*)p_tmp;
            DEBUG_SCOPE(vec_LockRead(p_current));
        }
        for (; i > 0; --i) {
            DEBUG_SCOPE(vec_UnlockRead(p_current));
            p_current = p_current->p_parent;
            DEBUG_ASSERT(p_current, "should not be NULL because this vec should have been used in the previous loop");
        }
        DEBUG_SCOPE(vec_UnlockRead(p_current));
        if (is_null) {
            is_valid = false;
        }

        return is_valid;
    }
    bool vec_IsValidAtVaArgs_SafeRead(Vec* p_vec, Type type, size_t n_args, ...) {
        DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        DEBUG_SCOPE(int* p_indices = alloc(NULL, n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        DEBUG_SCOPE(bool return_bool = vec_IsValidAtIndices_SafeRead(p_vec, type, n_args, p_indices));
        DEBUG_SCOPE(free(p_indices));
        return return_bool;
    }
    bool vec_IsValidAtPath_SafeRead(Vec* p_vec,  Type type, const char* path) {
        DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, &indices_count));
        DEBUG_ASSERT(p_indices, "Failed to get p_indices from path\n");
        DEBUG_SCOPE(bool return_bool = vec_IsValidAtIndices_SafeRead(p_vec, type, indices_count, p_indices));
        free(p_indices);
        return return_bool;
    }

// ================================================================================================================================
// MoveTo
// 
// These functions traverse a hierarchy of Vec objects with thread-safe read locks.
// When moving forward (parent to child), each intermediate Vec is locked, but the final Vec remains unlocked, letting you decide its lock state.
// When moving backward (child to parent), they unlock the parent Vecs as you go, so by the time you return to the starting Vec, all locks are released.
// For simplicity and safety, use only one Vec pointer per thread to avoid complex lock management.
// This also enforce index sequence rules: a positive index moves to a child, a negative index to a parent, and a positive index cannot immediately precede a negative one. 
// ================================================================================================================================
    Vec** vec_MoveStart(Vec* p_vec) {
        DEBUG_SCOPE(DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "vec is invalid"));
        DEBUG_SCOPE(ASSERT(!p_vec->p_parent, "p_vec is not ground Vec because p_parent is not NULL"));
        DEBUG_SCOPE(vec_LockRead(p_vec));
        DEBUG_SCOPE(Vec** pp_vec = alloc(NULL, sizeof(Vec*)));
        *pp_vec = p_vec;
        return pp_vec;
    }
    void vec_MoveEnd(Vec** pp_vec) {
        DEBUG_SCOPE(ASSERT(pp_vec, "pp_vec is NULL"));
        DEBUG_SCOPE(ASSERT(vec_IsValid_UnsafeRead(*pp_vec), "*pp_vec is invalid"));
        Vec* p_current = *pp_vec;
        DEBUG_SCOPE(vec_UnlockRead(p_current));
        while (p_current->p_parent) {
            p_current = p_current->p_parent;
            DEBUG_SCOPE(ASSERT(vec_IsValid_UnsafeRead(p_current), "p_vec %p | is invlaid"));
            DEBUG_SCOPE(vec_UnlockRead(p_current));
        }
        DEBUG_SCOPE(free(pp_vec));
    }
    void vec_MoveToIndex(Vec** pp_vec, int index, Type type) {
        DEBUG_ASSERT(pp_vec, "pp_vec is null\n");
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(*pp_vec), "*pp_vec is invalid\n");
        Vec* p_vec = *pp_vec;
        DEBUG_ASSERT(-1 <= index && index < (int)p_vec->count, "index(%d) is out of bounds(%d)", index, p_vec->count);
        if (index == -1) {
            Vec* p_parent = p_vec->p_parent;
            DEBUG_SCOPE(ASSERT(vec_IsValid_UnsafeRead(p_parent), "p_parent is not valid"));
            DEBUG_SCOPE(vec_UnlockRead(p_vec));
            DEBUG_SCOPE(ASSERT(type == vec_type, "wrong type. it has to be vec_type when going to p_parent"));
            *pp_vec = p_parent;
        } else {
            DEBUG_SCOPE(ASSERT(p_vec->type == vec_type, "you cannot move Vec to child that is not a Vec"));
            DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_vec->type));
            Vec* p_next = (Vec*)(p_vec->p_data + index * element_size);
            DEBUG_SCOPE(ASSERT(vec_IsValid_SafeRead(p_next), "p_next is not a valid Vec"));
            DEBUG_SCOPE(Type_Info next_type = type_GetTypeInfo_Safe(p_next->type));
            DEBUG_SCOPE(Type_Info given_type = type_GetTypeInfo_Safe(type));
            DEBUG_SCOPE(ASSERT(p_next->type == type, "wrong type: %s vs %s\n", next_type.name, given_type.name));
            DEBUG_SCOPE(vec_LockRead(p_next));
            *pp_vec = p_next;
        }
    }
    void vec_MoveToIndices(Vec** pp_vec, size_t indices_count, const int* p_indices) {
        DEBUG_ASSERT(pp_vec, "p_vec is NULL pointer\n");
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(*pp_vec), "*pp_vec is invalid\n");

        for (int i = 0; i < indices_count; ++i) {
            if (i < indices_count-1) {
                DEBUG_ASSERT(!(p_indices[i] >= 0 && p_indices[i+1] == -1), "positive number cannot come before -1. at index %d and %d\n", i, i+1);
            }
            int index = p_indices[i];
            DEBUG_ASSERT(-1 <= index && index < (int)(*pp_vec)->count, "index at index %d is %d and is out of bounds for count %d\n", i+1, index, (*pp_vec)->count);
            if (index == -1) {
                DEBUG_SCOPE(vec_MoveToIndex(pp_vec, index, vec_type));
            } else {
                DEBUG_SCOPE(vec_MoveToIndex(pp_vec, index, ((Vec*)(*pp_vec)->p_data)->type));
            }
        }
    }
    void vec_MoveToVaArgs(Vec** pp_vec, size_t n_args, ...) {
        DEBUG_ASSERT(pp_vec, "p_vec is NULL pointer\n");
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(*pp_vec), "*pp_vec is invalid\n");
        va_list args;
        va_start(args, n_args);
        int prev_index = -1;
        for (size_t i = 0; i < n_args; i++) {
            int index = va_arg(args, int);
            DEBUG_ASSERT(-1 <= index && index < (int)(*pp_vec)->count, "index at index %d is %d and is out of bounds for count %d\n", i+1, index, (*pp_vec)->count);
            if (prev_index != -1) {
                DEBUG_ASSERT(!(prev_index >= 0 && index == -1), "positive number cannot come before -1. at index %d and %d\n", i-1, i);
            }
            prev_index = index;
            if (index == -1) {
                DEBUG_SCOPE(vec_MoveToIndex(pp_vec, index, vec_type));
            } else {
                DEBUG_SCOPE(vec_MoveToIndex(pp_vec, index, ((Vec*)(*pp_vec)->p_data)->type));
            }
        }
        va_end(args);
    }
    void vec_MoveToAtPath(Vec** pp_vec, const char* path) {
        DEBUG_ASSERT(pp_vec, "p_vec is NULL pointer\n");
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(*pp_vec), "*pp_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, &indices_count));
        DEBUG_ASSERT(p_indices, "p_indices is NULL pointer\n");
        DEBUG_SCOPE(vec_MoveToIndices(pp_vec, indices_count, p_indices));
        free(p_indices);
    }
    void* vec_MoveToPathAndGetElement(Vec** pp_vec, const char* path, Type type) {
        DEBUG_ASSERT(pp_vec, "p_vec is NULL pointer\n");
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(*pp_vec), "*pp_vec is invalid\n");
        size_t indices_count = 0;
        DEBUG_SCOPE(int* p_indices = vec_Path_ToIndices(path, &indices_count));
        DEBUG_ASSERT(p_indices, "p_indices is NULL pointer\n");
        DEBUG_SCOPE(vec_MoveToIndices(pp_vec, indices_count-1, p_indices));
        void* p_element = vec_GetElement_UnsafeRead(*pp_vec, p_indices[indices_count-1], type);
        free(p_indices);
        return p_element;
    }

// ================================================================================================================================
// GetIndexOfVecWithType…_SafeRead Locking
// ================================================================================================================================
    int vec_GetVecWithTypeFromIndex_UnafeRead(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(p_vec->type == vec_type, "type of provided vec has to be vec_type");
        DEBUG_ASSERT(type_IsValid_Safe(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");

        DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
        DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_vec->type));
        int type_index = -1;
        for (unsigned int i = index; i < count; ++i) {
            Vec* element = (Vec*)(p_vec->p_data + i * element_size);
            DEBUG_SCOPE(vec_LockRead(element));
            if (element->type == type) {
                type_index = i;
                DEBUG_SCOPE(vec_UnlockRead(element));
                break; 
            }
            DEBUG_SCOPE(vec_UnlockRead(element));
        }
        return type_index;
    }
    int vec_GetVecWithType_UnsafeRead(Vec* p_vec, Type type) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(type_IsValid_Safe(type), "type is invalid");
        DEBUG_SCOPE(int return_index = vec_GetVecWithTypeFromIndex_UnafeRead(p_vec, type, 0));
        return return_index;
    }

// ================================================================================================================================
// UpsertIndexOfFirstVecWithType Locking
// ================================================================================================================================
    int vec_UpsertVecWithTypeFromIndex_UnsafeWrite(Vec* p_vec, Type type, int index) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid");
        DEBUG_ASSERT(type_IsValid_Safe(type), "type is invalid");
        DEBUG_ASSERT(index >= 0, "index is less than 0");

        DEBUG_SCOPE(int type_index = vec_GetVecWithTypeFromIndex_UnafeRead(p_vec, type, index));

        if (type_index == -1) {
            DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
            DEBUG_SCOPE(vec_SetCount_UnsafeWrite(p_vec, count + 1));
            DEBUG_SCOPE(Vec* new_element = (Vec*)((char*)p_vec->p_data + count * type_GetSize_Safe(p_vec->type)));
            DEBUG_SCOPE(vec_Initialize(new_element, p_vec, type));
            return count;
        }
        return type_index;
    }
    int vec_UpsertVecWithType_UnsafeWrite(Vec* p_vec, Type type) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "vec is invalid");
        DEBUG_ASSERT(type_IsValid_Safe(type), "type is invalid");
        DEBUG_SCOPE(int return_index = vec_UpsertVecWithTypeFromIndex_UnsafeWrite(p_vec, type, 0));
        return return_index;
    }

// ================================================================================================================================
// UpsertNullElement
// ================================================================================================================================
    int vec_UpsertNullElement_UnsafeWrite(Vec* p_vec, Type type) {
        DEBUG_SCOPE(DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "Vec is invalid"));
        DEBUG_SCOPE(Type_Info next_type = type_GetTypeInfo_Safe(p_vec->type));
        DEBUG_SCOPE(Type_Info given_type = type_GetTypeInfo_Safe(type));
        DEBUG_SCOPE(ASSERT(p_vec->type == type, "wrong type: %s vs %s\n", next_type.name, given_type.name));

        int index = -1;
        DEBUG_SCOPE(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
        DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_vec->type));
        for (int i = 0; i < count; ++i) {
            bool element_is_null = true;
            DEBUG_SCOPE(unsigned char* element_ptr = vec_GetElement_UnsafeRead(p_vec, i, p_vec->type));
            for (unsigned int j = 0; j < element_size; ++j) {
                if (*(element_ptr + j) != 0) {
                    element_is_null = false;
                    break;
                }
            }
            if (element_is_null) {
                index = i;
                break;
            }
        }

        // if not null element was found then the Vec has to increase in count
        if (index == -1) {
            DEBUG_SCOPE(vec_SetCount_UnsafeWrite(p_vec, count + 1));
            DEBUG_SCOPE(unsigned char* element_ptr = vec_GetElement_UnsafeRead(p_vec, count, p_vec->type));
            for (unsigned int i = 0; i < element_size; ++i) {
                ASSERT((*(element_ptr + i) == 0), "newly created element is not null");
            }
            index = count;
        }

        return index;
    }

// ================================================================================================================================
// Create Locking
// ================================================================================================================================

// ================================================================================================================================
// Get
// ================================================================================================================================
    unsigned char* vec_GetElement_UnsafeRead(Vec* p_vec, int index, Type type) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
        DEBUG_SCOPE(Type_Info next_type = type_GetTypeInfo_Safe(p_vec->type));
        DEBUG_SCOPE(Type_Info given_type = type_GetTypeInfo_Safe(type));
        DEBUG_SCOPE(ASSERT(p_vec->type == type, "wrong type: %s vs %s\n", next_type.name, given_type.name));
        ASSERT(p_vec->type == type, "wrong type");
        DEBUG_ASSERT(0 <= index && index < p_vec->count, "index(%d) is out of bounds(%d)", index, p_vec->count);
        DEBUG_SCOPE(unsigned char* ptr = p_vec->p_data + index * type_GetSize_Safe(p_vec->type));
        return ptr;
    }
    unsigned int vec_GetElementSize_UnsafeRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
        DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_vec->type));
    	return element_size;
    }
    Type vec_GetType_UnsafeRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	return p_vec->type;
    }
    unsigned int vec_GetCount_UnsafeRead(Vec* p_vec) {
        DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	return p_vec->count;
    }

// ================================================================================================================================
// Set
// ================================================================================================================================
    void vec_SetCount_UnsafeWrite(Vec* p_vec, unsigned int count) {
        printf("setting count\n");
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
    	if (p_vec->count < count) {
            DEBUG_SCOPE(unsigned int element_size = type_GetSize_Safe(p_vec->type));
    		if (count > p_vec->capacity) {
    			unsigned int new_capacity = 1;
    			while (count >= new_capacity) {
    				new_capacity*=2;
    			}
    			DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, new_capacity * element_size));
    			p_vec->capacity = new_capacity;
    		}
    		memset((unsigned char*)(p_vec->p_data) + p_vec->count * element_size, 0, (count - p_vec->count) * element_size);
    	}
    	p_vec->count = count;
    }
    void vec_SetCapacity_UnsafeWrite(Vec* p_vec, unsigned int capacity) {
    	DEBUG_ASSERT(vec_IsValid_UnsafeRead(p_vec), "p_vec is invalid\n");
        DEBUG_ASSERT(capacity >= p_vec->count, "capacity cannot be less than p_vec->count");
    	if (p_vec->capacity != capacity) {
    		DEBUG_SCOPE(p_vec->p_data = alloc(p_vec->p_data, capacity*type_GetSize_Safe(p_vec->type)));
    		p_vec->capacity = capacity;
    	}
    }

/*
void vec_CopyElement_SafeRead(
	Vec* p_vec, 
	unsigned int index, 
	void** const pp_dst_data, 
	unsigned int* p_dst_data_size) 
{
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
	SDL_LockRWLockForReading(p_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_vec->count <= p_vec->capacity, "Count is greater than capacity");
	DEBUG_ASSERT(p_vec->count > index, "Index out of bounds");
	DEBUG_ASSERT(pp_dst_data, "NULL pointer");
	if (*p_dst_data_size < p_vec->element_size || *pp_dst_data == NULL) {
		*p_dst_data_size = p_vec->element_size;
		DEBUG_SCOPE(*pp_dst_data = alloc(*pp_dst_data, p_vec->element_size));
	}
	if (p_vec->this_type == p_vec->type) {
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
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
	SDL_LockRWLockForReading(p_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_vec->count <= p_vec->capacity, "Count is greater than capacity");
	DEBUG_ASSERT(p_vec->count >= index*count, "Index out of bounds");
	DEBUG_ASSERT(pp_dst_data, "NULL pointer");
	if (*p_dst_data_size < count*p_vec->element_size || *pp_dst_data == NULL) {
		*p_dst_data_size = count*p_vec->element_size;
		DEBUG_SCOPE(*pp_dst_data = alloc(*pp_dst_data, count*p_vec->element_size));
	}
	if (p_vec->this_type == p_vec->type) {
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
void vec_SetCount_UnsafeWrite(
	Vec* p_vec, 
	unsigned int count) 
{
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
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
		if (p_vec->this_type == p_vec->type) {
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
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
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
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
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
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
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
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
	DEBUG_ASSERT(p_other_vec, "NULL pointer");
	DEBUG_ASSERT(p_other_vec->p_rwlock, "NULL pointer");
	SDL_LockRWLockForWriting(p_vec->p_rwlock);
	SDL_LockRWLockForReading(p_other_vec->p_rwlock);
	DEBUG_ASSERT(p_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_other_vec->p_data, "NULL pointer");
	DEBUG_ASSERT(p_vec->type == p_other_vec->type, "element types are not the same");
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
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
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
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
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
{}
void vec_InsertElements_SafeWrite(
	Vec* p_vec, 
	unsigned int index, 
	unsigned int count, 
	const void * const p_src_data) 
{}
void vec_RemoveElementBack_SafeWrite(
	Vec* p_vec)
{}
void vec_RemoveElementsBack_SafeWrite(
	Vec* p_vec, 
	unsigned int count) 
{}
void vec_RemoveElement_SafeWrite(
	Vec* p_vec, 
	unsigned int index)
{}
void vec_RemoveElements_SafeWrite(
	Vec* p_vec, 
	unsigned int index, 
	unsigned int count)
{}
void vec_Destroy_SafeWrite(
	Vec* p_vec)
{
	DEBUG_ASSERT(vec_IsValid_SafeRead(p_vec), "p_vec is invalid\n");
	SDL_DestroyRWLock(p_vec->p_rwlock);
}
*/