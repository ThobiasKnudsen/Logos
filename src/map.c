#include "map.h"
#include <stdbool.h>
#include <string.h>
#include "tklog.h"

// anymailfinder

/* ------------------------------------------------------------------------------------------------
 *  Verstable instantiations
 * ------------------------------------------------------------------------------------------------ */

#define NAME            str_id_map
#define KEY_TY          const char*
#define VAL_TY          map_id_t
#include "verstable.h"

#define NAME            id_str_map
#define KEY_TY          map_id_t
#define VAL_TY          const char*
#include "verstable.h"

#define NAME            id_ptr_map
#define KEY_TY          map_id_t
#define VAL_TY          void*
#include "verstable.h"

/* ------------------------------------------------------------------------------------------------
 *  Globals
 * ------------------------------------------------------------------------------------------------ */
static str_id_map       str_map;
static id_ptr_map       id_map;
static map_id_t         id_counter          = 1ULL;

/* ------------------------------------------------------------------------------------------------
 *  Init – runs automatically at load time (constructor priority 101 so SDL is ready)  
 * ------------------------------------------------------------------------------------------------ */
__attribute__((constructor(101)))
static void _map_Init( void ) {
    str_id_map_init( &str_map );
    id_ptr_map_init( &id_map );
}

/* ------------------------------------------------------------------------------------------------
 *  Create (string → id → ptr) – returns the freshly-assigned id on success, 0 on failure.
 * ------------------------------------------------------------------------------------------------ */
map_id_t map_InsertByString(const char *string, void *value)
{
    if (!string) {
        tklog_warning("map_InsertByString: key is NULL (value %p)\n", value);
        return MAP_NULL_ID;
    }

    /* Fast-path: key already present?                                         */
    if (map_HasKeyString(string)) {
        tklog_warning("map_InsertByString(\"%s\", %p): key already exists\n", string, value);
        return MAP_NULL_ID;
    }

    /* Duplicate *after* we know we need it.                                   */
    char *key_dup = strdup(string);
    if (!key_dup) {
        tklog_critical("map_InsertByString: out of memory duplicating \"%s\"\n", string);
        return MAP_NULL_ID;
    }

    map_id_t new_id = id_counter;          /* reserve the id up-front          */

    /* Insert (string  -> id) ------------------------------------------------ */
    str_id_map_itr s_itr = str_id_map_insert(&str_map, key_dup, new_id);
    if (str_id_map_is_end(s_itr)) {
        tklog_critical("map_InsertByString: failed to insert \"%s\" into str_map\n", key_dup);
        free(key_dup);
        return MAP_NULL_ID;
    }

    /* Insert (id -> ptr) ---------------------------------------------------- */
    id_ptr_map_itr i_itr = id_ptr_map_insert(&id_map, new_id, value);
    if (id_ptr_map_is_end(i_itr)) {
        /* roll back & clean up --------------------------------------------- */
        str_id_map_erase(&str_map, key_dup);
        free(key_dup);
        tklog_critical("map_InsertByString: failed to insert id→value pair for \"%s\"\n", key_dup);
        return MAP_NULL_ID;
    }

    ++id_counter;                          /* only advance if both inserts ok  */
    return new_id;
}

/* ------------------------------------------------------------------------------------------------
 *  Erase by id – removes the (id, ptr) pair and its associated string key.
 * ------------------------------------------------------------------------------------------------ */
bool map_Erase(map_id_t id)
{
    if (id == MAP_NULL_ID) {
        tklog_warning("map_Erase: id 0 is invalid\n");
        return false;
    }

    id_ptr_map_itr id_itr = id_ptr_map_get(&id_map, id);
    if (id_ptr_map_is_end(id_itr)) {
        tklog_debug("map_Erase: id %llu does not exist\n", id);
        return false;
    }

    /* Remove id → ptr first ------------------------------------------------- */
    id_ptr_map_erase(&id_map, id);

    /* Find the matching (string → id) pair so we can erase *and* free key.    */
    for (str_id_map_itr s_itr = str_id_map_first(&str_map);
         !str_id_map_is_end(s_itr);
         s_itr = str_id_map_next(s_itr))
    {
        if (s_itr.data->val == id) {
            const char *dup_key = s_itr.data->key;     /* save pointer before erase */
            str_id_map_erase(&str_map, dup_key);
            free((void *)dup_key);                     /* free the strdup’ed key    */
            return true;
        }
    }

    tklog_warning("map_Erase(%llu): removed id but no matching string key found\n", id);
    return true;    /* id part was still removed – maps are already diverging */
}

/* ------------------------------------------------------------------------------------------------
 *  Erase by string key – convenience wrapper.
 * ------------------------------------------------------------------------------------------------ */
bool map_EraseByString(const char *string)
{
    if (!string) {
        tklog_warning("map_EraseByString: key is NULL\n");
        return false;
    }

    str_id_map_itr s_itr = str_id_map_get(&str_map, (char *)string);
    if (str_id_map_is_end(s_itr)) {
        tklog_warning("map_EraseByString: key \"%s\" not found\n", string);
        return false;
    }

    /* Grab the stored key pointer and id before we mutate anything.          */
    const char  *dup_key = s_itr.data->key;
    map_id_t     id      = s_itr.data->val;

    /* First remove from str → id so look-ups cannot see half-deleted state.  */
    str_id_map_erase(&str_map, dup_key);
    free((void *)dup_key);

    /* Then remove from id → ptr.                                             */
    if (!id_ptr_map_erase(&id_map, id)) {
        tklog_error("map_EraseByString: id %llu not found in id_map "
                    "(maps were already inconsistent)\n", id);
        return false;
    }
    return true;
}

void** map_Get( map_id_t id ) {
    if( id == MAP_NULL_ID ) {
        tklog_warning( "map_Get: id 0 is invalid\n" );
        return NULL;
    }

    id_ptr_map_itr id_itr = id_ptr_map_get( &id_map, id );
    if (id_ptr_map_is_end( id_itr )) {
    	tklog_error("in map_Get(%llu): didnt find id %llu\n", id, id);
    	return NULL;
    }

    void** result = &id_itr.data->val;
    return result;
}

/* ------------------------------------------------------------------------------------------------
 *  Lookup by string key
 * ------------------------------------------------------------------------------------------------ */
void **map_GetByString( const char *string ) {
    if( !string ) {
        tklog_warning( "map_GetByString: key is NULL\n" );
        return NULL;
    }

    str_id_map_itr str_itr = str_id_map_get( &str_map, (char *)string );
    if( str_id_map_is_end( str_itr ) ) {
        tklog_debug( "map_GetByString: key %s not found\n", string );
        return NULL;
    }

    tklog_scope( void **pp = map_Get(str_itr.data->val) );

    return pp;
}

/* ----- Unsafe variant – caller must already hold the lock --------------------------------------- */
map_id_t map_Size( void ) {
    return id_ptr_map_size( &id_map );
}


bool map_HasKeyID( map_id_t id ) {
    id_ptr_map_itr id_itr = id_ptr_map_get( &id_map, id );
    return !id_ptr_map_is_end( id_itr );
}

/* ----- Unsafe: by string key ------------------------------------------------------------------- */
bool map_HasKeyString( const char *string ) {
    if( !string ) {
        return false;
    }

    str_id_map_itr str_itr = str_id_map_get( &str_map, (char *)string );
    return !str_id_map_is_end( str_itr );
}