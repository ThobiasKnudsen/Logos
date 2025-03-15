#ifndef CPI_ARRAY_H
#define CPI_ARRAY_H

#include "type.h"
#include <stdbool.h>
#include <stdarg.h>
#include <SDL3/SDL.h>


// ================================================================================================================================
// Whenever you have a Vec* this class assumes that all parent Vecs are read locked
// ================================================================================================================================

// 64 bytes
typedef struct Vec Vec;
struct Vec {
	SDL_Mutex*  		p_internal_lock;
	SDL_RWLock* 		p_rw_lock;
	SDL_Mutex* 			p_read_lock;
	Vec*   				p_parent;
	unsigned char* 		p_data;
	unsigned char  		reading_count;
	unsigned char  		writing_locks;
	Type				type;
	unsigned int  		count;
	unsigned int  		capacity;
}; 

extern Type vec_type;

// ================================================================================================================================
// Fundamental 
// ================================================================================================================================
void 				vec_Initialize(
						Vec* p_vec, 
						Vec* p_parent, 
						Type type);
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
						unsigned int n_layers);
bool 				vec_MatchElement_SafeRead(
        				Vec* p_vec, 
        				unsigned char* p_data, 
        				size_t data_size, 
        				int** const pp_return_indices, 
        				size_t* const p_return_indices_count);

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
void 				vec_SwitchReadToWrite(
						Vec* p_vec);
void 				vec_SwitchWriteToRead(
						Vec* p_vec);
unsigned short   	vec_GetReadingLocksCount(
						Vec* p_vec);
bool   				vec_IsWriteLocked(
						Vec* p_vec);

// ================================================================================================================================
// IsValid
// ================================================================================================================================
bool 				vec_IsValid_SafeRead(
						Vec* p_vec);
bool 				vec_IsValid_UnsafeRead(
						Vec* p_vec);
bool 				vec_IsValidAtVaArgs_SafeRead(
						Vec* p_vec, 
						Type type, 
						size_t n_args, 
						...);
bool 				vec_IsValidAtPath_SafeRead(
						Vec* p_vec,  
						Type type, 
						const char* path);

// ================================================================================================================================
// MoveTo
//
// These functions traverse a hierarchy of Vec objects with thread-safe read locks.
// When moving forward (parent to child), each intermediate Vec is locked, but the final Vec remains unlocked, letting you decide its lock state.
// When moving backward (child to parent), they unlock the parent Vecs as you go, so by the time you return to the starting Vec, all locks are released.
// For simplicity and safety, use only one Vec pointer per thread to avoid complex lock management.
// This also enforce index sequence rules: a positive index moves to a child, a negative index to a parent, and a positive index cannot immediately precede a negative one. 
// ================================================================================================================================
Vec** 				vec_MoveStart(
						Vec* p_vec);
void 				vec_MoveEnd(
						Vec** pp_vec);
void 				vec_MoveToIndex(
						Vec** pp_vec,
						int index,
						Type type);
void 				vec_MoveToIndices(
						Vec** pp_vec,
						size_t indices_count, 
						const int* p_indices);
void 				vec_MoveToVaArgs(
						Vec** pp_vec,
						size_t n_args, 
						...);
void 				vec_MoveToPath(
						Vec** pp_vec,
						const char* path);
void* 				vec_MoveToPathAndGetElement(
						Vec** pp_vec,
						const char* path, 
						Type type);


// ================================================================================================================================
// UpsertVecWithType…_SafeWrite
// ================================================================================================================================
int  				vec_UpsertVecWithType_UnsafeWrite(
						Vec* p_vec,
						Type type);
int 				vec_UpsertVecWithTypeFromIndex_UnsafeWrite(
						Vec* p_vec, 
						Type type, 
						int index);

// ================================================================================================================================
// UpsertNullElement_SafeWrite
// ================================================================================================================================
int  				vec_UpsertNullElement_UnsafeWrite(
						Vec* p_vec,
						Type type);

// ================================================================================================================================
// Create Locking
// ================================================================================================================================
void				vec_CreateAtVaArgs_UnsafeWrite(
						Vec* p_vec,
						Type type, 
						size_t n_args, 
						...);
        
// ================================================================================================================================
// GetIndexVecWithType…_SafeRead
// ================================================================================================================================
int 				vec_GetVecWithType_UnsafeRead(
						Vec* p_vec, 
						Type type);
int 				vec_GetVecWithTypeFromIndex_UnsafeRead(
						Vec* p_vec, 
						Type type, 
						int index);

// ================================================================================================================================
// Get…Between_SafeRead
// ================================================================================================================================
int*				vec_GetIndicesBetween_SafeRead(
						Vec* p_vec_begin,
						Vec* p_vec_end);
char*				vec_GetPathBetween_SafeRead(
						Vec* p_vec_begin,
						Vec* p_vec_end);

// ================================================================================================================================
// Get…_UnsafeRead
// ================================================================================================================================
unsigned char* 		vec_GetElement_UnsafeRead(
						Vec* p_vec,
						int index,
						Type type);
unsigned int 		vec_GetElementSize_UnsafeRead(
						Vec* p_vec);
Type 				vec_GetType_UnsafeRead(
						Vec* p_vec);
unsigned int		vec_GetCount_UnsafeRead(
						Vec* p_vec);
unsigned int		vec_GetCapacity_UnsafeRead(
						Vec* p_vec);

// ================================================================================================================================
// Set…_SafeWrite
// ================================================================================================================================
void  				vec_SetCount_UnsafeWrite(
						Vec* p_vec, 
						unsigned int count);
void  				vec_SetCapacity_UnsafeWrite(
						Vec* p_vec, 
						unsigned int capacity);


#endif // CPI_LIST_H