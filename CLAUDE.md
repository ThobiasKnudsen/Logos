# Logos

## Build

Zig project. Build with `zig build`, run with `zig build run`.

## Worktree + Submodule Setup

This repo uses git worktrees (`main`, `managing_files`, etc.) and has vendor submodules with **local-only commits** (not pushed upstream). After creating a new worktree, submodules will be empty and `zig build` will fail with errors like:

```
unable to find artifact 'SDL_shadercross'
```

### Fix: Initialize submodules in a new worktree

You must run **both steps** in order:

**Step 1 — Clone the submodule repos** (this fetches upstream but can't get the local-only commits yet):

```bash
git submodule init && git submodule update
```

**Step 2 — Fetch local-only commits from main's gitdir and checkout the correct refs:**

```bash
for sub in vendor/SDL_shadercross_zig vendor/SPIRV-Cross_zig; do
    main_gitdir="/home/o/Personal/Code/Logos/main/.git/modules/$sub"
    worktree_gitdir="$(git -C "$sub" rev-parse --git-dir)"
    GIT_DIR="$worktree_gitdir" git fetch "$main_gitdir"
    expected="$(git ls-tree HEAD "$sub" | awk '{print $3}')"
    git -C "$sub" checkout "$expected"
done
```

**Why two steps?** `git submodule update` alone will fail with `fatal: remote error: upload-pack: not our ref ...` because the submodules point to commits that only exist locally in `main`'s gitdir. Step 1 clones the repos so they exist on disk, then Step 2 fetches the missing commits from main.

### Removing a worktree with submodules

`git worktree remove <name>` will fail with `fatal: working trees containing submodules cannot be moved or removed`. Instead, manually delete and prune:

```bash
rm -rf ../<worktree_name> && git worktree prune
```
