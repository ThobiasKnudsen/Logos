#ifndef CPI_ARRAY_H
#define CPI_ARRAY_H

#include "type.h"
#include <stdbool.h>
#include <SDL3/SDL.h>
#include <stdarg.h>

// 48 bytes
typedef struct Vec Vec;
struct Vec {
#ifdef DEBUG 
	SDL_Mutex*  	p_debug_mutex;
#endif 
	SDL_Mutex* 		p_write_mutex;
	SDL_Mutex* 		p_read_mutex;
	Vec*   			p_parent;
	unsigned char* 	p_data;
	unsigned short  reading_count;
	unsigned short 	element_size;
	unsigned short	element_type;
	unsigned short  this_type;
	unsigned int  	count;
	unsigned int  	capacity;
}; 

typedef struct Vec_CreateInfo {
	Vec* 			p_parent;
	unsigned short 	element_size;
	unsigned short	element_type;
	unsigned short  this_type;
} Vec_CreateInfo;

extern Type vec_type;

// ================================================================================================================================
// Fundamental 
// ================================================================================================================================
void  				vec_Initialize(
						Vec* p_vec,
						Vec_CreateInfo* p_vec_create_info);
Vec 				vec_Create(
						Vec* p_parent,
						Type type);
void 				vec_Destroy(
						void* p_vec);

// ================================================================================================================================
// Debugging
// ================================================================================================================================
bool 				vec_IsNull_UnsafeRead(
						Vec* p_vec);
bool 				vec_IsNullAtVaArgs_SafeRead(
						Vec* p_vec,
						size_t n_args, 
						...);
void 				vec_Print_UnsafeRead(
						Vec* p_vec, 
						const unsigned int n_spaces, 
						const unsigned int n_layers);
unsigned int   		vec_GetReadingLocksCount(
						Vec* p_vec);
bool 				vec_TryGetReadingLocksCount(
						Vec* p_vec,
						unsigned int* const p_count);
bool   				vec_IsWriteLocked(
						Vec* p_vec);

// ================================================================================================================================
// Custom concurrency
// ================================================================================================================================
void    			vec_LockRead(
						Vec* p_vec);
void    			vec_LockWrite(
						Vec* p_vec);
void    			vec_UnlockRead(
						Vec* p_vec);
void    			vec_UnlockWrite(
						Vec* p_vec);

// ================================================================================================================================
// IsValid Locking
// ================================================================================================================================
bool 				vec_IsValid_UnsafeRead(
						Vec* p_vec);
bool 				vec_IsValidAtVaArgs_SafeRead(
						Vec* p_root_vec, 
						Type type, 
						size_t n_args, 
						...);
bool 				vec_IsValidAtPath_SafeRead(
						Vec* p_root_vec,  
						Type type, 
						const char* path);

// ================================================================================================================================
// Get Locking
// ================================================================================================================================
void* 				vec_GetAtIndices_LockRead(
						Vec* p_root_vec,
						size_t indices_count, 
						const int* p_indices);
void* 				vec_GetAtIndices_LockWrite(
						Vec* p_root_vec,
						size_t indices_count, 
						const int* p_indices);
void* 				vec_GetAtVaArgs_LockRead(
						Vec* p_root_vec,	
						size_t n_args, 
						...);
void* 				vec_GetAtVaArgs_LockWrite(
						Vec* p_root_vec,
						size_t n_args, 
						...);
void* 				vec_GetAtPath_LockRead(
						Vec* p_root_vec,
						const char* path);
void* 				vec_GetAtPath_LockWrite(
						Vec* p_root_vec,
						const char* path);

// ================================================================================================================================
// UpsertIndexOfNullElement Locking
// ================================================================================================================================
int 				vec_UpsertIndexOfNullElement_SafeWrite(
						Vec* p_vec);
        
// ================================================================================================================================
// GetIndexOfFirstVecWithType Locking
// ================================================================================================================================
int 				vec_GetIndexOfFirstVecWithType_SafeRead(
						Vec* p_vec, 
						Type type);
int 				vec_GetIndexOfFirstVecWithTypeFromIndex_SafeRead(
						Vec* p_vec, 
						Type type, 
						int index);

// ================================================================================================================================
// UpsertIndexOfFirstVecWithType Locking
// ================================================================================================================================
int 				vec_UpsertIndexOfFirstVecWithType_SafeWrite(
						Vec* p_vec, 
						Type type);
int 				vec_UpsertIndexOfFirstVecWithTypeFromIndex_SafeWrite(
						Vec* p_vec, 
						Type type, 
						int index);

// ================================================================================================================================
// UpsertVecWithType Lock
// ================================================================================================================================
void*  				vec_UpsertVecWithType_LockRead(
						Vec* p_vec, 
						Type type);

// ================================================================================================================================
// UpsertNullElement Safe
// ================================================================================================================================
void* 				vec_UpsertNullElement_LockRead(
						Vec* p_vec);
void* 				vec_UpsertNullElement_LockWrite(
						Vec* p_vec);

// ================================================================================================================================
// UpsertFirstVecWithType Locking
// ================================================================================================================================
Vec* 				vec_UpsertFirstVecWithType_LockRead(
						Vec* p_vec, 
						Type type);
Vec* 				vec_UpsertFirstVecWithType_LockWrite(
						Vec* p_vec, 
						Type type);
Vec* 				vec_UpsertFirstVecWithTypeFromIndex_LockRead(
						Vec* p_vec, 
						Type type, 
						int index);
Vec* 				vec_UpsertFirstVecWithTypeFromIndex_LockWrite(
						Vec* p_vec, 
						Type type, 
						int index);

// ================================================================================================================================
// Create Locking
// ================================================================================================================================
void* 				vec_CreateAtVaArgs_LockRead(
						Vec* p_root_vec,
						Type type, 
						size_t n_args, 
						...);
void* 				vec_CreateAtVaArgs_LockWrite(
						Vec* p_root_vec,
						Type type,
						size_t n_args, 
						...);

// ================================================================================================================================
// Get Unlocking
// ================================================================================================================================
void* 				vec_GetAtIndices_UnlockRead(
						Vec* p_root_vec,
						size_t indices_count, 
						const int* p_indices);
void* 				vec_GetAtIndices_UnlockWrite(
						Vec* p_root_vec,
						size_t indices_count, 
						const int* p_indices);
void* 				vec_GetAtVaArgs_UnlockRead(
						Vec* p_root_vec,
						size_t n_args, 
						...);
void* 				vec_GetAtVaArgs_UnlockWrite(
						Vec* p_root_vec,
						size_t n_args, 
						...);
void* 				vec_GetAtPath_UnlockRead(
						Vec* p_root_vec,
						const char* path);
void* 				vec_GetAtPath_UnlockWrite(
						Vec* p_root_vec,
						const char* path);

// ================================================================================================================================
// Get Unsafe
// ================================================================================================================================
void*   			vec_GetAt_Unsafe(
						Vec* p_vec,
						int index);
unsigned int 		vec_GetElementSize_UnsafeRead(
						Vec* p_vec);
unsigned short 		vec_GetElementType_UnsafeRead(
						Vec* p_vec);
unsigned short 		vec_GetThisType_UnsafeRead(
						Vec* p_vec);
unsigned int		vec_GetCount_UnsafeRead(
						Vec* p_vec);
unsigned int		vec_GetCapacity_UnsafeRead(
						Vec* p_vec);

// ================================================================================================================================
// Set Unsafe
// ================================================================================================================================
void  				vec_SetElementSize_UnsafeWrite(
						Vec* p_vec, 
						unsigned int size);
void  				vec_SetElementType_Unsafe(
						Vec* p_vec, 
						unsigned short type);
void  				vec_SetThisType_UnsafeWrite(
						Vec* p_vec, 
						unsigned short type);
void  				vec_SetCount_UnsafeWrite(
						Vec* p_vec, 
						unsigned int count);
void  				vec_SetCapacity_UnsafeWrite(
						Vec* p_vec, 
						unsigned int capacity);


#endif // CPI_LIST_H