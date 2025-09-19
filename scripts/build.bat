@echo off
REM Cross-platform build script for Windows

setlocal enabledelayedexpansion

REM Default values
set BUILD_TYPE=Release
set BUILD_TESTS=OFF
set INSTALL_HEADERS=OFF
set CLEAN_BUILD=OFF
set INSTALL=OFF
set PACKAGE=OFF
set BUILD_MAIN_APP=ON
set BUILD_EXTERNAL_DEPS=ON

REM Parse command line arguments
:parse_args
if "%~1"=="" goto :end_parse
if "%~1"=="--debug" (
    set BUILD_TYPE=Debug
    shift
    goto :parse_args
)
if "%~1"=="--release" (
    set BUILD_TYPE=Release
    shift
    goto :parse_args
)
if "%~1"=="--tests" (
    set BUILD_TESTS=ON
    shift
    goto :parse_args
)
if "%~1"=="--no-tests" (
    set BUILD_TESTS=OFF
    shift
    goto :parse_args
)
if "%~1"=="--install-headers" (
    set INSTALL_HEADERS=ON
    shift
    goto :parse_args
)
if "%~1"=="--clean" (
    set CLEAN_BUILD=ON
    shift
    goto :parse_args
)
if "%~1"=="--install" (
    set INSTALL=ON
    shift
    goto :parse_args
)
if "%~1"=="--package" (
    set PACKAGE=ON
    shift
    goto :parse_args
)
if "%~1"=="--help" (
    echo Usage: %0 [OPTIONS]
    echo Options:
    echo   --debug           Build in Debug mode
    echo   --release         Build in Release mode ^(default^)
    echo   --tests           Enable building tests
    echo   --no-tests        Disable building tests ^(default^)
    echo   --install-headers Install header files
    echo   --clean           Clean build directory before building
    echo   --install         Install after building
    echo   --package         Create package after building
    echo   --help            Show this help message
    exit /b 0
)
echo [ERROR] Unknown option: %~1
echo Use --help for usage information
exit /b 1
:end_parse

REM Get script directory
set SCRIPT_DIR=%~dp0
set PROJECT_DIR=%SCRIPT_DIR%..
set BUILD_DIR=%PROJECT_DIR%\build

echo [INFO] Building LOGOS in %BUILD_TYPE% mode
echo [INFO] Project directory: %PROJECT_DIR%
echo [INFO] Build directory: %BUILD_DIR%
echo [INFO] Tests: %BUILD_TESTS%

REM Clean build if requested
if "%CLEAN_BUILD%"=="ON" (
    echo [INFO] Cleaning build directory...
    if exist "%BUILD_DIR%" rmdir /s /q "%BUILD_DIR%"
)

REM Create build directory
if not exist "%BUILD_DIR%" mkdir "%BUILD_DIR%"
cd /d "%BUILD_DIR%"

REM Configure with CMake
echo [INFO] Configuring with CMake...
cmake "%PROJECT_DIR%" ^
    -DCMAKE_BUILD_TYPE="%BUILD_TYPE%" ^
    -DBUILD_TESTING="%BUILD_TESTS%" ^
    -DBUILD_TESTS="%BUILD_TESTS%" ^
    -DBUILD_MAIN_APP="%BUILD_MAIN_APP%" ^
    -DBUILD_EXTERNAL_DEPS="%BUILD_EXTERNAL_DEPS%" ^
    -DLOGOS_INSTALL_HEADERS="%INSTALL_HEADERS%"

if errorlevel 1 (
    echo [ERROR] CMake configuration failed
    exit /b 1
)

REM Build
echo [INFO] Building...
cmake --build . --config "%BUILD_TYPE%"

if errorlevel 1 (
    echo [ERROR] Build failed
    exit /b 1
)

echo [SUCCESS] Build completed successfully!

REM Install if requested
if "%INSTALL%"=="ON" (
    echo [INFO] Installing...
    cmake --install .
    if errorlevel 1 (
        echo [ERROR] Installation failed
        exit /b 1
    )
    echo [SUCCESS] Installation completed!
)

REM Create package if requested
if "%PACKAGE%"=="ON" (
    echo [INFO] Creating package...
    cmake --build . --target package
    if errorlevel 1 (
        echo [ERROR] Package creation failed
        exit /b 1
    )
    echo [SUCCESS] Package created successfully!
)

echo [SUCCESS] All operations completed successfully! 