#ifndef LOGOS_SDL3_INIT_H
#define LOGOS_SDL3_INIT_H

#include "tsm.h"

/**
 * This is a SINGLETON module. Only one should exist at the time. 
 * But dont worry. You will not be able to create multiple instances 
 * of this module.
 */

CM_RES sdl3_core_init(const struct tsm_base_node* p_tsm_base, const struct tsm_key* p_key);
CM_RES sdl3_core_tsm_get(const struct tsm_base_node** pp_output_tsm_node);
CM_RES sdl3_core_quit();

#endif // LOGOS_SDL3_INIT_H