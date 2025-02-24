#define CPI_DEBUG
#include "cpi_debug.h"
#include "cpi.h"
#include <SDL3/SDL_events.h>
#include <SDL3/SDL_timer.h>

// Λόγος
// libuv
// libcurl

// 	TODO:
// 		ID shouldnt be int but long long
//  	GPI_Shader should be integrated into graphics and compute pipeline

int main() {
	TRACK(ASSERT(o_Initialize(), "ERROR: could not initialize Lo_GUI\n"));
	TRACK(CPI_ID_Handle window_handle = o_Window_Create(800, 600, "Λόγος"));
	TRACK(ASSERT(window_handle>=0, "ERROR: failed to create window\n"));
	TRACK(o_Window_Show(window_handle));
	TRACK(ASSERT(o_Window_Destroy(&window_handle), "ERROR: failed to destroy window\n"));
	TRACK(ASSERT(o_Destroy(), "ERROR: could not destroy Lo_GUI\n"));
	return 0;
}