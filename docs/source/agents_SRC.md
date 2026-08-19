# agents Module SRC

## Overview

Documentation for the `agents` module.

## Dependencies

This module depends on the following modules:
- [checkpoint](checkpoint_SRC.md)
- [codebase](codebase_SRC.md)
- [context](context_SRC.md)
- [coordination](coordination_SRC.md)
- [data_commons](data_commons_SRC.md)
- [domain](domain_SRC.md)
- [memory](memory_SRC.md)
- [observability](observability_SRC.md)
- [ports](ports_SRC.md)
- [retrieval](retrieval_SRC.md)
- [scheduler](scheduler_SRC.md)
- [search](search_SRC.md)
- [secrets](secrets_SRC.md)
- [settings](settings_SRC.md)
- [utils](utils_SRC.md)

## Components

- `agents/anomaly_scanner.rs`
- `agents/extraction.rs`
- `agents/cve_learner.rs`
- `agents/supervisor.rs`
- `agents/curation.rs`
- `agents/self_harness_coordinator.rs`
- `agents/ui_render.rs`
- `agents/belief_evaluator.rs`
- `agents/runtime.rs`
- `agents/self_improve.rs`
- `agents/rate_limit.rs`
- `agents/system2.rs`
- `agents/unregister_agent_handler.rs`
- `agents/registry.rs`
- `agents/mod.rs`
- `agents/system1.rs`
- `agents/router.rs`
- `agents/system3/types.rs`
- `agents/system3/client.rs`
- `agents/system3/engine.rs`
- `agents/system3/mod.rs`
- `agents/system3/tests.rs`
- `agents/system3/helpers/nlp.rs`
- `agents/system3/helpers/text.rs`
- `agents/system3/helpers/date.rs`
- `agents/system3/helpers/mod.rs`
- `agents/evolve/researcher.rs`
- `agents/evolve/integrator.rs`
- `agents/evolve/mutator.rs`
- `agents/evolve/reflector.rs`
- `agents/evolve/gap_analyzer.rs`
- `agents/evolve/experiment.rs`
- `agents/evolve/config.rs`
- `agents/evolve/results.rs`
- `agents/evolve/evaluator.rs`
- `agents/evolve/mod.rs`
- `agents/evolve/tests.rs`
- `agents/provider/types.rs`
- `agents/provider/client.rs`
- `agents/provider/anthropic.rs`
- `agents/provider/router_tests.rs`
- `agents/provider/model_manager.rs`
- `agents/provider/gemini.rs`
- `agents/provider/openai.rs`
- `agents/provider/llama_cpp.rs`
- `agents/provider/config.rs`
- `agents/provider/rate_limit.rs`
- `agents/provider/minimax.rs`
- `agents/provider/local.rs`
- `agents/provider/hardware.rs`
- `agents/provider/traits.rs`
- `agents/provider/mod.rs`
- `agents/provider/tests.rs`
- `agents/provider/router.rs`
- `agents/hormer/persistence_test.rs`
- `agents/hormer/mod.rs`
- `agents/hormer/reward.rs`
- `agents/hormer/tests.rs`
