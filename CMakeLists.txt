cmake_minimum_required(VERSION 3.24)
project(LOGOS LANGUAGES C VERSION 1.0.0)

# Add this after the project declaration to enable offline builds after initial fetch
set(FETCHCONTENT_UPDATES_DISCONNECTED ON CACHE BOOL "Prevent FetchContent from updating sources")

# Ensure Git is found to perform direct population at configure time (avoids deferred fetches during build)
find_package(Git REQUIRED)

# Set C standard
set(CMAKE_C_STANDARD 17)
set(CMAKE_C_STANDARD_REQUIRED ON)

# Build options (SAFE DEFAULTS)
option(LOGOS_ADDRESS_SANITIZER "Enable Address Sanitizer" OFF)
option(LOGOS_THREAD_SANITIZER "Enable Thread Sanitizer" OFF)

# Set output directories
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/lib")
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/lib")

# Enable IPO/LTO for release builds if supported
include(CheckIPOSupported)
check_ipo_supported(RESULT ipo_supported LANGUAGES C)

# Platform detection
if(WIN32)
    set(LOGOS_PLATFORM_WINDOWS ON)
    set(LOGOS_PLATFORM_LIBS "")
elseif(APPLE)
    set(LOGOS_PLATFORM_MACOS ON)
    set(LOGOS_PLATFORM_LIBS "")
else()
    set(LOGOS_PLATFORM_LINUX ON)
    set(LOGOS_PLATFORM_LIBS "asound")
endif()

# Compiler detection
if(CMAKE_C_COMPILER_ID MATCHES "GNU|Clang")
    set(LOGOS_COMPILER_GCC_CLANG ON)
    set(LOGOS_COMMON_FLAGS "-Werror" "-Werror=return-type") # "-Wall" "-Wextra"
    set(LOGOS_DEBUG_FLAGS "-g" "-O1" "-DDEBUG")
    set(LOGOS_RELEASE_FLAGS "-Ofast" "-march=native" "-DNDEBUG")
    
    # Sanitizer flags
    if(LOGOS_ADDRESS_SANITIZER)
        if(CMAKE_C_COMPILER_ID MATCHES "Clang")
            set(LOGOS_SANITIZER_FLAGS "-fsanitize=address" "-fno-omit-frame-pointer")
        else()
            set(LOGOS_SANITIZER_FLAGS "-fsanitize=address" "-fno-omit-frame-pointer")
        endif()
        # Set environment variable to ensure proper sanitizer loading
        set(ENV{ASAN_OPTIONS} "detect_leaks=1:abort_on_error=1")
        message(STATUS "Address Sanitizer enabled")
    elseif(LOGOS_THREAD_SANITIZER)
        if(CMAKE_C_COMPILER_ID MATCHES "Clang")
            set(LOGOS_SANITIZER_FLAGS "-fsanitize=thread" "-fno-omit-frame-pointer")
        else()
            set(LOGOS_SANITIZER_FLAGS "-fsanitize=thread" "-fno-omit-frame-pointer")
        endif()
        # Set environment variable to ensure proper sanitizer loading with suppressions
        # Use absolute path to suppressions file
        set(SUPPRESSIONS_FILE "${CMAKE_SOURCE_DIR}/tsan_suppressions.txt")
        set(ENV{TSAN_OPTIONS} "abort_on_error=1:suppressions=${SUPPRESSIONS_FILE}:history_size=5:verbosity=2")
        message(STATUS "Thread Sanitizer enabled with suppressions from: ${SUPPRESSIONS_FILE}")
    endif()
elseif(MSVC)
    set(LOGOS_COMPILER_MSVC ON)
    set(LOGOS_COMMON_FLAGS "/W4" "/WX" "/D_CRT_SECURE_NO_WARNINGS")
    set(LOGOS_DEBUG_FLAGS "/Zi" "/Od" "/DDEBUG")
    set(LOGOS_RELEASE_FLAGS "/O2" "/DNDEBUG")
    
    # Sanitizers not supported on MSVC
    if(LOGOS_ADDRESS_SANITIZER OR LOGOS_THREAD_SANITIZER)
        message(WARNING "Sanitizers are not supported with MSVC compiler")
        set(LOGOS_ADDRESS_SANITIZER OFF)
        set(LOGOS_THREAD_SANITIZER OFF)
    endif()
endif()

# Find required packages
find_package(PkgConfig REQUIRED)

# Build URCU locally with debug options (replaces system pkg_check_modules)
include(ExternalProject)
include(FetchContent)
set(URCU_INSTALL_PREFIX "${CMAKE_BINARY_DIR}/urcu-install")
set(URCU_INCLUDE_DIRS "${URCU_INSTALL_PREFIX}/include")
set(URCU_LIB_DIR "${URCU_INSTALL_PREFIX}/lib")
set(URCU_LIBRARIES "${URCU_LIB_DIR}/liburcu.so" "${URCU_LIB_DIR}/liburcu-cds.so")  # Shared libs; use .a for static if preferred

FetchContent_Declare(
    urcu
    URL https://lttng.org/files/urcu/userspace-rcu-0.14.0.tar.bz2
)

FetchContent_GetProperties(urcu)
if(NOT urcu_POPULATED)
    FetchContent_Populate(urcu)
endif()

set(URCU_DEBUG_FLAGS "")
if(NOT CMAKE_BUILD_TYPE STREQUAL "Release")
    set(URCU_DEBUG_FLAGS "--enable-rcu-debug" "--enable-cds-lfht-iter-debug")
endif()

ExternalProject_Add(urcu
    SOURCE_DIR ${urcu_SOURCE_DIR}
    CONFIGURE_COMMAND <SOURCE_DIR>/configure --prefix=${URCU_INSTALL_PREFIX} ${URCU_DEBUG_FLAGS}
    BUILD_COMMAND make -j${CMAKE_BUILD_PARALLEL_LEVEL}
    INSTALL_COMMAND make install
    LOG_UPDATE ON
    LOG_CONFIGURE ON
    LOG_BUILD ON
    LOG_INSTALL ON
)

# External dependencies using FetchContent

# SDL3 - must be first so other packages can find it
FetchContent_Declare(
    SDL3
    GIT_REPOSITORY https://github.com/libsdl-org/SDL
    GIT_TAG release-3.2.0
)
set(SDL_SHARED OFF CACHE BOOL "Build SDL3 as a static library" FORCE)
set(SDL_STATIC ON CACHE BOOL "Build SDL3 as a static library" FORCE)
FetchContent_MakeAvailable(SDL3)

# Make SDL3 discoverable by other packages
list(APPEND CMAKE_PREFIX_PATH "${sdl3_BINARY_DIR}")

# Set up SDL3 for external packages to find it
set(SDL3_FOUND TRUE CACHE BOOL "SDL3 found" FORCE)
set(SDL3_DIR "${sdl3_BINARY_DIR}" CACHE PATH "SDL3 directory" FORCE)
set(SDL3_INCLUDE_DIRS "${sdl3_SOURCE_DIR}/include" CACHE PATH "SDL3 include directories" FORCE)
set(SDL3_LIBRARIES SDL3::SDL3 CACHE STRING "SDL3 libraries" FORCE)

# Set up all SDL3 components that external packages might look for
set(SDL3_Headers_FOUND TRUE CACHE BOOL "SDL3 Headers found" FORCE)
set(SDL3_SDL3-static_FOUND TRUE CACHE BOOL "SDL3 static library found" FORCE)
set(SDL3_SDL3-shared_FOUND FALSE CACHE BOOL "SDL3 shared library found" FORCE)
set(SDL3_SDL3_FOUND TRUE CACHE BOOL "SDL3 logos library found" FORCE)
set(SDL3_SDL3_test_FOUND FALSE CACHE BOOL "SDL3 test library found" FORCE)

# Verstable
FetchContent_Declare(
    verstable
    GIT_REPOSITORY https://github.com/JacksonAllan/Verstable.git
    GIT_TAG main
)
FetchContent_MakeAvailable(verstable)

# xxHash
FetchContent_Declare(
    xxhash
    GIT_REPOSITORY https://github.com/Cyan4973/xxHash.git
    GIT_TAG v0.8.2
)
FetchContent_MakeAvailable(xxhash)

# Shader dependencies (always enabled)
# SDL3_shadercross (will reuse SPIRV-Headers/SPIRV-Tools)
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

# Makes SPIRV-Headers and SPIRV-Tools available
set(FETCHCONTENT_SOURCE_DIR_SPIRV-Headers "${sdl3_shadercross_SOURCE_DIR}/external/SPIRV-Headers")
set(FETCHCONTENT_SOURCE_DIR_SPIRV-Tools "${sdl3_shadercross_SOURCE_DIR}/external/SPIRV-Tools")

# glslang
FetchContent_Declare(
    glslang
    GIT_REPOSITORY https://github.com/KhronosGroup/glslang
    GIT_TAG vulkan-sdk-1.3.296.0
)
FetchContent_MakeAvailable(glslang)

# shaderc (reuse SPIRV-Headers/SPIRV-Tools)
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

# SPIRV-Reflect
FetchContent_Declare(
    SPIRV-Reflect
    GIT_REPOSITORY https://github.com/KhronosGroup/SPIRV-Reflect
    GIT_TAG main
)
FetchContent_MakeAvailable(SPIRV-Reflect)

# Configure version header
configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/include/version.h.in"
    "${CMAKE_CURRENT_BINARY_DIR}/include/version.h"
)


# Add executables
add_executable(logos
    src/main.c
    src/cpi.c
    src/tklog.c
    src/vec.c
    src/vec_path.c
    src/type.c
    src/map.c
    src/global_data/core.c
    src/global_data/urcu_safe.c
    src/tsm.c)
target_sources(logos PRIVATE ${spirv-reflect_SOURCE_DIR}/spirv_reflect.c)

add_executable(logos_prev
    src/main_2.c
    src/cpi.c
    src/tklog.c
    src/vec.c
    src/vec_path.c
    src/type.c
    src/map.c
    src/global_data/core.c
    src/global_data/urcu_safe.c)
target_sources(logos_prev PRIVATE ${spirv-reflect_SOURCE_DIR}/spirv_reflect.c)

add_executable(test_tklog 
    src/tests/test_tklog.c 
    src/tklog.c)
add_executable(test_urcu_safety 
    src/tests/test_urcu_safety.c
    src/global_data/urcu_safe.c
    src/tklog.c)
add_executable(test_global_data 
    src/tests/test_global_data_2.c 
    src/global_data/core.c 
    src/global_data/urcu_safe.c 
    src/tklog.c)
add_executable(test_asan_demo 
    src/tests/test_asan_demo.c)

# Enable LTO for release builds if supported
if(ipo_supported)
    set_property(TARGET logos PROPERTY INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)
    set_property(TARGET logos_prev PROPERTY INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)
    set_property(TARGET test_tklog PROPERTY INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)
    set_property(TARGET test_urcu_safety PROPERTY INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)
    set_property(TARGET test_global_data PROPERTY INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)
    set_property(TARGET test_asan_demo PROPERTY INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)
endif()

# Apply compiler flags to
if(LOGOS_COMPILER_GCC_CLANG)
    target_compile_options(logos PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}> ${LOGOS_SANITIZER_FLAGS})
    target_compile_options(logos_prev PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}> ${LOGOS_SANITIZER_FLAGS})
    target_compile_options(test_tklog PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}> ${LOGOS_SANITIZER_FLAGS})
    target_compile_options(test_urcu_safety PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}> ${LOGOS_SANITIZER_FLAGS})
    target_compile_options(test_global_data PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}> ${LOGOS_SANITIZER_FLAGS})
    target_compile_options(test_asan_demo PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}> ${LOGOS_SANITIZER_FLAGS} -Wno-stringop-overflow)
elseif(LOGOS_COMPILER_MSVC)
    target_compile_options(logos PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}>)
    target_compile_options(logos_prev PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}>)
    target_compile_options(test_tklog PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}>)
    target_compile_options(test_urcu_safety PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}>)
    target_compile_options(test_global_data PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}>)
    target_compile_options(test_asan_demo PRIVATE ${LOGOS_COMMON_FLAGS} $<$<CONFIG:Release>:${LOGOS_RELEASE_FLAGS}> $<$<CONFIG:Debug>:${LOGOS_DEBUG_FLAGS}>)
endif()


# Include directories
target_include_directories(logos PRIVATE 
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/shaders
    ${verstable_SOURCE_DIR}
    ${spirv-reflect_SOURCE_DIR}
    ${URCU_INCLUDE_DIRS}
)
target_include_directories(logos_prev PRIVATE 
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/shaders
    ${verstable_SOURCE_DIR}
    ${spirv-reflect_SOURCE_DIR}
    ${URCU_INCLUDE_DIRS}
)
target_include_directories(test_tklog PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_include_directories(test_urcu_safety PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include ${URCU_INCLUDE_DIRS})
target_include_directories(test_global_data PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include ${URCU_INCLUDE_DIRS} ${verstable_SOURCE_DIR})

# Compile definitions
target_compile_definitions(logos PRIVATE 
    TKLOG_SHOW_LOG_LEVEL
    TKLOG_SHOW_TIME
    TKLOG_SHOW_THREAD
    TKLOG_SHOW_PATH
    TKLOG_SCOPE
    TKLOG_INFO
    TKLOG_NOTICE
    TKLOG_WARNING
    TKLOG_ERROR
    TKLOG_CRITICAL
    TKLOG_ALERT
    TKLOG_EMERGENCY
    TKLOG_EXIT_ON_ERROR
    TKLOG_EXIT_ON_CRITICAL
    TKLOG_EXIT_ON_ALERT
    TKLOG_EXIT_ON_EMERGENCY
    URCU_LFHT_SAFETY_ON
    RPMALLOC_FIRST_CLASS_HEAPS=1
    $<$<CONFIG:Debug>:DEBUG_RCU>  # Fine-grained RCU debug
    $<$<CONFIG:Debug>:DEBUG_YIELD>  # Add random delays for testing races
)
target_compile_definitions(logos_prev PRIVATE 
    TKLOG_SHOW_LOG_LEVEL
    TKLOG_SHOW_TIME
    TKLOG_SHOW_THREAD
    TKLOG_SHOW_PATH
    TKLOG_SCOPE
    TKLOG_INFO
    TKLOG_NOTICE
    TKLOG_WARNING
    TKLOG_ERROR
    TKLOG_CRITICAL
    TKLOG_ALERT
    TKLOG_EMERGENCY
    TKLOG_EXIT_ON_ERROR
    TKLOG_EXIT_ON_CRITICAL
    TKLOG_EXIT_ON_ALERT
    TKLOG_EXIT_ON_EMERGENCY
    URCU_LFHT_SAFETY_ON
    RPMALLOC_FIRST_CLASS_HEAPS=1
    $<$<CONFIG:Debug>:DEBUG_RCU>  # Fine-grained RCU debug
    $<$<CONFIG:Debug>:DEBUG_YIELD>  # Add random delays for testing races
)
target_compile_definitions(test_tklog PRIVATE 
    TKLOG_SHOW_LOG_LEVEL
    TKLOG_SHOW_TIME
    TKLOG_SHOW_THREAD
    TKLOG_SHOW_PATH
    TKLOG_SCOPE
    TKLOG_MEMORY
    TKLOG_INFO
    TKLOG_NOTICE
    TKLOG_WARNING
    TKLOG_ERROR
    TKLOG_CRITICAL
    TKLOG_ALERT
    TKLOG_EMERGENCY
    TKLOG_EXIT_ON_ERROR
    TKLOG_EXIT_ON_CRITICAL
    TKLOG_EXIT_ON_ALERT
    TKLOG_EXIT_ON_EMERGENCY
)

target_compile_definitions(test_urcu_safety PRIVATE 
    URCU_LFHT_SAFETY_ON
    TKLOG_SHOW_LOG_LEVEL
    TKLOG_SHOW_TIME
    TKLOG_SHOW_THREAD
    TKLOG_SHOW_PATH
    TKLOG_SCOPE
    TKLOG_INFO
    TKLOG_NOTICE
    TKLOG_WARNING
    TKLOG_ERROR
    TKLOG_CRITICAL
    TKLOG_ALERT
    TKLOG_EMERGENCY
    TKLOG_EXIT_ON_ERROR
    TKLOG_EXIT_ON_CRITICAL
    TKLOG_EXIT_ON_ALERT
    TKLOG_EXIT_ON_EMERGENCY
    $<$<CONFIG:Debug>:DEBUG_RCU>
    $<$<CONFIG:Debug>:DEBUG_YIELD>
)
target_compile_definitions(test_global_data PRIVATE 
    URCU_LFHT_SAFETY_ON
    # TKLOG_MEMORY
    # TKLOG_MEMORY_PRINT_ON_EXIT
    TKLOG_SHOW_LOG_LEVEL
    TKLOG_SHOW_TIME
    TKLOG_SHOW_THREAD
    TKLOG_SHOW_PATH
    TKLOG_SCOPE
    TKLOG_INFO
    TKLOG_NOTICE
    TKLOG_WARNING
    TKLOG_ERROR
    TKLOG_CRITICAL
    TKLOG_ALERT
    TKLOG_EMERGENCY
    TKLOG_EXIT_ON_CRITICAL
    TKLOG_EXIT_ON_ALERT
    TKLOG_EXIT_ON_EMERGENCY
    $<$<CONFIG:Debug>:DEBUG_RCU>
    $<$<CONFIG:Debug>:DEBUG_YIELD>
)

# Add output name
if(LOGOS_PLATFORM_WINDOWS)
    set_target_properties(logos PROPERTIES OUTPUT_NAME "logos.exe")
    set_target_properties(logos_prev PROPERTIES OUTPUT_NAME "logos_prev.exe")
    set_target_properties(test_tklog PROPERTIES OUTPUT_NAME "test_tklog.exe")
    set_target_properties(test_urcu_safety PROPERTIES OUTPUT_NAME "test_urcu_safety.exe")
    set_target_properties(test_global_data PROPERTIES OUTPUT_NAME "test_global_data.exe")
    set_target_properties(test_asan_demo PROPERTIES OUTPUT_NAME "test_asan_demo.exe")
else()
    set_target_properties(logos PROPERTIES OUTPUT_NAME "logos")
    set_target_properties(logos_prev PROPERTIES OUTPUT_NAME "logos_prev")
    set_target_properties(test_tklog PROPERTIES OUTPUT_NAME "test_tklog")
    set_target_properties(test_urcu_safety PROPERTIES OUTPUT_NAME "test_urcu_safety")
    set_target_properties(test_global_data PROPERTIES OUTPUT_NAME "test_global_data")
    set_target_properties(test_asan_demo PROPERTIES OUTPUT_NAME "test_asan_demo")
endif()

# Add sanitizer libraries if enabled (must be first)
if(LOGOS_ADDRESS_SANITIZER AND LOGOS_COMPILER_GCC_CLANG)
    target_link_libraries(logos PRIVATE asan)
    target_link_libraries(logos_prev PRIVATE asan)
    target_link_libraries(test_tklog PRIVATE asan)
    target_link_libraries(test_urcu_safety PRIVATE asan)
    target_link_libraries(test_global_data PRIVATE asan)
    target_link_libraries(test_asan_demo PRIVATE asan)
elseif(LOGOS_THREAD_SANITIZER AND LOGOS_COMPILER_GCC_CLANG)
    target_link_libraries(logos PRIVATE tsan)
    target_link_libraries(logos_prev PRIVATE tsan)
    target_link_libraries(test_tklog PRIVATE tsan)
    target_link_libraries(test_urcu_safety PRIVATE tsan)
    target_link_libraries(test_global_data PRIVATE tsan)
    target_link_libraries(test_asan_demo PRIVATE tsan)
endif()

# Link libraries (after sanitizer libraries)
if(WIN32)
    # On Windows, pthread is provided by MinGW-w64 or similar
    target_link_libraries(test_tklog PRIVATE)
else()
    target_link_libraries(test_tklog PRIVATE pthread)
endif()


# Link other libraries
target_link_libraries(logos PRIVATE
    SDL3::SDL3
    xxhash
    ${LOGOS_PLATFORM_LIBS}
    ${URCU_LIBRARIES}
    SDL3_shadercross::SDL3_shadercross
    shaderc_combined
    stdc++
)
target_link_libraries(logos_prev PRIVATE
    SDL3::SDL3
    xxhash
    ${LOGOS_PLATFORM_LIBS}
    ${URCU_LIBRARIES}
    SDL3_shadercross::SDL3_shadercross
    shaderc_combined
    stdc++
)
target_link_libraries(test_urcu_safety PRIVATE ${URCU_LIBRARIES} SDL3::SDL3)
target_link_libraries(test_global_data PRIVATE ${URCU_LIBRARIES} SDL3::SDL3 xxhash)

# Make executables depend on URCU build
add_dependencies(logos urcu)
add_dependencies(logos_prev urcu)
add_dependencies(test_urcu_safety urcu)
add_dependencies(test_global_data urcu)

# Install rules
install(TARGETS logos
    RUNTIME DESTINATION bin
    LIBRARY DESTINATION lib
    ARCHIVE DESTINATION lib
)
install(TARGETS logos_prev
    RUNTIME DESTINATION bin
    LIBRARY DESTINATION lib
    ARCHIVE DESTINATION lib
)

# Install headers if requested
if(LOGOS_INSTALL_HEADERS)
    install(DIRECTORY include/
        DESTINATION include/logos
        FILES_MATCHING PATTERN "*.h"
    )
endif()

# Package configuration
set(CPACK_PACKAGE_NAME "LOGOS")
set(CPACK_PACKAGE_VERSION "${PROJECT_VERSION}")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "LOGOS Application")
set(CPACK_PACKAGE_VENDOR "LOGOS Team")
set(CPACK_PACKAGE_CONTACT "thobknu@gmail.com")

if(LOGOS_PLATFORM_WINDOWS)
    set(CPACK_GENERATOR "NSIS;ZIP")
elseif(LOGOS_PLATFORM_MACOS)
    set(CPACK_GENERATOR "DragNDrop;TGZ")
else()
    set(CPACK_GENERATOR "DEB;RPM;TGZ")
endif()

include(CPack)