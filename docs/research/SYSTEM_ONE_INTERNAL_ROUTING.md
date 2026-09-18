# Internal System One Architecture for Xavier

## Overview
This document outlines a proposal for adopting a "System One" architectural pattern natively within Xavier, drawing inspiration from TypeSafe's Jev model and local routing paradigms. The goal is to dramatically reduce latency and inference costs by avoiding unnecessary calls to large language models (System Two) for tasks that can be deterministically or probabilistically resolved by strict, lightweight local components.

## Core Concepts

1. **Fast-Lane Triaging (System One)**
   - Instead of immediately routing every user intent, tool call, or telemetry log through a generative LLM (which incurs high latency and token costs), Xavier should evaluate inputs using a fast, deterministic, or lightweight probabilistic model.
   - Outputs from this system are strictly typed (e.g., JSON schemas or Rust structs) and do not involve token-by-token generation (no strings attached).

2. **Constrained Decoding**
   - For instances where a small local model (e.g., via `candle` or ONNX) is used, we enforce constrained decoding (e.g., outlines or GBNF grammars) to guarantee that the output perfectly matches a predefined schema (e.g., `{ "is_urgent": bool, "confidence": float, "route": string }`).

3. **Fallback to System Two (Fail-Open)**
   - If the fast-lane router encounters low confidence (e.g., `< 0.90`) or an ambiguous state, it fails gracefully by passing the context to the heavier, frontier LLMs (System Two).

## Proposed Internal Integration

### 1. The Fast Router (`FastRouter`)
Located in `src/agents/fast_router_poc.rs` (as a Proof of Concept), this module acts as a gatekeeper for incoming messages and tool-call continuations. It uses strict heuristics or local classifiers to route tasks:
- **Mechanical Continuations:** Tool-steps with no errors are routed to the fastest local logic (`FastLane`), bypassing LLMs.
- **RAG/Telemetry Scrubbing:** Validating whether a log contains PII or if a RAG chunk is relevant can be handled by Boolean logic or small embeddings, evaluated in milliseconds.
- **Complex Prompts:** Sent to the frontier LLM (`SystemTwoLane`).

### 2. Benefits
- **Zero External API Dependency:** We don't need to consume external models like Jev. We extract the architectural concept and implement the logic using Rust and local ML bindings.
- **Cost Efficiency:** Offloading up to ~75% of "mechanical" tool continuations and simple extractions to the Fast Lane.
- **Speed:** Triaging occurs in under 50ms locally.

## Conclusion
By implementing a native "System One" router, Xavier can achieve the speed and reliability of typed, probabilistic decisions without compromising on the depth of the existing frontier model capabilities.
