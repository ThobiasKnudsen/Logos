#ifndef GPU_WINDOW_H
#define GPU_WINDOW_H

#include "tsm.h"
#include <SDL3/SDL.h>

/**
 * BE CAUTIOS
 * You must be sure that any defered free of any window is called from the callback thread
 * before SDL_Quit is called, otherwise the window will not be destroyed properly as 
 * the defered free callback itself defers destruction of the SDL_Window to the main thread.
 * Be sure that rcu_barrier() is called before SDL_Quit so that you are sure the defered free is 
 * called and SDL_Quit will continue and complete the destruction of the window.
 * As long as the windowing loop is still running when the defer free is called, the window 
 * will be destroyed anyways inside the "e.type == destroyWindowEventType && e.user.code == 1" branch. 
 * 
 * AS OF 2025:
 * child windows is not supported, because when a parent window is destroy all child windows will also
 * be destroyed according to SDL_DestroyWindow documentation and makes all child windows nodes inside 
 * TSM invalid as the pointer to the window gets invalid.
 */
CM_RES sdl3_window_create(
    const struct tsm_key* p_key,
    unsigned int width,
    unsigned int height,
    const char* title);
/**
 * This function must be within a read section and must last throughout all usage of the 
 * returned gpu device pointer
 */
CM_RES sdl3_window_get(const struct tsm_key* p_key, SDL_Window** pp_output_window);
CM_RES sdl3_window_show(const struct tsm_key* p_window_key, const struct tsm_key* p_graphics_pipeline_key);
CM_RES sdl3_window_show_1(const struct tsm_key* p_window_key, const struct tsm_key* p_graphics_pipeline_key);

#endif // GPU_WINDOW_H