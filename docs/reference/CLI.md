# LLM configuration and unattended setup

`xavier llm` configures and probes the general language model or a separate model
for explanations. Every subcommand accepts `--json` and `--scope general|explain`
(default: `general`). JSON output is one document on stdout; diagnostics and
restart hints go to stderr. Responses include `schema_version: 1` and `ok`.

```sh
xavier llm status --json
xavier llm set --provider local --model llama3.2 --url http://127.0.0.1:11434/v1 --json
xavier llm set --provider openai --model gpt-4.1-nano --api-key-file /run/secrets/openai-key --json
xavier llm set --provider anthropic --model claude-haiku-4-5 --scope explain --api-key-stdin --json < /run/secrets/explain-key
xavier llm list-models --json
xavier llm test --scope explain --json
xavier setup --local --model llama3.2 --yes --json
```

`status` reports the effective provider, model, base URL, each setting's source,
and API key presence (`set` or `unset`, never its value). Sources are `default`,
`config`, `runtime`, `env:NAME`, `vault`, or `unset`. The local Ollama placeholder
is not a configured key. `daemon.reachable` and `daemon.llm_configured` describe
`/health` at the configured Xavier URL; `llm_configured` is null when unavailable.
The daemon health field describes its general LLM, including for explain scope.
CLI configuration and the running daemon can differ until the daemon restarts.

`set` requires `--provider local|openai|openrouter|groq|anthropic` and a nonempty
`--model`. `--url` supplies an HTTP(S) provider API root, normally ending in `/v1`
(or `/openai/v1` for Groq). Credentials, query strings, and fragments are refused
in URLs. Settings go through `XavierSettings::save()` to the runtime state file
(`XAVIER_RUNTIME_STATE_PATH` overrides its location), never the config file.
Restart the daemon after changes. Existing environment variables take precedence
and must be updated or removed in the daemon environment to use saved settings.
Explain settings are independent; until an explain provider is selected,
explanations inherit the general provider.

API keys are accepted only through mutually exclusive `--api-key-file PATH` or
`--api-key-stdin`. They are stored in the existing encrypted provider vault,
never runtime JSON. A vault failure aborts with a hint naming the provider's key
environment variable. Explain keys use a separate vault entry. The existing
`XAVIER_EXPLAIN_API_KEY_FILE` override remains supported. No key argument is
accepted on the command line.

`list-models` returns `models`, an array of IDs from the provider's `/models`
endpoint. Local servers must expose the OpenAI-compatible API (Ollama supports
`/v1/models`). `test` first lists models, then submits a one-token completion
(`/chat/completions` or Anthropic `/messages`). Completion text and remote error
bodies are never printed. `--timeout-secs` bounds each HTTP request (default 10,
range 1–120). These commands contact the provider directly, not the Xavier daemon.

Unattended `setup --local --model ID` writes local runtime settings without
prompts, model downloads, shell changes, or `.env` edits. It is selected by
`--yes`, `--json`, or non-terminal stdin. Missing `--local` or `--model` exits 2
with a usage hint. Install models separately and use `llm test` to check them.
The existing interactive wizard remains available with terminal stdin.

Exit codes: 0 success; 1 operation/protocol failure; 2 invalid or missing input;
3 authentication failure (HTTP 401/403); 4 provider connection failure or timeout.
For JSON operation failures, stdout contains `ok: false` and a safe `error`
message. Argument parsing errors use stderr and exit 2.
