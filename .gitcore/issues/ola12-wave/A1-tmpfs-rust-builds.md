# A1: Configure tmpfs Rust builds (/build)

## Problem

Rust compilation writes 2-6GB per full build to disk. Xavier's `target/` has accumulated 90GB.
NVMe SSDs have limited write endurance (300-600 TBW). Current setup writes directly to SSD.

## Solution

Configure tmpfs at `/build` (16GB) and redirect `CARGO_TARGET_DIR` there.

### Steps

1. Add tmpfs mount to NixOS hardware-configuration.nix (reference only — user applies):
```nix
fileSystems."/build" = {
  device = "tmpfs";
  fsType = "tmpfs";
  options = [ "size=16G" "mode=755" "noswap" ];
};
```

2. Update `.cargo/config.toml`:
```toml
[build]
target-dir = "/build/rust-target"
```

3. Add `shell.nix` at project root with `CARGO_TARGET_DIR=/build/rust-target`, mold linker config, and cleanup trap.

4. Verify: `df -h /build` shows 16G tmpfs, `cargo check -p xavier --lib` works from /build.

## Acceptance

- [ ] `.cargo/config.toml` points to `/build/rust-target`
- [ ] `shell.nix` exists with proper env vars
- [ ] `cargo check -p xavier --lib` passes using tmpfs target
- [ ] No SSD writes during compilation (verify with `iotop` or `iostat`)

## Files

- `.cargo/config.toml`
- `shell.nix` (new)

## Notes

- Skill reference: `rust-build-ramdisk`
- Mold linker currently disabled (Bus error in CI) — keep commented
- tmpfs is volatile: reboot clears all build artifacts (intentional)
