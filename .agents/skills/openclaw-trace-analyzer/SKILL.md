---
name: openclaw-trace-analyzer
description: "A weakness mining skill that clusters failed execution traces from OpenClaw to identify recurring failure patterns."
---

# OpenClaw Trace Analyzer (Self-Harness Step 1)

This skill operationalizes the "Weakness Mining" stage of the Self-Harness paradigm for OpenClaw bots. It analyzes execution traces of failed tasks to identify reusable failure patterns rather than isolated mistakes.

## 🔍 Core Responsibilities

1. **Trace Ingestion:** Collect execution traces of failed tasks from OpenClaw logs or the Xavier memory store.
2. **Failure Attribution:** For each failed trace, identify:
   - The terminal verifier-level cause (e.g., timeout, missing artifact, assertion failure).
   - The agent-side behavior connected to the failure.
   - The causal status of that behavior within the trace.
3. **Clustering:** Group failed records by exact agreement of their failure signature to form clusters.
4. **Evidence Generation:** Output an "evidence bundle" summarizing the dominant failure patterns, which will be used by the `openclaw-harness-optimizer` skill.

## ⚙️ Operating Procedures

### Step 1: Gather Failed Traces
Query the execution logs for a given bot/model combination. Focus only on traces where the evaluation resulted in a `fail` outcome.

### Step 2: Attribute Failure
Do not treat a "timeout" as the root cause. Investigate *why* it timed out. Did the agent get stuck in a tool-use loop? Did it fail to create a file early on? Document the abstract agent mechanism.

### Step 3: Cluster and Prioritize
Group traces with identical failure mechanisms. Sort the clusters by their support (number of traces in the cluster) and actionability. Strongly favor recurring mechanisms that are likely to be mitigated by a narrow change to the execution protocol or system prompt.

### Step 4: Export Evidence Bundle
Format the findings into an evidence bundle that includes:
- Cluster size
- Representative task instances
- Shared trace symptoms
- Verifier evidence
- Inferred agent mechanism

## 🛠 Tools & Scripts (To Be Implemented)
- `scripts/trace_ingestor.py`: Fetches logs from OpenClaw.
- `scripts/cluster_failures.py`: Groups traces by signature.
