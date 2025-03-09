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
Type 			type_Create_Safe(
					Type_Name name,
					Type_Size size, 
					Type_Destructor destructor);
bool 			type_IsValid_Safe(
					Type type);
Type_Info 		type_GetTypeInfo_Safe(
					Type type);
Type_Name 		type_GetName_Safe(
					Type type);
Type_Size 		type_GetSize_Safe(
					Type type);
Type_Destructor type_GetDestructor_Safe(
					Type type);

#endif // VEC_TYPE_H