import subprocess
import sys

def run_cmd(cmd):
    try:
        result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
        return result.returncode, result.stdout, result.stderr
    except Exception as e:
        return 1, "", str(e)

def audit():
    print("Running security audit...")
    
    # Check if cargo-audit is installed
    rc, stdout, stderr = run_cmd("cargo audit --version")
    if rc != 0:
        print("[WARN] cargo-audit is not installed. Skipping security scan.")
        print("To install: cargo install cargo-audit")
        return True # Don't fail the whole diag for a missing tool
        
    print("Scanning dependencies for vulnerabilities...")
    rc, stdout, stderr = run_cmd("cargo audit")
    if rc == 0:
        print("[OK] No known vulnerabilities found.")
        return True
    else:
        print("[FAIL] Vulnerabilities detected!")
        print(stdout)
        return False

if __name__ == "__main__":
    success = audit()
    sys.exit(0 if success else 1)
