cmake_minimum_required(VERSION 3.24)
project(CPI LANGUAGES C)

include(FetchContent)

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/lib")

# ====================================================================
# SDL3
# ====================================================================
FetchContent_Declare(
    SDL3
    GIT_REPOSITORY https://github.com/libsdl-org/SDL
    GIT_TAG release-3.2.0
)
set(SDL_SHARED OFF CACHE BOOL "Build SDL3 as a static library" FORCE)
set(SDL_STATIC ON CACHE BOOL "Build SDL3 as a static library" FORCE)
FetchContent_MakeAvailable(SDL3)
# list(APPEND CMAKE_PREFIX_PATH "${sdl3_BINARY_DIR}")

# ====================================================================
# SDL3_shadercross (will reuse SPIRV-Headers/SPIRV-Tools)
# ====================================================================
FetchContent_Declare(
    SDL3_shadercross
    GIT_REPOSITORY https://github.com/libsdl-org/SDL_shadercross.git
    GIT_TAG main
)
set(SDLSHADERCROSS_VENDORED ON CACHE BOOL "Use vendored dependencies (SPIRV-Cross, DXC, etc.)")
if(WIN32 OR WIN64)
    set(SDLSHADERCROSS_DXC ON CACHE BOOL "Enable HLSL support via DirectX Shader Compiler")
else()
    set(SDLSHADERCROSS_DXC OFF CACHE BOOL "Enable HLSL support via DirectX Shader Compiler")
endif()
set(SDLSHADERCROSS_CLI OFF CACHE BOOL "Disable building the CLI tool")
FetchContent_MakeAvailable(SDL3_shadercross)

# ====================================================================
# Makes SPIRV-Headers and SPIRV-Tools available
# ====================================================================
set(FETCHCONTENT_SOURCE_DIR_SPIRV-Headers"${sdl3_shadercross_SOURCE_DIR}/external/SPIRV-Headers")
set(FETCHCONTENT_SOURCE_DIR_SPIRV-Tools "${sdl3_shadercross_SOURCE_DIR}/external/SPIRV-Tools")

# ====================================================================
# glslang
# ====================================================================
FetchContent_Declare(
    glslang
    GIT_REPOSITORY https://github.com/KhronosGroup/glslang
    GIT_TAG vulkan-sdk-1.3.296.0
)
FetchContent_MakeAvailable(glslang)

# ====================================================================
# shaderc (reuse SPIRV-Headers/SPIRV-Tools)
# ====================================================================
set(SHADERC_SKIP_INSTALL ON CACHE BOOL "Skip install" FORCE)
set(SHADERC_SKIP_TESTS ON CACHE BOOL "Disable tests" FORCE)
set(SHADERC_SKIP_EXAMPLES ON CACHE BOOL "Disable examples" FORCE)
set(SHADERC_SKIP_COPYRIGHT_CHECK ON CACHE BOOL "Skip copyright" FORCE)
set(SHADERC_ENABLE_EXAMPLES OFF CACHE BOOL "" FORCE)
set(SHADERC_ENABLE_TESTS OFF CACHE BOOL "" FORCE)
FetchContent_Declare(
    shaderc
    GIT_REPOSITORY https://github.com/google/shaderc
    GIT_TAG v2024.3
)
FetchContent_MakeAvailable(shaderc)

# ====================================================================
# SPIRV-Reflect
# ====================================================================
FetchContent_Declare(
    SPIRV-Reflect
    GIT_REPOSITORY https://github.com/KhronosGroup/SPIRV-Reflect
    GIT_TAG main
)
FetchContent_MakeAvailable(SPIRV-Reflect)

# ====================================================================
# tcc
# ====================================================================
#include(ExternalProject)
#ExternalProject_Add(tcc
#  GIT_REPOSITORY https://github.com/TinyCC/tinycc.git
#  GIT_TAG mob
#  CONFIGURE_COMMAND ./configure --prefix="${CMAKE_CURRENT_SOURCE_DIR}/tcc" --enable-static --enable-libonly
#  BUILD_COMMAND make
#  INSTALL_COMMAND make install
#   ^ Tweak the flags for TCC's configure script as needed
#)
# Create an imported target that depends on ExternalProject build steps.
#add_library(tcc_lib INTERFACE)
#add_dependencies(tcc_lib tcc)
# Expose the include directory and library from the installed location
#ExternalProject_Get_Property(tcc INSTALL_DIR)
#target_include_directories(tcc_lib INTERFACE "${INSTALL_DIR}/include")
#target_link_libraries(tcc_lib INTERFACE "${INSTALL_DIR}/lib/libtcc.a")
# Or libtcc.so, depending on how it was built.

# ====================================================================
# Executable and Linking
# ====================================================================
add_executable(main 
    src/main.c
    src/debug.c
    src/vec.c
    src/vec_path.c
    src/type.c
    src/cpi.c
    ${spirv-reflect_SOURCE_DIR}/spirv_reflect.c
)

# Add SPIRV-Reflect include directory
target_include_directories(main PRIVATE 
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/shaders
    ${spirv-reflect_SOURCE_DIR}
)

# Link SPIRV-Reflect library
target_link_libraries(main PRIVATE
    SDL3_shadercross::SDL3_shadercross
    SDL3::SDL3
    shaderc_combined
    stdc++
#    tcc_lib
)

target_compile_definitions(main PRIVATE 
    DEBUG
)