#!/usr/bin/env bash
# download_zig_compiler.sh <target_directory>
# Called automatically from build.zig with the correct install prefix
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <target_directory>"
    echo "Example: $0 ./zig-out/tools/zig_compiler"
    exit 1
fi

TARGET_DIR="$1"
echo "Installing latest Zig master into: $TARGET_DIR"

mkdir -p "$TARGET_DIR"
cd "$TARGET_DIR"

# Always ensure temp dir exists for JIT
mkdir -p temp

# Skip download if zig already exists and works
if [ -x zig ] && ./zig version >/dev/null 2>&1; then
    version=$(./zig version)
    echo "Bundled Zig already installed and working: $version"
    exit 0
fi

echo "Installing latest Zig master build..."

# Install required tools if missing (only on Debian/Ubuntu)
if ! command -v curl >/dev/null || ! command -v jq >/dev/null; then
    echo "Installing required tools (curl, jq)..."
    sudo apt update
    sudo apt install -y curl jq
fi

ARCH=$(uname -m)
case $ARCH in
    x86_64)
        ZIG_ARCH="x86_64-linux"
        ;;
    aarch64|arm64)
        ZIG_ARCH="aarch64-linux"
        ;;
    *)
        echo "Unsupported architecture: $ARCH (only x86_64 and aarch64 supported)"
        exit 1
        ;;
esac

INDEX_JSON=$(curl -s https://ziglang.org/download/index.json)
VERSION=$(echo "$INDEX_JSON" | jq -r '.master.version')
TARBALL_URL=$(echo "$INDEX_JSON" | jq -r --arg a "$ZIG_ARCH" '.master[$a].tarball')
SHASUM=$(echo "$INDEX_JSON" | jq -r --arg a "$ZIG_ARCH" '.master[$a].shasum')

if [ "$TARBALL_URL" = "null" ] || [ -z "$TARBALL_URL" ]; then
    echo "No master build available for $ZIG_ARCH"
    exit 1
fi

echo "Cleaning previous Zig installation (keeping temp/ and cache for JIT speed)..."
rm -rf zig lib doc zig-cache LICENSE VERSION cache || true

echo "Downloading Zig $VERSION ($ZIG_ARCH)..."
FILENAME=$(basename "$TARBALL_URL")
curl -L -# -o "$FILENAME" "$TARBALL_URL"

echo "Verifying SHA256 checksum..."
echo "$SHASUM $FILENAME" | sha256sum -c -

echo "Extracting..."
tar --strip-components=1 -xf "$FILENAME"
rm "$FILENAME"

version=$(./zig version)
echo "Zig $version successfully installed into $TARGET_DIR"
echo "Temp directory for JIT: $(realpath temp)"