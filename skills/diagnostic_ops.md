# Deterministic Project Diagnosis (deterministic-diag-ops)

High-level protocol for autonomous project diagnosis, system auditing, and deterministic feedback loops in Xavier.

## Overview

This skill provides a suite of tools to diagnose the state of the Xavier project deterministically. It avoids probabilistic guesses by relying on environment audits, memory integrity checks, and structured code analysis.

## Core Components

1.  **Environment Audit**: Captures OS details, Rust/Cargo versions, and path configurations.
2.  **Memory Verification**: Communicates with the local Xavier engine (`localhost:8003`) to verify that the memory layer is responsive and consistent.
3.  **Security Audit**: Scans for known vulnerabilities in dependencies using `cargo audit`.
4.  **Strict Static Analysis**: Enforces zero-warning policy using `cargo clippy`.
5.  **Codebase Diagnosis**: Runs `cargo check` and `cargo test`, parsing the output into a structured JSON/Markdown report.
6.  **Context Alignment**: Ensures that the codebase structure matches the core documentation (`AGENTS.md`, `MEMORY.md`).

## Usage

### Run Full Diagnosis

To run a complete project diagnosis and generate a report:

```powershell
python skills/deterministic-diag-ops/scripts/diag.py
```

### Verify Memory Integrity

To check only the memory layer:

```powershell
python skills/deterministic-diag-ops/scripts/verify_memory.py
```

### Audit Documentation Alignment

To check if documentation and code are in sync:

```powershell
python skills/deterministic-diag-ops/scripts/audit_context.py
```

## Output Artifacts

- **`DIAGNOSTIC_REPORT.md`**: A human-readable summary of the project's health.
- **`diag.json`**: A machine-readable state representation for automated processing.

## Best Practices

- **Baseline First**: Always run a diagnosis before starting a major refactor to establish a deterministic baseline.
- **Isolate Failures**: Use the individual scripts to isolate whether a failure is in the environment, memory, or code logic.
- **Durable Feedback**: Store the results of critical diagnoses in Xavier's memory for long-term health tracking.
