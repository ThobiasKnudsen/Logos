#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Test 1: Buffer overflow with dynamic condition
void test_buffer_overflow() {
    printf("Testing buffer overflow...\n");
    char buffer[10];
    int len = 20; // This could come from user input or external source
    char* src = "This string is too long for the buffer";
    
    // Make it harder for compiler to detect statically
    if (len > 5 && len < 50) { // Add some complexity
        if (len % 2 == 0) { // More complexity
            strncpy(buffer, src, len); // This will overflow at runtime
            buffer[9] = '\0'; // Ensure null termination
        }
    }
    printf("Buffer: %s\n", buffer);
}

// Test 2: Use after free
void test_use_after_free() {
    printf("Testing use after free...\n");
    char* ptr = malloc(10);
    strcpy(ptr, "hello");
    free(ptr);
    printf("After free: %s\n", ptr); // This will trigger ASan
}

// Test 3: Double free
void test_double_free() {
    printf("Testing double free...\n");
    char* ptr = malloc(10);
    free(ptr);
    free(ptr); // This will trigger ASan
}

// Test 4: Memory leak (ASan can detect this too)
void test_memory_leak() {
    printf("Testing memory leak...\n");
    char* ptr = malloc(100);
    strcpy(ptr, "This memory will be leaked");
    // Missing free(ptr) - this will be reported as a leak
}

// Test 5: Stack buffer overflow with dynamic index
void test_stack_overflow() {
    printf("Testing stack buffer overflow...\n");
    char buffer[10];
    int index = 15; // This could come from external input
    
    // Make it harder for compiler to detect
    if (index > 10 && index < 20) {
        buffer[index] = 'X'; // This will overflow at runtime
    }
}

// Test 6: Global buffer overflow
char global_buffer[10];
void test_global_overflow() {
    printf("Testing global buffer overflow...\n");
    int index = 15;
    if (index >= 10) {
        global_buffer[index] = 'X'; // This will overflow at runtime
    }
}

// Test 7: Heap buffer overflow
void test_heap_overflow() {
    printf("Testing heap buffer overflow...\n");
    char* ptr = malloc(10);
    int index = 15;
    if (index > 9) {
        ptr[index] = 'X'; // This will overflow at runtime
    }
    free(ptr);
}

int main() {
    printf("Address Sanitizer Demo\n");
    printf("======================\n\n");
    
    // Uncomment the test you want to run:
    
    // test_buffer_overflow();
    // test_use_after_free();
    // test_double_free();
    // test_memory_leak();
    // test_stack_overflow();
    // test_global_overflow();
     test_heap_overflow();
    
    printf("No tests run. Uncomment a test in main() to see ASan in action.\n");
    printf("When you run with ASan enabled, you'll see detailed error reports.\n");
    printf("Note: Some errors might be caught at compile time by the compiler.\n");
    printf("ASan catches the ones that happen at runtime.\n");
    
    return 0;
} 