#ifndef VEC_TYPE_H
#define VEC_TYPE_H

#include <stdbool.h>

typedef unsigned short Type;
typedef const char*    Type_Name;
typedef unsigned short Type_Size;
typedef void (*Type_Destructor)(void* type_instance);
typedef struct Type_Info {
	Type 				type;
	Type_Name   		name;
	Type_Size 			size;
	Type_Destructor 	destructor;
} Type_Info;

extern Type 			null_type;
extern Type 			type_type;

// if return = 0 it means it failed
Type 			type_Create(
					Type_Name name,
					Type_Size size, 
					Type_Destructor destructor);
bool 			type_IsValid(
					Type type);
Type_Info 		type_GetTypeInfo(
					Type type);

#endif // VEC_TYPE_H