# A2: Clean stale build artifacts across proyectoSWAL

## Problem

Rust `target/` directories accumulate across projects. Total: ~141GB of build artifacts
on a disk at 87% capacity (747GB/907GB). Xavier alone: 90GB target/ + 15GB target_local/.

## Solution

Remove stale build artifacts. These are fully regenerable from source.

### Directories to clean

| Directory | Size | Safe to delete? |
|-----------|------|-----------------|
| `xavier/target/` | 90 GB | YES — rebuilds from source |
| `xavier/target_local/` | 15 GB | YES — alternate build dir |
| `xavier/scratch/` | 70 MB | YES — old runtime experiments |
| `synapse-trading/target/` | 25 GB | YES |
| `gestalt/target/` | 8.2 GB | YES |
| `pgheart/target/` | 2.9 GB | YES |

**Total recovery: ~141 GB**

### Steps

1. Dry run first: `find ~/proyectosSWAL -name "target" -type d -maxdepth 2 -exec du -sh {} \;`
2. Clean Xavier: `rm -rf ~/proyectosSWAL/xavier/target ~/proyectosSWAL/xavier/target_local`
3. Clean scratch: `rm -rf ~/proyectosSWAL/xavier/scratch/runtime-*`
4. Clean other projects: `rm -rf ~/proyectosSWAL/synapse-trading/target ~/proyectosSWAL/gestalt/target ~/proyectosSWAL/pgheart/target`
5. Verify: `df -h /` shows freed space

## Acceptance

- [ ] `df -h /` shows ≥150GB free (was 114GB)
- [ ] `cargo check -p xavier --lib` still passes after rebuild
- [ ] No other project broken

## Files

- Shell commands only (no source changes)
