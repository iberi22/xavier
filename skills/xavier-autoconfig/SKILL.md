---
name: xavier-autoconfig
description: "Autoconfigures Xavier's environment variables and embedding models based on deterministic hardware analysis and OpenRouter recommendations, removing friction from onboarding."
domains: ["system", "config", "setup", "onboarding"]
author: "Xavier Architecture Team"
version: "1.0.0"
---

# Xavier Autoconfiguration Skill

## Purpose
This skill allows Xavier to inspect its host hardware, detect available VRAM/RAM, and deterministically choose the best embedding models and runtime configuration, removing the need for manual onboarding steps.

## Modus Operandi
1. **Analyze Environment:** Read environment variables (e.g. `XAVIER_EMBEDDING_URL`, `XAVIER_EMBEDDING_MODEL`) and hardware specs using system tooling (e.g., `sysinfo` or system probes).
2. **Determine Capabilities:**
   - If VRAM > 16GB, suggest loading large multimodal models (e.g., Qwen-VL, Local LLaVA) for local artifact indexing.
   - If VRAM is limited, default to Cloud/OpenRouter embedding suggestions (e.g., `nomic-embed-text` or `text-embedding-3-small`).
3. **Notify User/Agents:** Present the auto-configuration profile and adjust the environment automatically.

## Integration & Triggers
When the user or another agent requests system setup, optimization, or "onboarding", trigger this skill to apply deterministic configuration.
