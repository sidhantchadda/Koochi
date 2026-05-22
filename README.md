# Koochi

Koochi is a small Rust project for experimenting with cache-friendly code search APIs for parallel code review agents.

The MVP has three phases:

1. Scoping: define the repo revision, reachable repos, MCP servers, tools, and agents once.
2. Agent execution: agents use a repo-scoped, read-only search API with cache-friendly requests.
3. Synthesis: deterministic result aggregation with no LLM calls.

Each agent runs in an isolated, bounded search loop. On each turn it may call one cached search tool or return a final pass/fail verdict, which lets agents chase down evidence without getting their own shell.

By default Koochi scopes review to the smallest useful git change set:

- if the worktree has local changes, broad search/list operations use those changed files
- otherwise, broad search/list operations use the files changed by `HEAD`
- outside a git repo, Koochi falls back to the whole repository tree

Agents can still read a specific file path when a tool result points them there, but the starting inventory and broad searches stay focused on the review scope.

Run Koochi from a repository with a `koochi.toml` file:

```sh
koochi
```

Use verbose mode when you want config, scope, and per-batch progress details:

```sh
koochi --verbose
```

Use debug mode when you want cache and LLM timing metrics:

```sh
koochi --debug
```

Example config:

```toml
provider = "fake"
model = "gpt-5-nano"
max_parallel_agents = 128
max_agent_steps = 32
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
model = "gpt-5-nano"
```

```toml
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-5"
```

`provider = "fake"` remains the default so local runs and tests do not need network calls. `base_url` can be set for compatible gateways.

LLM bus controls:

- `max_parallel_agents` controls how many agents are prepared and submitted per runner chunk.
- `max_agent_steps` controls how many LLM turns each agent may take before it must return a final verdict. It defaults to `32`.
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

Run live-provider binary-boundary integration tests. These spend provider quota and require
`OPENAI_API_KEY` or `ANTHROPIC_API_KEY`, or a local `.env.local` with the same key names:

```sh
cargo integration
```

They default to OpenAI, and can use Anthropic with `KOOCHI_E2E_PROVIDER=anthropic` and
`ANTHROPIC_API_KEY`.

Run focused live provider E2E tests:

```sh
cargo live-fulfillment-e2e
cargo live-creator-e2e
cargo live-clinic-e2e
cargo live-loop-e2e
```

Run the live provider parallel E2E tests:

```sh
cargo live-parallel-e2e
cargo live-stress-e2e
```

Next steps:

- Add commit/diff-aware review modes:
  - `koochi --commit <sha>` to review a specific committed snapshot.
  - `koochi --base <ref> --head <ref>` to review a diff range.
  - `koochi --changed-only` to scope search and evidence to changed files.
- Teach the search layer to read from git snapshots/diffs instead of only the current working tree.
- Include changed-line metadata in agent context so findings can be tied to a code review diff.
