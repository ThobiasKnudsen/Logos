# LOGOS
![Video](./resources/demo_video.gif)  
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/ThobiasKnudsen/logos) [![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Overview

IN PROGRESS

LOGOS will be a high-performance mathematical computation and visualization tool inspired by GeoGebra and MATLAB. It provides an interactive environment for exploring geometry, algebra, numerical analysis, and vector-based graphics. The biggest feature will be a clean and logically consisten syntax all the way through at the same time being made in a general way so that coplex problems doesnt become a limitation. Built in pure C and not rust becasue of possible runtime compilation and linking. LOGOS leverages modern concurrency (via Userspace RCU), GPU-accelerated rendering (SDL3 + shaders), and advanced memory management to handle complex computations, visualizations smoothly and multithreaded scenarios.

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
   git clone https://github.com/ThobiasKnudsen/logos.git
   cd logos
   ```

2. Make sure these are installed:
   ```
   sudo apt update
   sudo apt install libxcursor-dev libx11-dev libxext-dev libxi-dev libxrender-dev libxrandr-dev libxss-dev libxcursor-dev libxinerama-dev libxfixes-dev libxxf86vm-dev
   sudo apt install libgl1-mesa-dev
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

Build and run:
   ```
   ./scripts/build.sh --release
   cd build
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

### Basic Usage

As example you can write and solve this:
```
x: axis_x(-10, 10)
y: axis_y(-10, 10)
show{x²+y²<9 and y < x² and ((x=0 and y=-1) or (x=0 and y=1))} # This will show a point at x=0,y=-1
show{x²+y²<9 and y < x² and y=x}                               # This will show a partial line of y = x
a: x²+y²<9 and y < x²                                          # we can do the same agian but storing the common part
show{a and ((x=0 and y=-1) or (x=0 and y=1))}                  # This will show a point at x=0,y=-1
show{a and y=x}                                                # This will show a partial line of y = x

 # This will show an animation in 10 seconds
t: time_seconds(0, 10)
a: x²+y²<t and y < x^t
show{
  if (a and (x>0 or y<-1)) {
    x=y
  } else {
    x=-y
  }
}
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

*Built with ❤️*
