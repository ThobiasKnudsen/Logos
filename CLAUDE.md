# Logos

Zig project using git worktrees and vendor submodules.

## Build & Run

**BEFORE running `zig build`, you MUST ensure submodules are initialized.**
Skipping this will crash with `unable to find artifact 'SDL_shadercross'`.

### Check if submodules are already set up

```bash
ls vendor/SDL_shadercross_zig/build.zig vendor/SPIRV-Cross_zig/build.zig 2>/dev/null
```

If both files exist, skip to **Build**. Otherwise, run the setup below.

### Submodule setup

```bash
git submodule init && git submodule update
```

**On worktrees** this will fail with `fatal: remote error: upload-pack: not our ref ...` — that's expected. The submodules reference commits that only exist in `main`'s local gitdir. After the error, run this to fetch those commits:

```bash
main_worktree="$(git worktree list | head -1 | awk '{print $1}')"
for sub in vendor/SDL_shadercross_zig vendor/SPIRV-Cross_zig; do
    main_gitdir="$main_worktree/.git/modules/$sub"
    worktree_gitdir="$(git -C "$sub" rev-parse --git-dir)"
    GIT_DIR="$worktree_gitdir" git fetch "$main_gitdir"
    expected="$(git ls-tree HEAD "$sub" | awk '{print $3}')"
    git -C "$sub" checkout "$expected"
done
```

On `main` (not a worktree), `git submodule update` should succeed and the above block is not needed.

### Build

```bash
zig build        # build only
zig build run    # build and run
```

## Removing a worktree with submodules

`git worktree remove <name>` will fail with `fatal: working trees containing submodules cannot be moved or removed`. Instead, manually delete and prune:

```bash
rm -rf ../<worktree_name> && git worktree prune
```
