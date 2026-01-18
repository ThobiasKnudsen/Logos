# Vendor Dependencies

This folder contains vendored dependencies as git submodules with Zig 0.16 compatibility fixes.

## SDL_shadercross_zig

**Why vendored:** Upstream has Zig 0.14/0.15 syntax that's incompatible with Zig 0.16+

**Source:** https://github.com/Beyley/SDL_shadercross_zig

**Setup:**
```bash
git submodule update --init --recursive
```

**Fixes applied for Zig 0.16 compatibility:**

### In `build.zig.zon`:
1. Line 2: `.name = "SDL_shadercross_zig"` → `.name = .SDL_shadercross_zig` (enum literal)
2. Line 9: `.@"SPIRV-Cross_zig"` → `.spirv_cross_zig` (valid identifier)
3. Line 18: `.@"SPIRV-Headers"` → `.spirv_headers` (valid identifier)

### In `build.zig`:
1. Line 26: `"SPIRV-Headers"` → `"spirv_headers"`
2. Line 43: `"SPIRV-Cross_zig"` → `"spirv_cross_zig"`

**Update submodule:**
```bash
cd vendor/SDL_shadercross_zig
git pull origin master
cd ../..
# Then reapply fixes above
```

**Commit changes:**
```bash
git add vendor/SDL_shadercross_zig
git commit -m "Apply Zig 0.16 compatibility fixes to SDL_shadercross_zig"
```

## Alternative: Fork upstream

If you prefer to maintain your own fork:
1. Fork https://github.com/Beyley/SDL_shadercross_zig
2. Apply the fixes above and push
3. Update `build.zig.zon` to point to your fork:
   ```zig
   .SDL_shadercross_zig = .{
       .url = "git+https://github.com/YOUR_USERNAME/SDL_shadercross_zig#main",
       .hash = "...",
   },
   ```
