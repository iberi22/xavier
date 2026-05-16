import subprocess
import os
import json
import platform
import datetime

def run_cmd(cmd):
    try:
        result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
        return result.returncode, result.stdout, result.stderr
    except Exception as e:
        return 1, "", str(e)

def get_env_info():
    return {
        "os": platform.system(),
        "os_release": platform.release(),
        "python_version": platform.python_version(),
        "timestamp": datetime.datetime.now().isoformat()
    }

def main():
    print("Starting Deterministic Project Diagnosis...")
    report = {
        "env": get_env_info(),
        "steps": {}
    }
    
    # 1. Environment Audit (Rust)
    rc, stdout, stderr = run_cmd("cargo --version")
    report["steps"]["rust_env"] = {
        "status": "PASS" if rc == 0 else "FAIL",
        "output": stdout.strip() if rc == 0 else stderr.strip()
    }
    
    # 2. Context Audit
    rc, stdout, stderr = run_cmd("python skills/deterministic-diag-ops/scripts/audit_context.py")
    report["steps"]["context_audit"] = {
        "status": "PASS" if rc == 0 else "FAIL",
        "output": stdout.strip()
    }
    
    # 3. Security Audit
    print("Running security audit...")
    rc, stdout, stderr = run_cmd("python skills/deterministic-diag-ops/scripts/security_audit.py")
    report["steps"]["security_audit"] = {
        "status": "PASS" if rc == 0 else "FAIL",
        "output": stdout.strip()
    }
    
    # 4. Strict Static Analysis (Clippy)
    print("Running strict static analysis (clippy)...")
    rc, stdout, stderr = run_cmd("cargo clippy -- -D warnings")
    report["steps"]["strict_analysis"] = {
        "status": "PASS" if rc == 0 else "FAIL",
        "output": stderr.strip()
    }
    
    # 5. Cargo Check
    print("Running cargo check...")
    rc, stdout, stderr = run_cmd("cargo check")
    report["steps"]["cargo_check"] = {
        "status": "PASS" if rc == 0 else "FAIL",
        "output": stderr.strip()
    }
    
    # 6. Technical Debt & Complexity Metrics
    print("Calculating metrics...")
    rc, stdout, stderr = run_cmd("python skills/deterministic-diag-ops/scripts/metrics.py")
    if rc == 0:
        metrics_data = json.loads(stdout)
        report["steps"]["metrics"] = {
            "status": "PASS",
            "data": metrics_data
        }
    else:
        report["steps"]["metrics"] = {
            "status": "FAIL",
            "output": stderr.strip()
        }

    # 7. Memory Verification
    print("Running memory verification...")
    rc, stdout, stderr = run_cmd("python skills/deterministic-diag-ops/scripts/verify_memory.py")
    report["steps"]["memory_verification"] = {
        "status": "PASS" if rc == 0 else "FAIL",
        "output": stdout.strip()
    }
    
    # Generate Markdown Report
    with open("DIAGNOSTIC_REPORT.md", "w", encoding="utf-8") as f:
        f.write("# Deterministic Diagnostic Report\n\n")
        f.write(f"**Date**: {report['env']['timestamp']}\n")
        f.write(f"**OS**: {report['env']['os']} {report['env']['os_release']}\n\n")
        
        for step, data in report["steps"].items():
            status_emoji = "✅" if data["status"] == "PASS" else "❌"
            f.write(f"## {status_emoji} {step.replace('_', ' ').title()}\n")
            if "output" in data:
                f.write(f"```\n{data['output']}\n```\n\n")
            elif "data" in data:
                f.write("### Technical Debt\n")
                debt = data["data"]["technical_debt"]
                for k, v in debt.items():
                    f.write(f"- **{k}**: {v}\n")
                f.write("\n### Top Complex Files\n")
                f.write("| Path | Lines | Functions | Nesting |\n")
                f.write("| --- | --- | --- | --- |\n")
                for item in data["data"]["top_files_by_size"]:
                    f.write(f"| {item['path']} | {item['lines']} | {item['functions']} | {item['max_nesting']} |\n")
                f.write("\n")
            
    # Save JSON data
    with open("diag.json", "w") as f:
        json.dump(report, f, indent=2)
        
    print("\nDiagnosis complete. Report generated: DIAGNOSTIC_REPORT.md")

if __name__ == "__main__":
    main()
