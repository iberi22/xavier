import os
import sys

CRITICAL_FILES = [
    "AGENTS.md",
    "SOUL.md",
    "MEMORY.md",
    "src/main.rs",
    "Cargo.toml"
]

def audit():
    print("Auditing project context and documentation...")
    missing = []
    for f in CRITICAL_FILES:
        if os.path.exists(f):
            print(f"[OK] Found {f}")
        else:
            print(f"[MISSING] {f}")
            missing.append(f)
            
    if missing:
        print(f"\n[FAIL] Missing {len(missing)} critical files.")
        return False
    
    # Check for memory daily logs
    logs_dir = "memory"
    if os.path.isdir(logs_dir):
        logs = [f for f in os.listdir(logs_dir) if f.endswith(".md")]
        if logs:
            print(f"[OK] Found {len(logs)} daily memory logs.")
        else:
            print("[WARN] No daily memory logs found.")
    else:
        print("[FAIL] Memory directory missing.")
        return False

    print("[OK] Context audit complete.")
    return True

if __name__ == "__main__":
    success = audit()
    sys.exit(0 if success else 1)
