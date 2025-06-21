# CPI (Computer Programming Interface)

A modern C-based graphics and game development framework focused on high-performance 2D rendering with concurrent data management capabilities.

## Overview

CPI is a comprehensive graphics programming interface that provides:
- **High-performance 2D rendering** with GPU acceleration
- **Lock-free concurrent data structures** using userspace RCU
- **Modern shader pipeline** with GLSL support
- **Cross-platform window management** via SDL3
- **Thread-safe development tools** and logging system

## Features

### 🎨 Graphics & Rendering
- GPU-accelerated 2D rectangle rendering
- Support for rotation, scaling, and corner radius
- Multi-texture binding (up to 8 textures)
- GLSL shader compilation and hot-reloading
- Cross-platform window management

### 🔄 Concurrent Programming
- Lock-free hash tables using userspace RCU
- Thread-safe global data graph system
- Type-safe node management
- Memory-safe concurrent access patterns

### 🛠️ Development Tools
- **TKLOG**: Advanced logging system with scope tracking
- Thread identification and timing
- Multiple log levels (DEBUG, INFO, WARNING, ERROR, etc.)
- Memory debugging and leak detection
- Vector and map data structures

### 🏗️ Architecture
- Modular design with clear separation of concerns
- Resource management with automatic cleanup
- Error handling and debugging utilities
- CMake-based build system

## Dependencies

### Required System Packages
```bash
# Ubuntu/Debian
sudo apt install liburcu-dev pkg-config

# Fedora/RHEL
sudo dnf install userspace-rcu-devel pkg-config

# Arch Linux
sudo pacman -S liburcu pkg-config
```

### Build Dependencies (Automatically Downloaded)
- SDL3 (window management)
- shaderc (GLSL compilation)
- SPIRV-Tools (shader optimization)
- glslang (shader validation)
- xxHash (fast hashing)
- userspace-rcu (concurrent data structures)

## Building

### Prerequisites
- CMake 3.24 or higher
- C compiler (GCC, Clang, or MSVC)
- Git

### Build Instructions

```bash
# Clone the repository
git clone <repository-url>
cd CPI_5

# Create build directory
mkdir build && cd build

# Configure and build
cmake ..
make -j$(nproc)

# Run the application
./bin/main
```

### Build Options

```bash
# Debug build with sanitizers
cmake -DCMAKE_BUILD_TYPE=Debug ..

# Release build with optimizations
cmake -DCMAKE_BUILD_TYPE=Release ..

# Custom compiler
cmake -DCMAKE_C_COMPILER=clang ..
```

## Usage

### Basic Example

```c
#include "cpi.h"

int main(void) {
    // Initialize the CPI system
    cpi_Initialize();
    
    // Create GPU device
    int gpu_device_id = cpi_GPUDevice_Create();
    
    // Create window
    int window_id = cpi_Window_Create(gpu_device_id, 800, 600, "My App");
    
    // Create shaders
    int vert_id = cpi_Shader_CreateFromGlslFile(gpu_device_id, 
        "shaders/shader.vert.glsl", "main", shaderc_vertex_shader, true);
    int frag_id = cpi_Shader_CreateFromGlslFile(gpu_device_id, 
        "shaders/shader.frag.glsl", "main", shaderc_fragment_shader, true);
    
    // Create graphics pipeline
    int pipeline_id = cpi_GraphicsPipeline_Create(vert_id, frag_id, true);
    
    // Show window
    cpi_Window_Show(window_id, pipeline_id);
    
    // Cleanup
    cpi_GraphicsPipeline_Destroy(&pipeline_id);
    cpi_Shader_Destroy(&vert_id);
    cpi_Shader_Destroy(&frag_id);
    cpi_Window_Destroy(&window_id);
    
    return 0;
}
```

### Concurrent Data Management

```c
#include "global_data.h"

// Initialize global data system
gd_init();

// Create a node
uint64_t node_id = gd_create_node(type_key);

// Thread-safe access
void* data = gd_get_unsafe(node_id, type_key);

// Cleanup
gd_free_node(node_id);
gd_cleanup();
```

### Logging with TKLOG

```c
#include "tklog.h"

// Log with different levels
TKLOG_DEBUG("Debug message");
TKLOG_INFO("Info message");
TKLOG_WARNING("Warning message");
TKLOG_ERROR("Error message");

// Scope-based logging
DEBUG_SCOPE(cpi_Initialize());
```

## Project Structure

```
CPI_5/
├── include/           # Header files
│   ├── cpi.h         # Main API
│   ├── global_data.h # Concurrent data structures
│   ├── tklog.h       # Logging system
│   └── ...
├── src/              # Source files
│   ├── main.c        # Example application
│   ├── cpi.c         # Core implementation
│   └── ...
├── shaders/          # GLSL shader files
│   ├── shader.vert.glsl
│   └── shader.frag.glsl
├── scripts/          # Build and installation scripts
└── CMakeLists.txt    # Build configuration
```

## Architecture

### Core Components

1. **CPI Core** (`cpi.c`): High-level graphics API
2. **Global Data** (`global_data.c`): Concurrent data management
3. **TKLOG** (`tklog.c`): Logging and debugging system
4. **Vector/Map** (`vec.c`, `map.c`): Data structures
5. **URCU Safe** (`urcu_safe.c`): Thread-safe utilities

### Data Flow

```
Application → CPI API → GPU Device → Shader Pipeline → Window Rendering
                ↓
            Global Data → RCU Hash Tables → Concurrent Access
```

## Development

### Adding New Features

1. **Graphics Features**: Extend `cpi.h` and implement in `cpi.c`
2. **Data Structures**: Add to `global_data.h` and implement thread-safe operations
3. **Shaders**: Create new GLSL files in `shaders/` directory
4. **Utilities**: Add to appropriate utility modules

### Debugging

Enable debug features in CMakeLists.txt:
```cmake
target_compile_definitions(main PRIVATE DEBUG)
```

Use TKLOG for comprehensive logging:
```c
TKLOG_DEBUG("Variable value: %d", my_var);
DEBUG_SCOPE(my_function_call());
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

[Add your license information here]

## Acknowledgments

- SDL3 team for cross-platform support
- Khronos Group for Vulkan and SPIR-V standards
- Google for shaderc compiler
- Userspace RCU project for concurrent data structures
