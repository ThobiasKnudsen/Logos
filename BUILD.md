# LOGOS Build System Guide

This document explains how to build and deploy LOGOS on Windows, macOS, and Linux using the modular CMake build system.

## Overview

The LOGOS project uses a modular CMake build system designed for cross-platform deployment. The build system automatically fetches all external dependencies including shader compilation tools.

## Prerequisites

### Windows
- Visual Studio 2019 or later with C++ development tools
- CMake 3.24 or later
- Git for Windows

### macOS
- Xcode Command Line Tools
- CMake 3.24 or later
- Homebrew (recommended for dependencies)

### Linux
- GCC or Clang compiler
- CMake 3.24 or later
- pkg-config
- Development headers for system libraries
- **userspace-rcu**: Required for concurrent data structures
  - Ubuntu/Debian: `sudo apt install liburcu8 liburcu-cds8 liburcu-dev`
  - Fedora/RHEL: `sudo dnf install liburcu liburcu-cds liburcu-devel`

## Quick Start

### Using Build Scripts (Recommended)

#### Linux/macOS
```bash
# Make build script executable
chmod +x scripts/build.sh

# Build in Release mode
./scripts/build.sh --release

# Build with tests and create package
./scripts/build.sh --release --tests --package

# Build in Debug mode with installation
./scripts/build.sh --debug --install
```

#### Windows
```cmd
# Build in Release mode
scripts\build.bat --release

# Build with tests and create package
scripts\build.bat --release --tests --package

# Build in Debug mode with installation
scripts\build.bat --debug --install
```

### Manual CMake Build

```bash
# Create build directory
mkdir build
cd build

# Configure
cmake .. -DCMAKE_BUILD_TYPE=Release

# Build (single job - safe)
cmake --build . --config Release

# Build with parallel jobs (use with caution)
cmake --build . --config Release --parallel 4

# Install (optional)
cmake --install .

# Create package (optional)
cmake --build . --target package
```

**Note:** The build scripts use single-job builds by default to prevent system freezes. If you want to use parallel builds, you can specify the number of jobs manually (e.g., `--parallel 4` for 4 jobs).

## Build Options

### CMake Options

- `CMAKE_BUILD_TYPE`: Build type (Debug, Release, RelWithDebInfo, MinSizeRel)
- `BUILD_TESTS`: Enable/disable test building (ON/OFF, default: OFF)
- `LOGOS_INSTALL_HEADERS`: Install header files for development (ON/OFF)

### Build Script Options

- `--debug`: Build in Debug mode
- `--release`: Build in Release mode (default)
- `--tests`: Enable building tests
- `--no-tests`: Disable building tests (default)
- `--install-headers`: Install header files
- `--clean`: Clean build directory before building
- `--install`: Install after building
- `--package`: Create package after building

## Platform-Specific Notes

### Windows
- Uses MSVC compiler by default
- Creates NSIS installer (.exe) and ZIP packages
- SDL3 DXC support is enabled for HLSL shaders
- Platform libraries: None required

### macOS
- Uses Clang compiler by default
- Creates DragNDrop and TGZ packages
- Platform libraries: None required

### Linux
- Uses GCC or Clang compiler
- Creates DEB, RPM, and TGZ packages
- Platform libraries: `asound` (ALSA)
- **URCU**: Required for concurrent data structures (Linux only)

## Dependencies

### System Dependencies (Linux Only)
- **userspace-rcu**: Required for concurrent data structures
  - Ubuntu/Debian: `sudo apt install liburcu8 liburcu-cds8 liburcu-dev`
  - Fedora/RHEL: `sudo dnf install liburcu liburcu-cds liburcu-devel`

### External Dependencies (Fetched Automatically)
- **SDL3**: Graphics, audio, and input (static build)
- **Verstable**: Version table management
- **xxHash**: Fast hashing library
- **SDL3_shadercross**: Shader cross-compilation
- **glslang**: GLSL compiler
- **shaderc**: Shader compilation
- **SPIRV-Reflect**: SPIR-V reflection

### tklog Cross-Platform Support

The tklog logging library has been designed for easy integration into other projects with minimal dependencies:

**Supported Platforms:**
- **Linux**: Native pthread support, POSIX timing
- **macOS**: Native pthread support, POSIX timing  
- **Windows**: pthread via MinGW-w64, Windows-specific high-resolution timing

**Dependencies:**
- **pthread**: For threading primitives (mutex, rwlock, thread-local storage)
- **Standard C library**: For memory management and string operations
- **Platform-specific timing**: 
  - Windows: `QueryPerformanceCounter` (with `GetTickCount64` fallback)
  - POSIX: `clock_gettime(CLOCK_MONOTONIC)` (with `gettimeofday` fallback)

**Integration:**
- No SDL dependency required
- Can be easily integrated into any C project
- Thread-safe logging with configurable output callbacks
- Optional memory tracking and scope tracing

## Output Structure

After building, the project structure will be:

```
build/
├── bin/
│   ├── logos                  # Main executable (logos.exe on Windows)
│   └── test_*                 # Test executables (if enabled)
├── lib/                       # Libraries (if any)
└── CMakeCache.txt            # CMake cache
```

## Installation

### System Installation
```bash
# Install to system directories
cmake --install .

# Install to custom directory
cmake --install . --prefix /opt/logos
```

### Package Installation

#### Windows
- Run the generated NSIS installer
- Or extract the ZIP package

#### macOS
- Mount the DragNDrop package
- Or extract the TGZ package

#### Linux
```bash
# Debian/Ubuntu
sudo dpkg -i LOGOS-1.0.0-Linux.deb

# Fedora/RHEL
sudo rpm -i LOGOS-1.0.0-Linux.rpm

# Generic
tar -xzf LOGOS-1.0.0-Linux.tar.gz
```

## Package Creation

The build system automatically creates platform-appropriate packages:

- **Windows**: NSIS installer (.exe) and ZIP archive
- **macOS**: DragNDrop and TGZ archive  
- **Linux**: DEB package, RPM package, and TGZ archive

### Creating Packages
```bash
# Using build script
./scripts/build.sh --package

# Using CMake directly
cmake --build . --target package
```

## Development

### Adding New Dependencies
1. Add `FetchContent_Declare` and `FetchContent_MakeAvailable` calls
2. Update target link libraries
3. Add platform-specific handling if needed

### Adding New Targets
1. Add target definition to main `CMakeLists.txt`
2. Update include directories and link libraries
3. Add to install rules if needed

### Platform-Specific Code
1. Use `LOGOS_PLATFORM_WINDOWS`, `LOGOS_PLATFORM_MACOS`, `LOGOS_PLATFORM_LINUX` variables
2. Use `LOGOS_PLATFORM_LIBS` for platform-specific libraries
3. Use `LOGOS_COMPILER_GCC_CLANG` and `LOGOS_COMPILER_MSVC` for compiler-specific code

## Troubleshooting

### Common Issues

#### Missing Dependencies (Linux)
```bash
# Install development headers
sudo apt install build-essential pkg-config liburcu-dev

# Or on Fedora/RHEL
sudo dnf install gcc cmake pkg-config liburcu-devel
```

#### CMake Version Issues
```bash
# Update CMake
# Linux: Use package manager or download from cmake.org
# macOS: brew install cmake
# Windows: Download from cmake.org
```

#### Build Failures
1. Check that all dependencies are installed
2. Ensure CMake version is 3.24 or later
3. Clean build directory: `./scripts/build.sh --clean`
4. Check compiler compatibility

### Getting Help
- Check the build output for specific error messages
- Verify all prerequisites are installed
- Ensure you're using a supported platform and compiler

## Continuous Integration

The build system supports CI/CD pipelines:

```yaml
# Example GitHub Actions workflow
- name: Build
  run: |
    mkdir build
    cd build
    cmake .. -DCMAKE_BUILD_TYPE=Release
    cmake --build . --config Release
    cmake --build . --target package
```

## Project Structure

```
LOGOS/
├── CMakeLists.txt              # Main build configuration
├── include/                    # Header files
├── src/                        # Source files
├── scripts/                    # Build scripts
│   ├── build.sh               # Unix build script
│   ├── build.bat              # Windows build script
│   └── clean.sh               # Clean script
├── cmake/                      # CMake configuration files
├── shaders/                    # Shader files
└── resources/                  # Application resources
```

This build system provides a robust foundation for cross-platform development and deployment of the LOGOS project with full shader support. 