#!/usr/bin/env python3
"""Rebuild the CSL image with REDUCE packages baked in.

Pipeline:
  1. Extract seed image from vendor/csl/image.cpp
  2. Build standalone CSL binary from vendored sources
  3. Download REDUCE package .red sources (if not already vendored)
  4. Run CSL to compile packages into a new image
  5. Convert binary image back to vendor/csl/image.cpp
"""

import os
import re
import shutil
import struct
import subprocess
import sys
import urllib.request
import urllib.error

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BUILD_DIR = os.path.join(ROOT, "build")
VENDOR_CSL = os.path.join(ROOT, "vendor", "csl")
IMAGE_CPP = os.path.join(VENDOR_CSL, "image.cpp")
SEED_IMG = os.path.join(BUILD_DIR, "seed.img")
OUTPUT_IMG = os.path.join(BUILD_DIR, "reduce.img")
CSL_BIN = os.path.join(BUILD_DIR, "csl")
PACKAGES_DIR = os.path.join(ROOT, "vendor", "reduce-packages")
BUILD_SCRIPT = os.path.join(ROOT, "scripts", "build_packages.red")

SVN_BASE = "https://svn.code.sf.net/p/reduce-algebra/code/trunk/packages"

# Packages and their sub-modules (SVN directory, list of .red files).
# Discovered by parsing create!-package calls in each header .red file.
PACKAGES = {
    "ezgcd": ("factor", [
        "ezgcd", "alphas", "coeffts", "ezgcdf", "facmisc", "facstr",
        "interfac", "linmodp", "mhensfns", "modpoly", "multihen", "unihens",
    ]),
    "factor": ("factor", [
        "factor", "bigmodp", "degsets", "facprim", "facmod", "facuni",
        "imageset", "pfactor", "vecpoly", "pfacmult",
    ]),
    "matrix": ("matrix", [
        "matrix", "matsm", "matpri", "matproc", "extops", "bareiss",
        "det", "glmat", "nullsp", "rank", "nestdom", "resultnt",
        "brsltnt_bmap", "cofactor",
    ]),
    "solve": ("solve", [
        "solve", "solve1", "ppsoln", "solvelnr", "glsolve", "solvealg",
        "solvetab", "quartic",
    ]),
    "int": ("int", [
        "int", "contents", "csolve", "idepend", "df2q", "distrib", "divide",
        "driver", "symint", "intfac", "ibasics", "makevars", "jpatches",
        "reform", "simpsqrt", "hacksqrt", "sqrtf", "isolve", "tidysqrt",
        "trcase", "halfangl", "trialdiv", "vect", "dint",
    ]),
    "taylor": ("taylor", [
        "taylor", "tayintro", "tayutils", "tayintrf", "tayexpnd", "taybasic",
        "taysimp", "taysubst", "taydiff", "tayconv", "tayprint", "tayfront",
        "tayfns", "tayrevrt", "tayimpl", "taypart", "taygamma",
    ]),
    "misc": ("misc", ["misc"]),
}


def step(msg):
    print(f"\n{'='*60}")
    print(f"  {msg}")
    print(f"{'='*60}")


# ─── Step 1: Extract seed image from image.cpp ──────────────────────────

def extract_seed_image():
    step("Step 1: Extract seed image from image.cpp")

    with open(IMAGE_CPP, "r") as f:
        content = f.read()

    # Extract the hex string between the first pair of outer parens
    # Format: const unsigned char *reduce_image = reinterpret_cast<const unsigned char *>(
    #    "\xNN\xNN..."
    #    ...);
    # Extract all the string literal contents
    hex_strings = re.findall(r'"((?:\\x[0-9a-fA-F]{2})*)"', content)
    if not hex_strings:
        sys.exit("ERROR: Could not find hex data in image.cpp")

    # Parse hex escapes
    data = bytearray()
    for hs in hex_strings:
        for m in re.finditer(r'\\x([0-9a-fA-F]{2})', hs):
            data.append(int(m.group(1), 16))

    # Extract expected size
    size_match = re.search(r'#define\s+REDUCE_IMAGE_SIZE\s+(\d+)', content)
    if size_match:
        expected = int(size_match.group(1))
        if len(data) != expected:
            print(f"WARNING: Extracted {len(data)} bytes, expected {expected}")
        else:
            print(f"Extracted {len(data)} bytes (matches REDUCE_IMAGE_SIZE)")
    else:
        print(f"Extracted {len(data)} bytes (no size check)")

    # Verify CSL5 magic
    if data[:4] != b'CSL5':
        sys.exit(f"ERROR: Bad magic: {data[:4]!r}, expected b'CSL5'")
    print("CSL5 magic header verified")

    os.makedirs(BUILD_DIR, exist_ok=True)
    with open(SEED_IMG, "wb") as f:
        f.write(data)
    print(f"Wrote seed image to {SEED_IMG}")


# ─── Step 2: Build standalone CSL binary ─────────────────────────────────

def build_standalone_csl():
    step("Step 2: Build standalone CSL binary")

    # Same sources as build.rs, but:
    # - csl.cpp instead of embedcsl.cpp
    # - No reduce_ffi.cpp
    # - No EMBEDDED or BUILTIN_IMAGE defines
    csl_sources = [
        "vendor/csl/cslbase/newallocate.cpp",
        "vendor/csl/cslbase/arith01.cpp",
        "vendor/csl/cslbase/arith02.cpp",
        "vendor/csl/cslbase/arith03.cpp",
        "vendor/csl/cslbase/arith04.cpp",
        "vendor/csl/cslbase/arith05.cpp",
        "vendor/csl/cslbase/arith06.cpp",
        "vendor/csl/cslbase/arith07.cpp",
        "vendor/csl/cslbase/arith08.cpp",
        "vendor/csl/cslbase/arith09.cpp",
        "vendor/csl/cslbase/arith10.cpp",
        "vendor/csl/cslbase/arith11.cpp",
        "vendor/csl/cslbase/arith12.cpp",
        "vendor/csl/cslbase/arith13.cpp",
        "vendor/csl/cslbase/arith14.cpp",
        "vendor/csl/cslbase/bytes1.cpp",
        "vendor/csl/cslbase/char.cpp",
        "vendor/csl/cslbase/csl.cpp",         # NOT embedcsl.cpp
        "vendor/csl/cslbase/cslmpi.cpp",
        "vendor/csl/cslbase/cslread.cpp",
        "vendor/csl/cslbase/eval1.cpp",
        "vendor/csl/cslbase/eval2.cpp",
        "vendor/csl/cslbase/eval3.cpp",
        "vendor/csl/cslbase/eval4.cpp",
        "vendor/csl/cslbase/fasl.cpp",
        "vendor/csl/cslbase/fns1.cpp",
        "vendor/csl/cslbase/fns2.cpp",
        "vendor/csl/cslbase/fns3.cpp",
        "vendor/csl/cslbase/fwin.cpp",
        "vendor/csl/cslbase/newcslgc.cpp",
        "vendor/csl/cslbase/lisphash.cpp",
        "vendor/csl/cslbase/isprime.cpp",
        "vendor/csl/cslbase/preserve.cpp",
        "vendor/csl/cslbase/print.cpp",
        "vendor/csl/cslbase/restart.cpp",
        "vendor/csl/cslbase/sysfwin.cpp",
        "vendor/csl/cslbase/termed.cpp",
        "vendor/csl/cslbase/inthash.cpp",
        "vendor/csl/cslbase/serialize.cpp",
        "vendor/csl/cslbase/stubs.cpp",
        "vendor/csl/cslbase/showhdr.cpp",
        "vendor/csl/cslbase/forks.cpp",
        "vendor/csl/cslbase/gc-check.cpp",
        "vendor/csl/cslbase/jit.cpp",
        "vendor/csl/cslbase/qsieve.cpp",
        "vendor/csl/machineid.cpp",          # NOT reduce_ffi.cpp
    ]

    # Resolve to absolute paths
    sources = [os.path.join(ROOT, s) for s in csl_sources]

    cmd = [
        "g++", "-std=gnu++17", "-O2",
        "-DHAVE_CONFIG_H=1",
        "-DNO_BYTECOUNTS=1",
        "-DAVOID_TERMINAL_THREADS=1",
        f"-I{os.path.join(VENDOR_CSL, 'include')}",
        f"-I{os.path.join(VENDOR_CSL, 'cslbase')}",
        f"-I{VENDOR_CSL}",
    ] + sources + [
        "-o", CSL_BIN,
        "-lz", "-lm", "-lstdc++", "-lpthread",
    ]

    print(f"Compiling {len(sources)} source files...")
    print(f"Command: g++ ... -o {CSL_BIN}")
    result = subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT)
    if result.returncode != 0:
        print("STDOUT:", result.stdout)
        print("STDERR:", result.stderr)
        sys.exit(f"ERROR: Compilation failed with exit code {result.returncode}")

    print(f"Built standalone CSL: {CSL_BIN}")
    os.chmod(CSL_BIN, 0o755)


# ─── Step 3: Download REDUCE package sources ────────────────────────────

def download_package_sources():
    step("Step 3: Download REDUCE package sources")

    os.makedirs(PACKAGES_DIR, exist_ok=True)

    # Download package.map for reference
    package_map_path = os.path.join(PACKAGES_DIR, "package.map")
    if not os.path.exists(package_map_path):
        url = f"{SVN_BASE}/package.map"
        print(f"Downloading package.map...")
        try:
            urllib.request.urlretrieve(url, package_map_path)
        except urllib.error.URLError as e:
            print(f"WARNING: Could not download package.map: {e}")

    for pkg_name, (svn_dir, modules) in PACKAGES.items():
        pkg_dir = os.path.join(PACKAGES_DIR, svn_dir)
        os.makedirs(pkg_dir, exist_ok=True)

        for mod in modules:
            fname = f"{mod}.red"
            dest = os.path.join(pkg_dir, fname)
            if os.path.exists(dest) and os.path.getsize(dest) > 0:
                continue

            url = f"{SVN_BASE}/{svn_dir}/{fname}"
            print(f"  Downloading {svn_dir}/{fname}...")
            try:
                urllib.request.urlretrieve(url, dest)
            except urllib.error.URLError as e:
                print(f"  WARNING: Could not download {svn_dir}/{fname}: {e}")


# ─── Step 4: List modules in seed image ─────────────────────────────────

def list_seed_modules():
    """Run CSL briefly to list available modules in the seed image."""
    step("Step 4: Probe seed image modules")

    # Copy seed to a working image
    shutil.copy2(SEED_IMG, OUTPUT_IMG)

    # Run CSL with a simple probe script
    probe = 'print "PROBE_OK"; quit;\n'
    result = subprocess.run(
        [CSL_BIN, "-w", "-i", SEED_IMG],
        input=probe, capture_output=True, text=True, timeout=30,
        cwd=BUILD_DIR,
    )
    print(f"CSL probe exit code: {result.returncode}")
    if result.stdout:
        # Show first few lines
        for line in result.stdout.splitlines()[:20]:
            print(f"  stdout: {line}")
    if result.stderr:
        for line in result.stderr.splitlines()[:10]:
            print(f"  stderr: {line}")

    return result.returncode == 0 or "PROBE_OK" in result.stdout


# ─── Step 5: Compile packages into image ────────────────────────────────

def compile_packages():
    step("Step 5: Compile packages into image")

    # Copy seed image to output location
    shutil.copy2(SEED_IMG, OUTPUT_IMG)

    # We'll pipe the build script to CSL.
    # CSL with -o opens the output image for writing (faslout + preserve).
    # The -i adds the seed as a readable image.
    # We use the output image as both -i (read existing modules) and -o (write new ones).
    cmd = [
        CSL_BIN, "-w",
        "-i", SEED_IMG,
        "-o", OUTPUT_IMG,
    ]

    with open(BUILD_SCRIPT, "r") as f:
        script = f.read()

    print(f"Running CSL to compile packages...")
    print(f"Command: {' '.join(cmd)}")
    result = subprocess.run(
        cmd,
        input=script,
        capture_output=True,
        text=True,
        timeout=300,  # 5 minutes
        cwd=ROOT,
    )

    print(f"CSL exit code: {result.returncode}")
    if result.stdout:
        lines = result.stdout.splitlines()
        for line in lines[:50]:
            print(f"  stdout: {line}")
        if len(lines) > 50:
            print(f"  ... ({len(lines) - 50} more lines)")
            for line in lines[-20:]:
                print(f"  stdout: {line}")
    if result.stderr:
        for line in result.stderr.splitlines()[:20]:
            print(f"  stderr: {line}")

    if not os.path.exists(OUTPUT_IMG):
        sys.exit("ERROR: Output image was not created")

    img_size = os.path.getsize(OUTPUT_IMG)
    print(f"Output image: {img_size} bytes ({img_size/1024:.1f} KB)")

    return result.returncode


# ─── Step 6: Convert binary image to image.cpp ──────────────────────────

def convert_image_to_cpp():
    step("Step 6: Convert binary image to image.cpp")

    with open(OUTPUT_IMG, "rb") as f:
        data = f.read()

    if data[:4] != b'CSL5':
        sys.exit(f"ERROR: Output image has bad magic: {data[:4]!r}")

    size = len(data)
    print(f"Image size: {size} bytes ({size/1024:.1f} KB)")

    # Generate C++ hex string in same format as original
    lines = []
    lines.append("// Reduce image file data\n")
    lines.append("const unsigned char *reduce_image = reinterpret_cast<const unsigned char *>(\n")

    # Write 16 bytes per line as hex escapes
    for i in range(0, size, 16):
        chunk = data[i:i+16]
        hex_str = "".join(f"\\x{b:02x}" for b in chunk)
        if i + 16 >= size:
            lines.append(f'   "{hex_str}");\n')
        else:
            lines.append(f'   "{hex_str}"\n')

    lines.append(f"\n#define REDUCE_IMAGE_SIZE {size}\n")
    lines.append("\n// End of image file data\n")

    with open(IMAGE_CPP, "w") as f:
        f.writelines(lines)

    cpp_size = os.path.getsize(IMAGE_CPP)
    print(f"Wrote {IMAGE_CPP} ({cpp_size} bytes, {cpp_size/1024:.1f} KB)")


# ─── Main ────────────────────────────────────────────────────────────────

def main():
    print("Rebuild REDUCE Image with Packages")
    print(f"Root: {ROOT}")

    os.makedirs(BUILD_DIR, exist_ok=True)

    extract_seed_image()
    build_standalone_csl()
    download_package_sources()

    # Probe the seed image
    if not list_seed_modules():
        print("WARNING: CSL probe had issues, continuing anyway...")

    compile_packages()
    convert_image_to_cpp()

    step("Done!")
    print(f"New image.cpp written to {IMAGE_CPP}")
    print(f"Run 'cargo build' to verify, then 'cargo test' to validate.")


if __name__ == "__main__":
    main()
