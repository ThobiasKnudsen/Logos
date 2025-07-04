#!/bin/bash
# Cross-platform build script for Unix-like systems (Linux/macOS)

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Default values
BUILD_TYPE="Release"
BUILD_TESTS="OFF"
INSTALL_HEADERS="OFF"
CLEAN_BUILD="OFF"
INSTALL="OFF"
PACKAGE="OFF"
ADDRESS_SANITIZER="OFF"
THREAD_SANITIZER="OFF"

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --debug)
            BUILD_TYPE="Debug"
            shift
            ;;
        --release)
            BUILD_TYPE="Release"
            shift
            ;;
        --tests)
            BUILD_TESTS="ON"
            shift
            ;;
        --no-tests)
            BUILD_TESTS="OFF"
            shift
            ;;
        --install-headers)
            INSTALL_HEADERS="ON"
            shift
            ;;
        --clean)
            CLEAN_BUILD="ON"
            shift
            ;;
        --install)
            INSTALL="ON"
            shift
            ;;
        --package)
            PACKAGE="ON"
            shift
            ;;
        --asan|--address-sanitizer)
            ADDRESS_SANITIZER="ON"
            shift
            ;;
        --tsan|--thread-sanitizer)
            THREAD_SANITIZER="ON"
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo "Options:"
            echo "  --debug           Build in Debug mode"
            echo "  --release         Build in Release mode (default)"
            echo "  --tests           Enable building tests"
            echo "  --no-tests        Disable building tests (default)"
            echo "  --install-headers Install header files"
            echo "  --clean           Clean build directory before building"
            echo "  --install         Install after building"
            echo "  --package         Create package after building"
            echo "  --asan, --address-sanitizer  Enable Address Sanitizer"
            echo "  --tsan, --thread-sanitizer   Enable Thread Sanitizer"
            echo "  --help, -h        Show this help message"
            echo ""
            echo "Sanitizer Notes:"
            echo "  - Address Sanitizer (ASan) detects memory errors like buffer overflows"
            echo "  - Thread Sanitizer (TSan) detects data races and threading issues"
            echo "  - Sanitizers are only available with GCC/Clang on Linux/macOS"
            echo "  - Using sanitizers automatically enables Debug mode"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

# Validate sanitizer options
if [ "$ADDRESS_SANITIZER" = "ON" ] && [ "$THREAD_SANITIZER" = "ON" ]; then
    print_error "Cannot use both Address Sanitizer and Thread Sanitizer simultaneously"
    exit 1
fi

# Force Debug mode when using sanitizers
if [ "$ADDRESS_SANITIZER" = "ON" ] || [ "$THREAD_SANITIZER" = "ON" ]; then
    if [ "$BUILD_TYPE" = "Release" ]; then
        print_warning "Sanitizers require Debug mode. Switching to Debug build."
        BUILD_TYPE="Debug"
    fi
fi

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"

print_status "Building LOGOS in $BUILD_TYPE mode"
print_status "Project directory: $PROJECT_DIR"
print_status "Build directory: $BUILD_DIR"
print_status "Tests: $BUILD_TESTS"
if [ "$ADDRESS_SANITIZER" = "ON" ]; then
    print_status "Address Sanitizer: ENABLED"
fi
if [ "$THREAD_SANITIZER" = "ON" ]; then
    print_status "Thread Sanitizer: ENABLED"
fi

# Clean build if requested
if [ "$CLEAN_BUILD" = "ON" ]; then
    print_status "Cleaning build directory..."
    rm -rf "$BUILD_DIR"
fi

# Create build directory
mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

# Configure with CMake
print_status "Configuring with CMake..."
cmake "$PROJECT_DIR" \
    -DCMAKE_BUILD_TYPE="$BUILD_TYPE" \
    -DBUILD_TESTING="$BUILD_TESTS" \
    -DBUILD_TESTS="$BUILD_TESTS" \
    -DLOGOS_INSTALL_HEADERS="$INSTALL_HEADERS" \
    -DLOGOS_ADDRESS_SANITIZER="$ADDRESS_SANITIZER" \
    -DLOGOS_THREAD_SANITIZER="$THREAD_SANITIZER"

# Build
print_status "Building..."
make -j$(nproc)

print_success "Build completed successfully!"

# Install if requested
if [ "$INSTALL" = "ON" ]; then
    print_status "Installing..."
    cmake --install .
    print_success "Installation completed!"
fi

# Create package if requested
if [ "$PACKAGE" = "ON" ]; then
    print_status "Creating package..."
    cmake --build . --target package
    print_success "Package created successfully!"
fi

# If ThreadSanitizer is enabled, set up environment for running tests
if [ "$THREAD_SANITIZER" = "ON" ]; then
    print_status "Setting up ThreadSanitizer environment..."
    export TSAN_OPTIONS="abort_on_error=1:suppressions=${PROJECT_DIR}/tsan_suppressions.txt"
    print_status "TSAN_OPTIONS set to: $TSAN_OPTIONS"
fi

print_success "All operations completed successfully!" 