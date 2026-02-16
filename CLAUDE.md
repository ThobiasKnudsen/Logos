# Logos

## Build

Zig project. Build with `zig build`, run with `zig build run`.

## Worktree + Submodule Setup

This repo uses git worktrees (`main`, `managing_files`, etc.) and has vendor submodules with **local-only commits** (not pushed upstream). After creating a new worktree, submodules will be empty or fail to init because the custom commits only exist in `main`'s submodule gitdir.

**Run this after creating a new worktree:**

```bash
for sub in vendor/SDL_shadercross_zig vendor/SPIRV-Cross_zig; do
    main_gitdir="/home/o/Personal/Code/Logos/main/.git/modules/$sub"
    worktree_gitdir="$(git -C "$sub" rev-parse --git-dir)"
    GIT_DIR="$worktree_gitdir" git fetch "$main_gitdir"
    expected="$(git ls-tree HEAD "$sub" | awk '{print $3}')"
    git -C "$sub" checkout "$expected"
done
```
