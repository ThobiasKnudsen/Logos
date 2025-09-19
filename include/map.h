#include <stdbool.h>

#define MAP_NULL_ID 0ULL
typedef unsigned long long map_id_t;

map_id_t  			map_Insert( void *value );
map_id_t  			map_InsertByString( const char *string, void *value );

bool  				map_Erase( map_id_t id );
bool        		map_EraseByString( const char *string );

void** 				map_Get( map_id_t id );
void** 				map_GetByString( const char *string );

unsigned long long 	map_Size( void );

bool 				map_HasKeyID( map_id_t id );
bool 				map_HasKeyString( const char *string );