#include "vec.h"
#include "vec_path.h"
#include "type.h"

#include <ctype.h>
#include <stdlib.h>
#include <stdarg.h>
#include <assert.h>

#include "tklog.h"

Type vec_type = 0;

// ================================================================================================================================
// Fundamental
// ================================================================================================================================
    __attribute__((constructor(103)))
    void _vec_Constructor() {
        vec_type = type_Create_Safe("Vec", sizeof(Vec), vec_Destroy);
    }
    void vec_Initialize(Vec* p_vec, Vec* p_parent, Type type) {
        if (!p_vec) { tklog_error("NULL pointer"); exit(-1); }
        if (!vec_IsNull_UnsafeRead(p_vec)) { tklog_error("vec should be Null before initializing it"); exit(-1); }
        if (!type_IsValid_Safe(type)) { tklog_error("type is invlaid"); exit(-1); }
        tklog_scope(Type_Info type_info = type_GetTypeInfo_Safe(type));
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
        if (!vec_IsValid_SafeRead(p_vec)) { tklog_error("newly created vec is invalid"); exit(-1); }
        printf("initialized new vec %p\n", p_vec);
    }
    Vec vec_Create(Vec* p_parent, Type type) {
        Vec vec = {0};
        tklog_scope(vec_Initialize(&vec, p_parent, type));
        return vec;
    }
    void vec_Destroy(void* p_vec) {
        Vec* p_vec_cast = (Vec*)p_vec;
        if (!vec_IsValid_SafeRead(p_vec_cast)) { tklog_error("Vec is invalid"); exit(-1); }
        tklog_scope(vec_LockWrite(p_vec_cast));
        tklog_scope(unsigned int count = vec_GetCount_UnsafeRead(p_vec_cast));
        tklog_scope(Type_Info type_info = type_GetTypeInfo_Safe(p_vec_cast->type));
        tklog_scope(Type_Destructor type_destructor = type_info.destructor);
        for (unsigned int i = 0; i < count; ++i) {
            type_destructor(vec_GetElement_UnsafeRead(p_vec_cast, i, p_vec_cast->type));
        }
        tklog_scope(vec_UnlockWrite(p_vec_cast));
        tklog_scope(SDL_DestroyMutex(p_vec_cast->p_internal_lock));
        tklog_scope(SDL_DestroyMutex(p_vec_cast->p_read_lock));
        tklog_scope(SDL_DestroyRWLock(p_vec_cast->p_rw_lock));
        memset(p_vec_cast, 0, sizeof(Vec));
    }

// ================================================================================================================================
// Debugging
// ================================================================================================================================
    bool vec_IsNull_UnsafeRead(Vec* p_vec) {
        if (!p_vec) { tklog_error("NULL pointer"); exit(-1); }
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
        tklog_scope(if (!vec_IsValid_SafeRead(p_vec)) { tklog_error("vec invalid"); exit(-1); });
        if (indices_count == 0) {
            tklog_scope(bool is_null = vec_IsNull_UnsafeRead(p_vec));
            return is_null;
        }

        bool is_null = false;
        Vec* p_current = p_vec;
        tklog_scope(vec_LockRead(p_current));
        int i = 0;
        for (; i < indices_count; ++i) {
            tklog_scope(unsigned char* p_tmp = (unsigned char*)vec_GetElement_UnsafeRead(p_current, p_indices[i], p_current->type));
            if (p_current->type != vec_type) {
                if (i < indices_count-1) {
                    is_null = true; // came to element which isnt vec type when not finished with p_indices
                } else {
                    bool tmp_is_null = true;
                    tklog_scope(unsigned int element_size = type_GetSize_Safe(p_current->type));
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
            tklog_scope(bool tmp_is_null = vec_IsNull_UnsafeRead((Vec*)p_tmp));
            if (tmp_is_null) {
                is_null = true;
                break;
            }
            tklog_scope(if (!vec_IsValid_SafeRead((Vec*)p_tmp)) { tklog_error("vec should be valid since its not null"); exit(-1); });
            p_current = (Vec*)p_tmp;
            tklog_scope(vec_LockRead(p_current));
        }
        for (; i > 0; --i) {
            tklog_scope(vec_UnlockRead(p_current));
            p_current = p_current->p_parent;
            if (!p_current) { tklog_error("should not be NULL because this vec should have been used in the previous loop"); exit(-1); }
        }
        tklog_scope(vec_UnlockRead(p_current));

        return is_null;
    }
    bool vec_IsNullAtVaArgs_SafeRead(Vec* p_vec, size_t n_args, ...) {
        if (!p_vec) { tklog_error("NULL pointer"); exit(-1); }
        va_list args;
        va_start(args, n_args);
        tklog_scope(int* p_indices = malloc(n_args * sizeof(int)));
        for (int i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        tklog_scope(bool is_null = vec_IsNullAtIndices_SafeRead(p_vec, n_args, p_indices));
        free(p_indices);
        return is_null;
    }
    void vec_Print_UnsafeRead(Vec* p_vec, unsigned int n_layers) {
        if (!vec_IsValid_SafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        tklog_scope(vec_LockRead(p_vec));

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
                tklog_scope(unsigned int element_size = type_GetSize_Safe(p_vec->type));
                printf("    elements:\n");
                if (p_vec->type == vec_type) {
                    for (unsigned int i = 0; i < p_vec->count; ++i) {
                        tklog_scope(bool is_valid = vec_IsValid_SafeRead((Vec*)(p_vec->p_data + i*element_size)));
                        if (!is_valid) {
                            continue;
                        }
                        printf("    %d %p:\n", i, p_vec->p_data + i*element_size);
                        tklog_scope(vec_Print_UnsafeRead((Vec*)(p_vec->p_data + i*element_size), n_layers-1));
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
        tklog_scope(vec_UnlockRead(p_vec));
    }
    bool vec_MatchElement_SafeRead(
        Vec* p_vec, 
        unsigned char* p_data, 
        size_t data_size, 
        int** const pp_return_indices, 
        size_t* const p_return_indices_count) 
    {
        if (!p_vec) { tklog_error("NULL pointer"); exit(-1); }
        if (!p_data) { tklog_error("NULL pointer"); exit(-1); }
        if (!pp_return_indices) { tklog_error("NULL pointer"); exit(-1); }
        if (*pp_return_indices) { tklog_error("not NULL pointer"); exit(-1); }
        if (!p_return_indices_count) { tklog_error("NULL pointer"); exit(-1); }

        tklog_scope(Vec** pp_vec = vec_MoveStart(p_vec));

        bool found_match = false;

        do {
            tklog_scope(Type type = vec_GetType_UnsafeRead(*pp_vec));
            tklog_scope(Type_Size element_size = type_GetSize_Safe(type));
            tklog_scope(unsigned int count = vec_GetCount_UnsafeRead(*pp_vec));

            for (unsigned int i = 0; i < count; ++i) {
                tklog_scope(unsigned char* p_element = (unsigned char*)vec_GetElement_UnsafeRead(*pp_vec, i, (*pp_vec)->type));
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
                        if (!p_vec_tmp->p_parent) { tklog_error("p_parent is NULL"); exit(-1); }
                        p_vec_tmp = p_vec_tmp->p_parent;
                        depth++;
                    }
                    tklog_scope(*pp_return_indices = realloc(*pp_return_indices, sizeof(int) * depth));
                    for (int j = depth-2; j >= 0; --j) {
                        p_vec_tmp = *pp_vec;
                        printf("move backwards\n");
                        vec_MoveToIndex(pp_vec, -1, vec_type);
                        int index = -1;
                        tklog_scope(count = vec_GetCount_UnsafeRead(*pp_vec));
                        for (unsigned int h = 0; h < count; ++h) {
                            tklog_scope(Vec* p_vec_tmp_2 = (Vec*)vec_GetElement_UnsafeRead(*pp_vec, h, (*pp_vec)->type));
                            if (p_vec_tmp_2 == p_vec_tmp) {
                                index = h;
                                break;
                            }
                        }
                        if (index == -1) { tklog_error("didnt find index when recursively going backwards"); exit(-1); }
                        (*pp_return_indices)[j] = index;
                    }
                    if (*pp_vec != p_vec) { tklog_error("didnt backpropagate to correct Vec*"); exit(-1); }
                    (*pp_return_indices)[depth-1] = i;
                    *p_return_indices_count = depth;
                    found_match = true;
                    break;
                }
            } 
            if (!found_match) {
                if (type == vec_type) {
                    printf("new child = %d\n", 0);
                    tklog_scope(vec_MoveToIndex(pp_vec, 0, ((Vec*)(*pp_vec)->p_data)->type));
                } else {
                    while (true) {
                        printf("one iteration\n");
                        Vec* p_vec_tmp = *pp_vec;
                        tklog_scope(vec_MoveToIndex(pp_vec, -1, vec_type));
                        tklog_scope(unsigned int count = vec_GetCount_UnsafeRead(*pp_vec));
                        int index = -1;
                        for (int i = 0; i < count-1; ++i) {
                            tklog_scope(Vec* p_vec_tmp_2 = (Vec*)vec_GetElement_UnsafeRead(*pp_vec, i, (*pp_vec)->type));
                            if (p_vec_tmp_2 == p_vec_tmp) {
                                index = i;
                                printf("new child = %d\n", i+1);
                                tklog_scope(vec_MoveToIndex(pp_vec, i+1, ((Vec*)(*pp_vec)->p_data)[(i+1)].type));
                                break;
                            }
                        }
                        if (index != -1) break;
                        if (*pp_vec == p_vec) break;
                    }
                }
            }

        } while (*pp_vec != p_vec);

        tklog_scope(vec_MoveEnd(pp_vec));

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
            tklog_scope(*pp_return_indices = realloc(*pp_return_indices, 0));
            *p_return_indices_count = 0;
        }

        if (found_match) {
            printf("path to match: ");
            for (unsigned int i = 0; i < *p_return_indices_count; ++i) {
                printf("%d ", (*pp_return_indices)[i]);
            }
            printf("\n");
        } else {
            tklog_scope(free(*pp_return_indices));
        }
        return found_match;
    }

// ================================================================================================================================
// Custom locking
// ================================================================================================================================
    void vec_LockRead(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }

        SDL_LockMutex(p_vec->p_read_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        if (p_vec->writing_locks != 0) { tklog_error("p_vec = %p | writing_locks is not 0. That should not be possible at this line", p_vec); exit(-1); }
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_LockRWLockForReading(p_vec->p_rw_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        p_vec->reading_count++;
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_UnlockMutex(p_vec->p_read_lock);
    }
    void vec_LockWrite(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }

        SDL_LockMutex(p_vec->p_read_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        if (p_vec->reading_count != 0) { tklog_error("p_vec = %p | reading_count is greater than 0. That should not be possible at this line", p_vec); exit(-1); }
        if (p_vec->writing_locks != 0) { tklog_error("p_vec = %p | writing_locks is not 1. That should not be possible at this line", p_vec); exit(-1); }
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_LockRWLockForWriting(p_vec->p_rw_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        p_vec->writing_locks++;
        SDL_UnlockMutex(p_vec->p_internal_lock);
    }
    void vec_UnlockRead(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }

        SDL_LockMutex(p_vec->p_internal_lock);
        if (p_vec->reading_count < 1) { tklog_error("p_vec = %p | youre trying to unlock read vec when there is no registered reading\n", p_vec); exit(-1); }
        if (p_vec->writing_locks != 0) { tklog_error("p_vec = %p | writing_locks is not 0. That should not be possible at this line", p_vec); exit(-1); }
        p_vec->reading_count--;
        SDL_UnlockMutex(p_vec->p_internal_lock);
        SDL_UnlockRWLock(p_vec->p_rw_lock);
    }
    void vec_UnlockWrite(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }

        SDL_LockMutex(p_vec->p_internal_lock);
        if (p_vec->reading_count != 0) { tklog_error("p_vec = %p | reading_locks is greater than 0. That should not be possible at this line", p_vec); exit(-1); }
        if (p_vec->writing_locks != 1) { tklog_error("p_vec = %p | writing_locks is not 1. That should not be possible at this line", p_vec); exit(-1); }
        p_vec->writing_locks--;
        SDL_UnlockRWLock(p_vec->p_rw_lock);
        SDL_UnlockMutex(p_vec->p_read_lock);
        SDL_UnlockMutex(p_vec->p_internal_lock);
    }
    void vec_SwitchReadToWrite(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }
        SDL_LockMutex(p_vec->p_read_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        if (p_vec->reading_count != 1) { tklog_error("p_vec = %p | reading_count(%d) is not 1 when switching from lock read to lock write. That should not happen here", p_vec, p_vec->reading_count); exit(-1); }
        if (p_vec->writing_locks != 0) { tklog_error("p_vec = %p | writing_locks(%d) is not 0 when switching from lock read to lock write. That should not happen here", p_vec, p_vec->reading_count); exit(-1); }
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_UnlockRWLock(p_vec->p_rw_lock);
        SDL_LockRWLockForWriting(p_vec->p_rw_lock);

        SDL_LockMutex(p_vec->p_internal_lock);
        p_vec->reading_count--;
        p_vec->writing_locks++;
        SDL_UnlockMutex(p_vec->p_internal_lock);
    }
    void vec_SwitchWriteToRead(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }

        SDL_LockMutex(p_vec->p_internal_lock);
        if (p_vec->reading_count != 0) { tklog_error("p_vec = %p | reading_count(%d) is not 0 when switching from lock write to lock read. That should not happen here", p_vec, p_vec->reading_count); exit(-1); }
        if (p_vec->writing_locks != 1) { tklog_error("p_vec = %p | writing_locks(%d) is not 1 when switching from lock write to lock read. That should not happen here", p_vec, p_vec->reading_count); exit(-1); }
        p_vec->reading_count++;
        p_vec->writing_locks--;
        SDL_UnlockMutex(p_vec->p_internal_lock);

        SDL_UnlockRWLock(p_vec->p_rw_lock);
        SDL_LockRWLockForReading(p_vec->p_rw_lock);
        SDL_UnlockMutex(p_vec->p_read_lock);
    }
    unsigned short vec_GetReadingLocksCount(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }
        SDL_LockMutex(p_vec->p_internal_lock);
        unsigned short reading_count = p_vec->reading_count;
        SDL_UnlockMutex(p_vec->p_internal_lock);
        return reading_count;
    }
    bool vec_IsWriteLocked(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec = %p is invalid\n", p_vec); exit(-1); }
        SDL_LockMutex(p_vec->p_internal_lock);
        if (p_vec->writing_locks > 1) { tklog_error("p_vec = %p | writing count is greater than 1"); exit(-1); }
        bool is_write_locked = p_vec->writing_locks == 1;
        SDL_UnlockMutex(p_vec->p_internal_lock);
        return is_write_locked;
    }

// ================================================================================================================================
// IsValid
// ================================================================================================================================
    bool vec_IsValid_SafeRead(Vec* p_vec) {
        if (!p_vec) { tklog_error("NULL pointer"); exit(-1); }
        if (p_vec->p_read_lock == NULL) {printf("p_vec->p_read_lock == NULL. %p\n", p_vec); return false;}
        if (p_vec->p_rw_lock == NULL) {printf("p_vec->p_rw_lock == NULL. %p\n", p_vec); return false;}
        if (p_vec->p_internal_lock == NULL) {printf("p_vec->p_internal_lock == NULL. %p\n", p_vec); return false;}
        tklog_scope(vec_LockRead(p_vec));
        if (p_vec->type == null_type) {printf("p_vec->type == 0. %p\n", p_vec); return false;}
        if (p_vec->count > p_vec->capacity) {printf("p_vec->count > p_vec->capacity. %p\n", p_vec); return false;}
        tklog_scope(vec_UnlockRead(p_vec));
        return true;
    }    
    bool vec_IsValid_UnsafeRead(Vec* p_vec) {
        if (!p_vec) { tklog_error("NULL pointer"); exit(-1); }
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
        tklog_scope(bool is_valid = vec_IsValid_SafeRead(p_vec));
        if (!is_valid) {
            if (!vec_IsNull_UnsafeRead(p_vec)) { tklog_error("if vec is not valid then it should be null"); exit(-1); }
            return false;
        }

        bool is_null = false;
        Vec* p_current = p_vec;
        tklog_scope(vec_LockRead(p_current));
        int i = 0;
        for (; i < indices_count; ++i) {
            tklog_scope(unsigned char* p_tmp = (unsigned char*)vec_GetElement_UnsafeRead(p_current, p_indices[i], p_current->type));
            if (p_current->type != vec_type) {
                if (i < indices_count-1) {
                    printf("element is not vec type while also not the last depth which is %d\n", i+1);
                    is_null = true; // came to element which isnt vec type when not finished with p_indices
                } else {
                    bool tmp_is_null = true;
                    unsigned int element_size = type_GetSize_Safe(p_current->type);
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
            tklog_scope(bool tmp_is_valid = vec_IsValid_SafeRead((Vec*)p_tmp));
            if (!tmp_is_valid) {
                if (!vec_IsNull_UnsafeRead((Vec*)p_tmp)) { tklog_error("vec should be null if its not valid"); exit(-1); }
                printf("vec at depth %d is invalid\n", i+1);
                is_null = true;
                is_valid = false; 
                break;
            }
            p_current = (Vec*)p_tmp;
            tklog_scope(vec_LockRead(p_current));
        }
        for (; i > 0; --i) {
            tklog_scope(vec_UnlockRead(p_current));
            p_current = p_current->p_parent;
            if (!p_current) { tklog_error("should not be NULL because this vec should have been used in the previous loop"); exit(-1); }
        }
        tklog_scope(vec_UnlockRead(p_current));
        if (is_null) {
            is_valid = false;
        }

        return is_valid;
    }
    bool vec_IsValidAtVaArgs_SafeRead(Vec* p_vec, Type type, size_t n_args, ...) {
        if (!vec_IsValid_SafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        va_list args;
        va_start(args, n_args);
        tklog_scope(int* p_indices = malloc(n_args * sizeof(int)));
        for (size_t i = 0; i < n_args; i++) {
            p_indices[i] = va_arg(args, int);
        }
        va_end(args);
        tklog_scope(bool return_bool = vec_IsValidAtIndices_SafeRead(p_vec, type, n_args, p_indices));
        tklog_scope(free(p_indices));
        return return_bool;
    }
    bool vec_IsValidAtPath_SafeRead(Vec* p_vec,  Type type, const char* path) {
        if (!vec_IsValid_SafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        size_t indices_count = 0;
        tklog_scope(int* p_indices = vec_Path_ToIndices(path, &indices_count));
        if (!p_indices) { tklog_error("Failed to get p_indices from path\n"); exit(-1); }
        tklog_scope(bool return_bool = vec_IsValidAtIndices_SafeRead(p_vec, type, indices_count, p_indices));
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
        tklog_scope(if (!vec_IsValid_SafeRead(p_vec)) { tklog_error("vec is invalid"); exit(-1); });
        tklog_scope(if (p_vec->p_parent) { tklog_error("p_vec is not ground Vec because p_parent is not NULL"); exit(-1); });
        tklog_scope(vec_LockRead(p_vec));
        tklog_scope(Vec** pp_vec = malloc(sizeof(Vec*)));
        *pp_vec = p_vec;
        return pp_vec;
    }
    void vec_MoveEnd(Vec** pp_vec) {
        tklog_scope(if (!pp_vec) { tklog_error("pp_vec is NULL"); exit(-1); });
        tklog_scope(if (!vec_IsValid_UnsafeRead(*pp_vec)) { tklog_error("*pp_vec is invalid"); exit(-1); });
        Vec* p_current = *pp_vec;
        tklog_scope(vec_UnlockRead(p_current));
        while (p_current->p_parent) {
            p_current = p_current->p_parent;
            tklog_scope(if (!vec_IsValid_UnsafeRead(p_current)) { tklog_error("p_vec %p | is invlaid", p_current); exit(-1); });
            tklog_scope(vec_UnlockRead(p_current));
        }
        tklog_scope(free(pp_vec));
    }
    void vec_MoveToIndex(Vec** pp_vec, int index, Type type) {
        if (!pp_vec) { tklog_error("pp_vec is null\n"); exit(-1); }
        if (!vec_IsValid_UnsafeRead(*pp_vec)) { tklog_error("*pp_vec is invalid\n"); exit(-1); }
        Vec* p_vec = *pp_vec;
        if (index < -1 || index >= (int)p_vec->count) { tklog_error("index(%d) is out of bounds(%d)", index, p_vec->count); exit(-1); }
        if (index == -1) {
            Vec* p_parent = p_vec->p_parent;
            tklog_scope(if (!vec_IsValid_UnsafeRead(p_parent)) { tklog_error("p_parent is not valid"); exit(-1); });
            tklog_scope(vec_UnlockRead(p_vec));
            tklog_scope(if (type != vec_type) { tklog_error("wrong type. it has to be vec_type when going to p_parent"); exit(-1); });
            *pp_vec = p_parent;
        } else {
            tklog_scope(if (p_vec->type != vec_type) { tklog_error("you cannot move Vec to child that is not a Vec"); exit(-1); });
            tklog_scope(unsigned int element_size = type_GetSize_Safe(p_vec->type));
            Vec* p_next = (Vec*)(p_vec->p_data + index * element_size);
            tklog_scope(if (!vec_IsValid_SafeRead(p_next)) { tklog_error("p_next is not a valid Vec"); exit(-1); });
            tklog_scope(Type_Info next_type = type_GetTypeInfo_Safe(p_next->type));
            tklog_scope(Type_Info given_type = type_GetTypeInfo_Safe(type));
            tklog_scope(if (p_next->type != type) { tklog_error("wrong type: %s vs %s\n", next_type.name, given_type.name); exit(-1); });
            tklog_scope(vec_LockRead(p_next));
            *pp_vec = p_next;
        }
    }
    void vec_MoveToIndices(Vec** pp_vec, size_t indices_count, const int* p_indices) {
        if (!pp_vec) { tklog_error("p_vec is NULL pointer\n"); exit(-1); }
        if (!vec_IsValid_UnsafeRead(*pp_vec)) { tklog_error("*pp_vec is invalid\n"); exit(-1); }

        for (int i = 0; i < indices_count; ++i) {
            if (i < indices_count-1) {
                if (p_indices[i] >= 0 && p_indices[i+1] == -1) { tklog_error("positive number cannot come before -1. at index %d and %d\n", i, i+1); exit(-1); }
            }
            int index = p_indices[i];
            if (index < -1 || index >= (int)(*pp_vec)->count) { tklog_error("index at index %d is %d and is out of bounds for count %d\n", i+1, index, (*pp_vec)->count); exit(-1); }
            if (index == -1) {
                tklog_scope(vec_MoveToIndex(pp_vec, index, vec_type));
            } else {
                tklog_scope(vec_MoveToIndex(pp_vec, index, ((Vec*)(*pp_vec)->p_data)->type));
            }
        }
    }
    void vec_MoveToVaArgs(Vec** pp_vec, size_t n_args, ...) {
        if (!pp_vec) { tklog_error("p_vec is NULL pointer\n"); exit(-1); }
        if (!vec_IsValid_UnsafeRead(*pp_vec)) { tklog_error("*pp_vec is invalid\n"); exit(-1); }
        va_list args;
        va_start(args, n_args);
        int prev_index = -1;
        for (size_t i = 0; i < n_args; i++) {
            int index = va_arg(args, int);
            if (index < -1 || index >= (int)(*pp_vec)->count) { tklog_error("index at index %d is %d and is out of bounds for count %d\n", i+1, index, (*pp_vec)->count); exit(-1); }
            if (prev_index != -1) {
                if (prev_index >= 0 && index == -1) { tklog_error("positive number cannot come before -1. at index %d and %d\n", i-1, i); exit(-1); }
            }
            prev_index = index;
            if (index == -1) {
                tklog_scope(vec_MoveToIndex(pp_vec, index, vec_type));
            } else {
                tklog_scope(vec_MoveToIndex(pp_vec, index, ((Vec*)(*pp_vec)->p_data)->type));
            }
        }
        va_end(args);
    }
    void vec_MoveToAtPath(Vec** pp_vec, const char* path) {
        if (!pp_vec) { tklog_error("p_vec is NULL pointer\n"); exit(-1); }
        if (!vec_IsValid_UnsafeRead(*pp_vec)) { tklog_error("*pp_vec is invalid\n"); exit(-1); }
        size_t indices_count = 0;
        tklog_scope(int* p_indices = vec_Path_ToIndices(path, &indices_count));
        if (!p_indices) { tklog_error("p_indices is NULL pointer\n"); exit(-1); }
        tklog_scope(vec_MoveToIndices(pp_vec, indices_count, p_indices));
        free(p_indices);
    }
    void* vec_MoveToPathAndGetElement(Vec** pp_vec, const char* path, Type type) {
        if (!pp_vec) { tklog_error("p_vec is NULL pointer\n"); exit(-1); }
        if (!vec_IsValid_UnsafeRead(*pp_vec)) { tklog_error("*pp_vec is invalid\n"); exit(-1); }
        size_t indices_count = 0;
        tklog_scope(int* p_indices = vec_Path_ToIndices(path, &indices_count));
        if (!p_indices) { tklog_error("p_indices is NULL pointer\n"); exit(-1); }
        tklog_scope(vec_MoveToIndices(pp_vec, indices_count-1, p_indices));
        void* p_element = vec_GetElement_UnsafeRead(*pp_vec, p_indices[indices_count-1], type);
        free(p_indices);
        return p_element;
    }

// ================================================================================================================================
// GetIndexOfVecWithType…_SafeRead Locking
// ================================================================================================================================
    int vec_GetVecWithTypeFromIndex_UnafeRead(Vec* p_vec, Type type, int index) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid"); exit(-1); }
        if (p_vec->type != vec_type) { tklog_error("type of provided vec has to be vec_type"); exit(-1); }
        if (!type_IsValid_Safe(type)) { tklog_error("type is invalid"); exit(-1); }
        if (index < 0) { tklog_error("index is less than 0"); exit(-1); }

        tklog_scope(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
        tklog_scope(unsigned int element_size = type_GetSize_Safe(p_vec->type));
        int type_index = -1;
        for (unsigned int i = index; i < count; ++i) {
            Vec* element = (Vec*)(p_vec->p_data + i * element_size);
            tklog_scope(vec_LockRead(element));
            if (element->type == type) {
                type_index = i;
                tklog_scope(vec_UnlockRead(element));
                break; 
            }
            tklog_scope(vec_UnlockRead(element));
        }
        return type_index;
    }
    int vec_GetVecWithType_UnsafeRead(Vec* p_vec, Type type) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid"); exit(-1); }
        if (!type_IsValid_Safe(type)) { tklog_error("type is invalid"); exit(-1); }
        tklog_scope(int return_index = vec_GetVecWithTypeFromIndex_UnafeRead(p_vec, type, 0));
        return return_index;
    }

// ================================================================================================================================
// UpsertIndexOfFirstVecWithType Locking
// ================================================================================================================================
    int vec_UpsertVecWithTypeFromIndex_UnsafeWrite(Vec* p_vec, Type type, int index) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid"); exit(-1); }
        if (!type_IsValid_Safe(type)) { tklog_error("type is invalid"); exit(-1); }
        if (index < 0) { tklog_error("index is less than 0"); exit(-1); }

        tklog_scope(int type_index = vec_GetVecWithTypeFromIndex_UnafeRead(p_vec, type, index));

        if (type_index == -1) {
            tklog_scope(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
            tklog_scope(vec_SetCount_UnsafeWrite(p_vec, count + 1));
            tklog_scope(Vec* new_element = (Vec*)((char*)p_vec->p_data + count * type_GetSize_Safe(p_vec->type)));
            tklog_scope(vec_Initialize(new_element, p_vec, type));
            return count;
        }
        return type_index;
    }
    int vec_UpsertVecWithType_UnsafeWrite(Vec* p_vec, Type type) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("vec is invalid"); exit(-1); }
        if (!type_IsValid_Safe(type)) { tklog_error("type is invalid"); exit(-1); }
        tklog_scope(int return_index = vec_UpsertVecWithTypeFromIndex_UnsafeWrite(p_vec, type, 0));
        return return_index;
    }

// ================================================================================================================================
// UpsertNullElement
// ================================================================================================================================
    int vec_UpsertNullElement_UnsafeWrite(Vec* p_vec, Type type) {
        tklog_scope(if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("Vec is invalid"); exit(-1); });
        tklog_scope(Type_Info next_type = type_GetTypeInfo_Safe(p_vec->type));
        tklog_scope(Type_Info given_type = type_GetTypeInfo_Safe(type));
        tklog_scope(if (p_vec->type != type) { tklog_error("wrong type: %s vs %s\n", next_type.name, given_type.name); exit(-1); });

        int index = -1;
        tklog_scope(unsigned int count = vec_GetCount_UnsafeRead(p_vec));
        tklog_scope(unsigned int element_size = type_GetSize_Safe(p_vec->type));
        for (int i = 0; i < count; ++i) {
            bool element_is_null = true;
            tklog_scope(unsigned char* element_ptr = vec_GetElement_UnsafeRead(p_vec, i, p_vec->type));
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
            tklog_scope(vec_SetCount_UnsafeWrite(p_vec, count + 1));
            tklog_scope(unsigned char* element_ptr = vec_GetElement_UnsafeRead(p_vec, count, p_vec->type));
            for (unsigned int i = 0; i < element_size; ++i) {
                if (*(element_ptr + i) != 0) { tklog_error("newly created element is not null"); exit(-1); }
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
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        tklog_scope(Type_Info next_type = type_GetTypeInfo_Safe(p_vec->type));
        tklog_scope(Type_Info given_type = type_GetTypeInfo_Safe(type));
        tklog_scope(if (p_vec->type != type) { tklog_error("wrong type: %s vs %s\n", next_type.name, given_type.name); exit(-1); });
        if (p_vec->type != type) { tklog_error("wrong type"); exit(-1); }
        if (index < 0 || index >= p_vec->count) { tklog_error("index(%d) is out of bounds(%d)", index, p_vec->count); exit(-1); }
        tklog_scope(unsigned char* ptr = p_vec->p_data + index * type_GetSize_Safe(p_vec->type));
        return ptr;
    }
    unsigned int vec_GetElementSize_UnsafeRead(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        tklog_scope(unsigned int element_size = type_GetSize_Safe(p_vec->type));
        return element_size;
    }
    Type vec_GetType_UnsafeRead(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        return p_vec->type;
    }
    unsigned int vec_GetCount_UnsafeRead(Vec* p_vec) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        return p_vec->count;
    }

// ================================================================================================================================
// Set
// ================================================================================================================================
    void vec_SetCount_UnsafeWrite(Vec* p_vec, unsigned int count) {
        printf("setting count\n");
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        if (p_vec->count < count) {
            tklog_scope(unsigned int element_size = type_GetSize_Safe(p_vec->type));
            if (count > p_vec->capacity) {
                unsigned int new_capacity = 1;
                while (count >= new_capacity) {
                    new_capacity*=2;
                }
                tklog_scope(p_vec->p_data = realloc(p_vec->p_data, new_capacity * element_size));
                p_vec->capacity = new_capacity;
            }
            memset((unsigned char*)(p_vec->p_data) + p_vec->count * element_size, 0, (count - p_vec->count) * element_size);
        }
        p_vec->count = count;
    }
    void vec_SetCapacity_UnsafeWrite(Vec* p_vec, unsigned int capacity) {
        if (!vec_IsValid_UnsafeRead(p_vec)) { tklog_error("p_vec is invalid\n"); exit(-1); }
        if (capacity < p_vec->count) { tklog_error("capacity cannot be less than p_vec->count"); exit(-1); }
        if (p_vec->capacity != capacity) {
            tklog_scope(p_vec->p_data = realloc(p_vec->p_data, capacity*type_GetSize_Safe(p_vec->type)));
            p_vec->capacity = capacity;
        }
    }