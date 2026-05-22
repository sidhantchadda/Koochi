# Koochi

Koochi is a small Rust project for experimenting with cache-friendly code search APIs for parallel code review agents.

The MVP has three phases:

1. Scoping: define the repo revision, reachable repos, MCP servers, tools, and agents once.
2. Agent execution: agents use a repo-scoped, read-only search API with cache-friendly requests.
3. Synthesis: deterministic result aggregation with no LLM calls.

Run Koochi from a repository with a `koochi.toml` file:

```sh
koochi
```

Example config:

```toml
provider = "fake"
model = "gpt-5.4-nano"
max_parallel_agents = 128
max_parallel_llm_requests = 32
llm_max_retries = 2

tests = [
  "Check whether API calls need retry handling.",
  "Check for missing authorization checks on public handlers."
]

[[test]]
id = "critical-paths"
instruction = "Check critical paths for unhandled errors."
severity = "high"
```

Provider config is intentionally small:

```toml
provider = "openai"
api_key_env = "OPENAI_API_KEY"
model = "gpt-5.4-nano"
```

```toml
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-5"
```

`provider = "fake"` remains the default so local runs and tests do not need network calls. `base_url` can be set for compatible gateways.

LLM bus controls:

- `max_parallel_agents` controls how many agents are prepared and submitted per runner chunk.
- `max_parallel_llm_requests` controls how many provider requests the managed LLM bus may keep in flight at once. It defaults to `max_parallel_agents`.
- `llm_max_retries` controls retry attempts for transient provider failures such as rate limits, server errors, and transport errors. It defaults to `2`.

The agent-facing search API is deliberately boring:

- `list_files`
- `search_text`
- `read_file`
- `get_file_context`
- `find_definitions`
- `find_references`

The repo is not repeated in every request. It lives in `ScopeConfig`, which is attached to a `KoochiSession`.

The LLM layer is provider-agnostic behind a `LlmBus` trait. The current adapters cover the deterministic fake bus, OpenAI-compatible chat completions, and Anthropic messages.

Source layout:

```text
src/agents/   agent execution and verdict domain types
src/cli/      command-line runner
src/config/   koochi.toml parsing and normalization
src/llm/      provider-agnostic bus plus fake/OpenAI/Anthropic adapters
src/prompts/  prompt builders
src/scope/    repo/revision/session scope
src/search/   cache-friendly search API and local implementation
src/synthesis deterministic report aggregation
```

Run the default cheap test suite:

```sh
cargo test
```

Run deterministic binary-boundary integration tests:

```sh
cargo integration
```

Run opt-in live provider E2E tests. They default to OpenAI, and can use Anthropic with `KOOCHI_E2E_PROVIDER=anthropic` and `ANTHROPIC_API_KEY`.

```sh
cargo live-fulfillment-e2e
cargo live-creator-e2e
cargo live-clinic-e2e
```

Run the opt-in live provider polyglot parallel E2E test:

```sh
cargo live-parallel-e2e
```
