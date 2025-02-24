#include "type.h"
#include "vec.h"
#include "debug.h"

static Type_Info* 		p_types 			= NULL;
static size_t 			types_count  		= 0;
static size_t 			types_capacity  	= 0;
static unsigned int  	unique_type_count  	= 0;
static bool   			constructed  		= false;
Type  					null_type;
Type  					type_type;

__attribute__((constructor(102)))
void _type_Constructor() {
	null_type = type_Create("NULL", 0, NULL);
	type_type = type_Create("Type", sizeof(Type), NULL);
	constructed = true;
}
Type type_Create(const char* type_name, Type_Size type_size, Type_Destructor destructor) {
	DEBUG_ASSERT(constructed ^ (types_count <= 1), "_type_Constructor is not run before running type_Create, constructed(%d) types_count(%d)", constructed, types_count);
	DEBUG_ASSERT(types_count <= types_capacity, "types_count is somehow greater than types_capacity");
	Type new_type = unique_type_count++;
	if (types_count == types_capacity) {
		types_capacity = 1;
		while (types_capacity <= types_count) {
			types_capacity *= 2;
		}
		DEBUG_SCOPE(p_types = alloc(p_types, types_capacity*sizeof(Type_Info)));
	}
	p_types[types_count].type = new_type;
	p_types[types_count].name = type_name;
	p_types[types_count].size = type_size;
	p_types[types_count].destructor = destructor;
	types_count++;
	printf("Created type %s\n", type_name);
	ASSERT(types_count < 65536, "there are more than 65536 types. connot support more. types_count = %d\n", types_count);
	return new_type;
}
bool type_IsValid(Type type) {
	ASSERT(types_count < 65536, "there are more than 65536 types. connot support more. types_count = %d\n", types_count);
	if (type == null_type) {
		return false;
	}
	bool found_type = false;
	for (int i = 0; i < types_count; ++i) {
		if (p_types[i].type == type) {
			ASSERT(!found_type, "allready found type index. there cant be multiples of the same type");
			found_type = true;
		}
	}
	return found_type;
}
Type_Info type_GetTypeInfo(Type type) {
	ASSERT(types_count < 65536, "there are more than 65536 types. connot support more. types_count = %d\n", types_count);
	bool found_type = false;
	int type_index = 0;
	for (int i = 0; i < types_count; ++i) {
		if (p_types[i].type == type) {
			ASSERT(!found_type, "allready found type index. there cant be multiples of the same type");
			found_type = true;
			type_index = i;
		}
	}
	return p_types[type_index];
}