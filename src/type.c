#include "type.h"
#include <SDL3/SDL.h>
#include <stdlib.h>
#include "tklog.h"

static Type_Info* 		p_types 			= NULL;
static size_t 			types_count  		= 0;
static size_t 			types_capacity  	= 0;
static unsigned int  	unique_type_count  	= 0;
static bool   			constructed  		= false;
static SDL_Mutex*  		p_mutex;
Type  					null_type;
Type  					type_type;


__attribute__((constructor(102)))
void _type_Constructor() {
	null_type = type_Create_Safe("NULL", 0, NULL);
	type_type = type_Create_Safe("Type", sizeof(Type), NULL);
	constructed = true;
}
Type type_Create_Safe(const char* type_name, Type_Size type_size, Type_Destructor destructor) {
	SDL_LockMutex(p_mutex);
	if (!(constructed ^ (types_count <= 1))) { 
		tklog_error("_type_Constructor is not run before running type_Create_Safe, constructed(%d) types_count(%d)", constructed, types_count);
		return null_type;
	}
	if (types_count > types_capacity) {
		tklog_error("types_count is somehow greater than types_capacity");
		return null_type;
	}
	Type new_type = unique_type_count++;
	if (types_count == types_capacity) {
		types_capacity = 1;
		while (types_capacity <= types_count) {
			types_capacity *= 2;
		}
		tklog_scope(p_types = realloc(p_types, types_capacity*sizeof(Type_Info)));
	}
	p_types[types_count].type = new_type;
	p_types[types_count].name = type_name;
	p_types[types_count].size = type_size;
	p_types[types_count].destructor = destructor;
	types_count++;
	tklog_info("Created type %s\n", type_name);
	if (types_count >= 65536) {
		tklog_critical("there are more than 65536 types. connot support more. types_count = %d\n", types_count);	
		return null_type;
	} 
	SDL_UnlockMutex(p_mutex);
	return new_type;
}
bool type_IsValid_Safe(Type type) {
	SDL_LockMutex(p_mutex);
	if (types_count >= 65536) {
		tklog_critical("there are more than 65536 types. connot support more. types_count = %d\n", types_count);
		return false;
	}
	if (type == null_type) {
		return false;
	}
	bool found_type = false;
	for (int i = 0; i < types_count; ++i) {
		if (p_types[i].type == type) {
			if (found_type) {
				tklog_critical("allready found type index. there cant be multiples of the same type");
				return false;
			}
			found_type = true;
		}
	}
	SDL_UnlockMutex(p_mutex);
	return found_type;
}
Type_Info type_GetTypeInfo_Safe(Type type) {
	SDL_LockMutex(p_mutex);
	if (types_count >= 65536) {
		tklog_critical("there are more than 65536 types. connot support more. types_count = %d\n", types_count);;
	}
	int type_index = -1;
	for (int i = 0; i < types_count; ++i) {
		if (p_types[i].type == type) {
			if (type_index != -1) {
				tklog_critical("allready found type index. there cant be multiples of the same type");
			}
			type_index = i;
		}
	}
	if (type_index == -1) {
		tklog_error("did not find type %d", type);
	}
	SDL_UnlockMutex(p_mutex);
	return p_types[type_index];
}
Type_Name type_GetName_Safe(Type type) {
	SDL_LockMutex(p_mutex);
	if (types_count >= 65536) {
		tklog_critical("there are more than 65536 types. connot support more. types_count = %d\n", types_count);
	}
	int type_index = -1;
	for (int i = 0; i < types_count; ++i) {
		if (p_types[i].type == type) {
			if (type_index != -1) {
				tklog_critical("allready found type index. there cant be multiples of the same type");
			}
			type_index = i;
		}
	}
	if (type_index == -1) {
		tklog_error("did not find type %d", type);
	}
	SDL_UnlockMutex(p_mutex);
	return p_types[type_index].name;
}
Type_Size type_GetSize_Safe(Type type) {
	SDL_LockMutex(p_mutex);
	if (types_count >= 65536) {
		tklog_critical("there are more than 65536 types. connot support more. types_count = %d\n", types_count);
		return 0;
	}
	int type_index = -1;
	for (int i = 0; i < types_count; ++i) {
		if (p_types[i].type == type) {
			if (type_index != -1) {
				tklog_error("allready found type index. there cant be multiples of the same type");
				return 0;
			}
			type_index = i;
		}
	}
	if (type_index == -1) {
		tklog_error("did not find type %d", type);
		return 0;
	}
	SDL_UnlockMutex(p_mutex);
	return p_types[type_index].size;
}
Type_Destructor type_GetDestructor_Safe(Type type) {
	SDL_LockMutex(p_mutex);
	if (types_count >= 65536) {
		tklog_critical("there are more than 65536 types. connot support more. types_count = %d\n", types_count);
		return NULL;
	}
	int type_index = -1;
	for (int i = 0; i < types_count; ++i) {
		if (p_types[i].type == type) {
			if (type_index != -1) {
				tklog_critical("allready found type index. there cant be multiples of the same type");
				return NULL;
			}
			type_index = i;
		}
	}
	if (type_index == -1) {
		tklog_error("did not find type %d", type);
		return NULL;
	}
	SDL_UnlockMutex(p_mutex);
	return p_types[type_index].destructor;
}