#!/bin/bash
# Cleanup script to remove build artifacts and start fresh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"

print_status "Cleaning build directory: $BUILD_DIR"

if [ -d "$BUILD_DIR" ]; then
    print_warning "Removing build directory..."
    rm -rf "$BUILD_DIR"
    print_success "Build directory removed"
else
    print_status "Build directory does not exist"
fi

print_success "Cleanup completed! You can now run a fresh build."
print_status "Recommended safe build: ./scripts/build.sh --safe-build" 