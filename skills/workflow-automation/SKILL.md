---
name: workflow-automation
description: "Handles CI/CD pipeline automation for Xavier via workflow.ps1. Supports building, evaluating, formatting, checking E2E functionality, and packaging Windows installers. Emulates a /workflow slash command approach."
---

# 🚀 Workflow Automation Skill

This skill defines the operational standard for building, testing, checking, and packaging the **Xavier** application using the centralized `scripts\workflow.ps1` pipeline.

**Why use this skill?**
Professional grade projects employ unified automation to prevent "it works on my machine" issues and broken builds. By enforcing pipeline rules through `workflow.ps1`, agents and humans execute exactly the same checks before deploying or merging.

## ⚙️ Triggering the Pipeline (Slash Commands)

Users may ask you to run `/workflow`, `/build`, `/check`, or `/evaluate`. When they do, map their intent to one of the following commands inside the `e:\scripts-python\xavier` directory.

### 1. **Check (Format, Lint, Clippy)**
Use this to ensure code hygiene and check for warnings.
```powershell
powershell .\scripts\workflow.ps1 check
```
- Includes `cargo fmt --all -- --check`
- Runs `cargo clippy` over backend and UI
- Executes Vitest type checking.

### 2. **Evaluate (Unit Tests + Integration + E2E)**
Use this before concluding any architectural change or PR merge. This tests the 99% coverage metric.
```powershell
powershell .\scripts\workflow.ps1 evaluate
```
- Runs unit tests
- Compiles a background instance of Xavier
- Invokes `.ps1` E2E test suites (like `e2e_system_alerts.ps1`) to verify HTTP/API boundaries and UI integration.

### 3. **Build (Compile and Package)**
Use this to generate the final release binaries and changelogs.
```powershell
powershell .\scripts\workflow.ps1 build
```
- Bumps semantic versioning automatically
- Generates Git changelogs
- Creates the native Windows installers using WiX/Inno (depending on project config).

### 4. **All (Full Pipeline)**
Runs everything sequentially.
```powershell
powershell .\scripts\workflow.ps1 all
```

## 🛠️ Best Practices & Rules

1. **Wait for Lock Release:** Never launch `workflow.ps1` if another instance of `cargo build` or `xavier.exe` is running in the background. Kill zombie processes using `Stop-Process -Name "xavier" -Force` if you encounter `os error 5` (Access Denied) during compilation.
2. **Reviewing Errors:** If the E2E verification fails, check the logs of the background `Xavier` PID spawned during the test to find the root cause (such as a missing feature flag or unavailable port).
3. **Continuous Improvement (Changelogs):** Any build automatically creates changelog diffs based on recent commits. Ensure PRs have descriptive titles.
