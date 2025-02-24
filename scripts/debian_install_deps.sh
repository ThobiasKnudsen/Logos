#!/usr/bin/env bash

# ------------------------------------------------------------------
# Script: install_sdl_deps.sh
# Description: Installs dependencies for SDL, Vulkan SDK, and shaderc
#              with checks to avoid redundant installations.
# ------------------------------------------------------------------

set -e  # Exit immediately on error
set -o pipefail  # Ensure pipeline errors are caught

### Helper Functions ###

# Check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Check if a directory exists
directory_exists() {
    [ -d "$1" ]
}

# Check if a package is installed
package_installed() {
    dpkg -l "$1" | grep -q ^ii
}

### 1) System-wide dependencies ###
echo "Checking system dependencies..."
sudo apt-get update

# List of required packages
REQUIRED_PKGS=(
    build-essential cmake git ninja-build
    libasound2-dev libpulse-dev libx11-dev libxext-dev
    libxrandr-dev libxcursor-dev libxi-dev libxinerama-dev
    libxxf86vm-dev libwayland-dev wayland-protocols
    libwayland-egl-backend-dev libxkbcommon-dev libdrm-dev
    libgbm-dev libudev-dev libpipewire-0.3-dev libharfbuzz-dev
    libfreetype6-dev libgl1-mesa-dev libvulkan1 mesa-vulkan-drivers
    libvulkan-dev vulkan-validationlayers-dev glslang-dev
    spirv-tools libspirv-cross-c-shared-dev
)

# Install missing packages
TO_INSTALL=()
for pkg in "${REQUIRED_PKGS[@]}"; do
    if ! package_installed "$pkg"; then
        TO_INSTALL+=("$pkg")
    fi
done

if [ ${#TO_INSTALL[@]} -gt 0 ]; then
    echo "Installing missing packages..."
    sudo apt-get install -y "${TO_INSTALL[@]}"
else
    echo "All required packages already installed."
fi

### 2) shaderc installation ###
SHADERC_VERSION="2024.4"
if ! command_exists shaderc_combined || \
   ! pkg-config --exists shaderc --atleast-version=$SHADERC_VERSION; then
    echo "Installing shaderc from source..."
    
    # Cleanup previous attempts
    [ -d "shaderc" ] && rm -rf shaderc
    
    # Clone with all submodules
    git clone --recurse-submodules https://github.com/google/shaderc.git
    cd shaderc
    git checkout v$SHADERC_VERSION
    
    # Use Shaderc's official dependency sync script
    ./utils/git-sync-deps
    
    mkdir build && cd build
    cmake -DCMAKE_INSTALL_PREFIX=/usr/local \
          -DSHADERC_ENABLE_SHARED_CRT=ON \
          -DSHADERC_SKIP_GMOCK_DOWNLOAD=ON \
          -DSHADERC_SKIP_GOOGLETEST_DOWNLOAD=ON \
          -DSHADERC_SKIP_TEST_INSTALL=ON \
          -DSHADERC_ENABLE_TESTS=OFF \
          -DSPIRV_BUILD_TESTS=OFF \
          -Dglslang_BUILD_TESTS=OFF \
          -DCMAKE_BUILD_TYPE=Release ..
    make -j$(nproc)
    sudo make install
    cd ../..
    rm -rf shaderc
else
    echo "shaderc (>= $SHADERC_VERSION) already installed."
fi

### 3) Vulkan SDK installation ###
VULKAN_SDK_VERSION="1.4.304.0"
SDK_INSTALL_DIR="/opt/vulkan-sdk-$VULKAN_SDK_VERSION"

if [ ! -d "$SDK_INSTALL_DIR" ] || [ ! -f "$SDK_INSTALL_DIR/setup-env.sh" ]; then
    echo "Installing Vulkan SDK $VULKAN_SDK_VERSION..."
    
    # Cleanup previous downloads
    [ -f "vulkansdk.tar.xz" ] && rm vulkansdk.tar.xz
    
    SDK_URL="https://sdk.lunarg.com/sdk/download/$VULKAN_SDK_VERSION/linux/vulkansdk-linux-x86_64-$VULKAN_SDK_VERSION.tar.xz"
    if ! wget -O vulkansdk.tar.xz "$SDK_URL"; then
        echo "Failed to download Vulkan SDK"
        exit 1
    fi

    # Verify checksum (recommended)
    # You can add official checksum verification here
    
    tar -xf vulkansdk.tar.xz
    sudo mkdir -p "$SDK_INSTALL_DIR"
    sudo mv "$VULKAN_SDK_VERSION/x86_64"/* "$SDK_INSTALL_DIR"
    sudo chown -R root:root "$SDK_INSTALL_DIR"
    rm -rf "$VULKAN_SDK_VERSION" vulkansdk.tar.xz
else
    echo "Vulkan SDK $VULKAN_SDK_VERSION already installed."
fi

### 4) Environment setup ###
ZSHRC="${HOME}/.zshrc"
ENV_VARS=(
    "export VULKAN_SDK=\"$SDK_INSTALL_DIR\""
    "export PATH=\"\$VULKAN_SDK/bin:\$PATH\""
    "export LD_LIBRARY_PATH=\"\$VULKAN_SDK/lib:\$LD_LIBRARY_PATH\""
)

# Check if variables already exist
NEEDS_UPDATE=false
for var in "${ENV_VARS[@]}"; do
    if ! grep -qxF "$var" "$ZSHRC"; then
        NEEDS_UPDATE=true
        break
    fi
done

if $NEEDS_UPDATE; then
    echo "Updating environment variables..."
    echo -e "\n# Vulkan SDK environment" >> "$ZSHRC"
    for var in "${ENV_VARS[@]}"; do
        echo "$var" >> "$ZSHRC"
    done
    echo "Please run 'source $ZSHRC' or restart your shell"
else
    echo "Environment variables already configured."
fi

echo -e "\nInstallation complete!"