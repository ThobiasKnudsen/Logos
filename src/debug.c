#include "debug.h"

#include <stdlib.h>
#include <signal.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <SDL3/SDL.h>

#ifdef DEBUG
	#undef printf
	#undef malloc
	#undef realloc
	#undef free
// ===========================================================================================================================
// Internal
// ===========================================================================================================================
typedef struct {
    void* 			ptr;
    size_t 			size_bytes;
    size_t 			line;
    char* 			file;
} AllocTracking;
typedef struct {
    AllocTracking* 	all_allocs;
    size_t          all_allocs_size;
    size_t          all_allocs_count;
    unsigned int    start_time_ms;
    char*           code_location;
    size_t          code_location_size;
} DebugData;
static DebugData debug_data;
void _debug_ExitFunction() {
	if (debug_data.all_allocs_count == 0) {
		return;
	}
	printf("\nMEMORY NOT FREED:\n");
	for (int i = 0; i < debug_data.all_allocs_count; i++) {
		printf("	%p %zu bytes | allocated at%s:%zu\n", 	
			debug_data.all_allocs[i].ptr,
			debug_data.all_allocs[i].size_bytes,
			debug_data.all_allocs[i].file,
			debug_data.all_allocs[i].line);
		free(debug_data.all_allocs[i].ptr);
	}
}
void _debug_CrashFunction(int sig, siginfo_t *info, void *context) {
    size_t buffer_size = 2048;
    char buffer[buffer_size];
    int offset = 0;
    int temp;
    // Construct the initial error message
    temp = snprintf(buffer + offset, buffer_size - offset,
                    "\nERROR program crashed with signal %d in %s\n",
                    sig, debug_data.code_location);
    if (temp > 0 && temp < buffer_size - offset) {
        offset += temp;
    }
    if (debug_data.all_allocs_count == 0) {
        write(STDERR_FILENO, buffer, offset);  // Output the message
        _exit(1);
    }
    // Additional message about freeing memory
    temp = snprintf(buffer + offset, buffer_size - offset, "\nfreeing memory\n");
    if (temp > 0 && temp < buffer_size - offset) {
        offset += temp;
    }
    // Iterate over all allocations
    for (int i = 0; i < debug_data.all_allocs_count; i++) {
        temp = snprintf(buffer + offset, buffer_size - offset,
                        "    address %p | %zu bytes | at %s:%zu\n",
                        debug_data.all_allocs[i].ptr,
                        debug_data.all_allocs[i].size_bytes,
                        debug_data.all_allocs[i].file,
                        debug_data.all_allocs[i].line);
        if (temp > 0 && temp < buffer_size - offset) {
            offset += temp;
        }

        free(debug_data.all_allocs[i].ptr);  // This is still not safe in a SIGSEGV handler
    }
    // Write the entire message at once
    write(STDERR_FILENO, buffer, offset);
    _exit(1);  // Terminate the program immediately
}

__attribute__((constructor(101)))
void debug_Init() {
	debug_data.all_allocs = malloc(256*sizeof(AllocTracking));
	if (debug_data.all_allocs == NULL) {
		printf("ERROR | debug_data.all_allocs = malloc(2048*sizeof(DebugData));\n");
		exit(EXIT_FAILURE);
	}

    debug_data.all_allocs_size = 256;
    debug_data.all_allocs_count = 0;
    debug_data.start_time_ms = (unsigned int)(clock() * 1000 / CLOCKS_PER_SEC);

    debug_data.code_location = malloc(2048 * sizeof(char));
    if (debug_data.code_location == NULL) {
		printf("ERROR | debug_data.code_location = malloc(2048 * sizeof(char));\n");
		exit(EXIT_FAILURE);
    }

	memset(debug_data.code_location, 0, 2048 * sizeof(char));
    debug_data.code_location_size = 2048;


	atexit(_debug_ExitFunction);

	struct sigaction sa;
    memset(&sa, 0, sizeof(struct sigaction));
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = SA_SIGINFO;
    sa.sa_sigaction = _debug_CrashFunction;

    sigaction(SIGSEGV, &sa, NULL);
	sigaction(SIGILL, &sa, NULL);
	sigaction(SIGFPE, &sa, NULL);
}

// ===========================================================================================================================
// External
// ===========================================================================================================================
void debug_Printf(
	const char* message, 
	size_t line, 
	const char* file) 
{
	unsigned int current_time = (unsigned int)(clock() * 1000 / CLOCKS_PER_SEC);
	printf("%dms %s %s:%ld | %s", current_time-debug_data.start_time_ms, debug_data.code_location, file, line, message);
}
void* debug_Malloc(
	size_t size, 
	size_t line, 
	const char* file) 
{
	while (debug_data.all_allocs_count >= debug_data.all_allocs_size) {
		debug_data.all_allocs_size *= 2;
		void* tmp = realloc(debug_data.all_allocs, debug_data.all_allocs_size * sizeof(AllocTracking));
		if (!tmp) {
			fprintf(stderr, "ERROR | Allocation failed at %s:%zu during realloc.\n", file, line);
			exit(EXIT_FAILURE);
		}
		debug_data.all_allocs = tmp;
	}

	void* tmp = malloc(size);
	if (!tmp) {
		fprintf(stderr, "ERROR | Allocation failed at %s:%zu during malloc.\n", file, line);
		exit(EXIT_FAILURE);
	}

	debug_data.all_allocs[debug_data.all_allocs_count].ptr = tmp;
	debug_data.all_allocs[debug_data.all_allocs_count].size_bytes = size;
	debug_data.all_allocs[debug_data.all_allocs_count].line = line;

	size_t string_length = strlen(debug_data.code_location) + strlen(file) + 2;
	if (!debug_data.all_allocs[debug_data.all_allocs_count].file) {
		debug_data.all_allocs[debug_data.all_allocs_count].file = malloc(string_length);
		if (!debug_data.all_allocs[debug_data.all_allocs_count].file) {
			fprintf(stderr, "ERROR | debug.h debug_Malloc debug_data.all_allocs[debug_data.all_allocs_count].file=malloc(byte_size);\n");
			exit(EXIT_FAILURE);
		}
	}
	else {
		tmp = realloc(debug_data.all_allocs[debug_data.all_allocs_count].file, string_length);
		if (!tmp) {
			fprintf(stderr, "ERROR | debug.h debug_Malloc tmp=realloc(debug_data.all_allocs[debug_data.all_allocs_count].file,byte_size);\n");
			exit(EXIT_FAILURE);
		}
		debug_data.all_allocs[debug_data.all_allocs_count].file = tmp;
	}
	sprintf(debug_data.all_allocs[debug_data.all_allocs_count].file, "%s %s", debug_data.code_location, file);

	debug_data.all_allocs_count++;
	return debug_data.all_allocs[debug_data.all_allocs_count-1].ptr; // -1 because it is itterated once
}
void* debug_Realloc(
	void* ptr, 
	size_t size, 
	size_t line, 
	char* file) 
{

	void* new_ptr = NULL;

    for (size_t i = 0; i < debug_data.all_allocs_count; i++) {
        if (debug_data.all_allocs[i].ptr == ptr) {
        	if (new_ptr) {
        		debug_Printf("ERROR: there are multiple of given ptr in allocation tracking for debug_Realloc", __LINE__, __FILE__);
    			exit(-1);
        	}
            new_ptr = realloc(ptr, size);
            if (!new_ptr) {
                fprintf(stderr, "ERROR: Failed to reallocate memory in debug_Realloc at %s:%zu\n", file, line);
                exit(EXIT_FAILURE);
            }
            debug_data.all_allocs[i].ptr = new_ptr;
            debug_data.all_allocs[i].size_bytes = size;
            debug_data.all_allocs[i].line = line;
            debug_data.all_allocs[i].file = file;
        }
    }

    if (new_ptr == NULL) {
    	debug_Printf("ERROR: Pointer not found in allocation tracking for debug_Realloc", __LINE__, __FILE__);
	    exit(-1);
    }

	return new_ptr;
}
void debug_Free(
	void* ptr, 
	size_t line, 
	const char* file) 
{
	if (!ptr) {
		debug_Printf("you tried to free NULL ptr\n", line, file); // Changed function name to "debug_Printf" to match existing function
		printf("ERROR\n");
		exit(EXIT_FAILURE);
	}
	int index = -1;
	for (int i = 0; i < debug_data.all_allocs_count; i++) {
		if (debug_data.all_allocs[i].ptr == ptr) { // Changed '=' to '==' for correct comparison
			index = i;
			break;
		}
	}
	if (index == -1) {
		debug_Printf("you tried to double free\n", line, file); // Changed function name to "debug_Printf" to match existing function
		printf("ERROR\n");
		exit(EXIT_FAILURE);
	}
	free(ptr);
	for (int i = index; i < debug_data.all_allocs_count-1; i++) {
		debug_data.all_allocs[i] = debug_data.all_allocs[i+1]; // Changed '+1' to correct placement for struct copying
	}
	debug_data.all_allocs_count--;
}
void debug_StartScope(
	size_t line, 
	const char* file) 
{
	if (!file) {
		printf("ERROR | !file\n");
		exit(EXIT_FAILURE);
	}

    char adding_string[50];
    snprintf(adding_string, sizeof(adding_string), " %s:%ld", file, line);
    size_t current_length = strlen(debug_data.code_location);
    size_t needed_size = current_length + strlen(adding_string) + 1;

    if (debug_data.code_location_size < needed_size) {
        while (debug_data.code_location_size < needed_size) {
            debug_data.code_location_size *= 2;
        }
        void* tmp = realloc(debug_data.code_location, debug_data.code_location_size);
        if (!tmp) {
            printf("ERROR | void* tmp = realloc(debug_data.code_location, debug_data.code_location_size);\n");
            exit(EXIT_FAILURE);
        }
        debug_data.code_location = tmp;
    }

    strcat(debug_data.code_location + strlen(debug_data.code_location), adding_string);
}
void debug_EndScope() {
	int index = -1;
	for (int i = debug_data.code_location_size-1; i >= 0 ; i--) {
		if (debug_data.code_location[i]==' ') {
			index = i;
			break;
		}
	}
	if (index==-1) {
		printf("ERROR | index==-1");
		exit(EXIT_FAILURE);
	}
	debug_data.code_location[index] = '\0';
}
size_t debug_GetPointerSize(void* ptr) {
	if (!ptr) {
		return 0;
	}
	int index = -1;
	for (int i = 0; i < debug_data.all_allocs_count; i++) {
		if (debug_data.all_allocs[i].ptr == ptr) {
			index = i;
			break;
		}
	}
	if (index == -1) {
		return 0;
	}
	return debug_data.all_allocs[index].size_bytes;
}
void debug_PrintMemory() {
	printf("\nunfreed memory:\n");
	for (int i = 0; i < debug_data.all_allocs_count; i++) {
		printf("	address %p | %zu bytes | at %s:%zu\n",  
			debug_data.all_allocs[i].ptr,
			debug_data.all_allocs[i].size_bytes,
			debug_data.all_allocs[i].file,
			debug_data.all_allocs[i].line);
	}
	printf("\n");
}
#endif