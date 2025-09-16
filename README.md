# LOGOS

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/yourusername/logos) [![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Overview

LOGOS is an open-source, high-performance mathematical computation and visualization tool inspired by GeoGebra and MATLAB. It provides an interactive environment for exploring geometry, algebra, numerical analysis, and vector-based graphics. Built in pure C for maximum efficiency, LOGOS leverages modern concurrency (via Userspace RCU), GPU-accelerated rendering (SDL3 + shaders), and advanced memory management to handle complex computations and visualizations smoothly.

Key highlights:
- **Interactive Geometry**: Draw and manipulate vector paths, curves, and shapes with real-time feedback.
- **Numerical Computing**: Compute constants (e.g., π via high-precision methods), solve equations, and perform matrix operations.
- **Cross-Platform**: Runs on Windows, macOS, and Linux with native performance.
- **Extensible**: Thread-safe data structures for custom math libraries and plugins.
- **Visual Rendering**: Shader-based graphics for 2D/3D math visualizations.

LOGOS is designed for educators, students, and researchers who need a lightweight yet powerful alternative to bloated math software.

## Features

- **Core Math Primitives**: Vector operations (`vec.c`), path manipulation (`vec_path.c`), and type-safe data handling (`type.c`).
- **Concurrent Data Management**: Lock-free hash tables (`tsm.c`) for thread-safe storage of mathematical objects.
- **High-Precision Logging & Debugging**: Configurable logging (`tklog.c`) with memory tracking and scope tracing.
- **GPU Integration**: Real-time shader compilation and execution for dynamic visualizations.
- **Testing Suite**: Built-in unit tests for concurrency safety, memory leaks, and core algorithms.
- **Offline Builds**: Dependencies fetched once via CMake, then built disconnected for reproducible environments.

## Requirements

- **Compiler**: GCC/Clang (C17 standard) or MSVC.
- **CMake**: Version 3.24 or higher.
- **Git**: Required for initial dependency fetching.
- **Platform-Specific**:
  - **Linux**: `pkg-config`, `libasound2-dev` (for audio, optional).
  - **macOS**: Xcode Command Line Tools.
  - **Windows**: Visual Studio 2022 or MinGW-w64.
- **Optional Sanitizers**: AddressSanitizer (ASan) or ThreadSanitizer (TSan) for debugging (GCC/Clang only).

No internet required after initial setup—dependencies are vendored via FetchContent.

## Building

LOGOS uses CMake for configuration and supports Debug/Release builds with optional sanitizers.

### Quick Start (Unix-like: Linux/macOS)

1. Clone the repository:
   ```
   git clone https://github.com/yourusername/logos.git
   cd logos
   ```

2. Run the build script:
   ```
   ./build.sh --release
   ```
   - For Debug mode: `./build.sh --debug`
   - Enable tests: `./build.sh --tests`
   - Sanitizers: `./build.sh --asan` or `./build.sh --tsan` (forces Debug mode)
   - Clean build: `./build.sh --clean`
   - Install: `./build.sh --install`
   - Package: `./build.sh --package`

   The binary will be in `build/bin/logos` (or `logos.exe` on Windows).

### Manual CMake Build

1. Create a build directory:
   ```
   mkdir build && cd build
   ```

2. Configure:
   ```
   cmake .. -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTS=ON -DLOGOS_ADDRESS_SANITIZER=OFF
   ```

3. Build:
   ```
   cmake --build . -j$(nproc)
   ```

4. Run:
   ```
   ./bin/logos
   ```

### Windows (Visual Studio)

1. Open `CMakeLists.txt` in Visual Studio or use CMake GUI.
2. Generate project files and build the `logos` target.
3. Run `bin\logos.exe`.

### Build Options

| Option | Description | Default |
|--------|-------------|---------|
| `CMAKE_BUILD_TYPE` | `Debug` or `Release` | `Release` |
| `BUILD_TESTS` | Enable test executables | `OFF` |
| `LOGOS_ADDRESS_SANITIZER` | Enable ASan for memory debugging | `OFF` |
| `LOGOS_THREAD_SANITIZER` | Enable TSan for race detection | `OFF` |
| `LOGOS_INSTALL_HEADERS` | Install headers for development | `OFF` |

Tests include:
- `test_tklog`: Logging validation.
- `test_urcu_lfht_safety`: Concurrency safety.
- `test_global_data`: Global data integrity.
- `test_tsm`: Thread-safe map operations.

## Usage

Launch the application:
```
./bin/logos
```

### Basic Commands

LOGOS starts in an interactive mode similar to MATLAB/GeoGebra:

- **Compute π**: Use `cpi` module for high-precision calculation (e.g., via Machin-like formulas in `cpi.c`).
- **Vector Ops**: Define vectors with `vec_new(x, y)` and manipulate paths: `vec_path_add(point)`.
- **Plotting**: Enter commands like `plot sin(x)` to visualize functions with shader-based rendering.
- **Data Storage**: Store results in thread-safe maps: `tsm_insert(key, value)`.
- **Export**: Save visualizations as images or data files.

For scripting, pipe commands or use the embedded REPL. Full API docs are in `include/` (generated via Doxygen if enabled).

Example session:
```
$ ./bin/logos
LOGOS 1.0.0 > compute_pi(1000)
3.14159265358979323846
LOGOS 1.0.0 > vec v1 = vec_new(1.0, 2.0)
LOGOS 1.0.0 > plot_line(v1, vec_new(3.0, 4.0))
[Rendered: line.png]
```

### Keyboard Shortcuts (in GUI Mode)

- `Ctrl + Z`: Undo last operation.
- `F1`: Toggle logging levels.
- `Esc`: Exit interactive mode.

## Architecture

- **Core**: Single-threaded math primitives with optional concurrency via RCU.
- **Concurrency**: Lock-free hash tables (`tsm.c`) for safe multi-threaded access.
- **Graphics**: SDL3 for windowing, GLSLang/Shaderc for compute shaders.
- **Memory**: Mimalloc for fast allocation; optional tracking via TKLog.
- **Safety**: URCU wrappers (`urcu_lfht_safe.c`) prevent common race conditions.

## Contributing

1. Fork the repo and create a feature branch (`git checkout -b feat/amazing-feature`).
2. Commit changes (`git commit -m 'Add some feature'`).
3. Push to the branch (`git push origin feat/amazing-feature`).
4. Open a Pull Request.

Run tests before submitting:
```
./build.sh --tests --debug
ctest -V
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- [SDL3](https://github.com/libsdl-org/SDL) for graphics.
- [Userspace RCU](https://lttng.org/urcu) for concurrency.
- [xxHash](https://github.com/Cyan4973/xxHash) for fast hashing.
- Inspired by GeoGebra's interactivity and MATLAB's computation power.

Questions? Contact: thobknu@gmail.com

---

*Built with ❤️ by the LOGOS Team*