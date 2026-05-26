# Koochi

Koochi is built around one core idea: Shift Context Right.

It lets you turn the review rules your team cares about into fast, local, repeatable invariants that can run across hundreds of parallel agents. Instead of burying important review context in `AGENTS.md` files, prompt notes, or tribal knowledge, put it in `koochi.toml` and run it directly against the code that changed.

Shift Context Right means putting durable review knowledge next to the codebase, where it can run automatically. Instead of hoping every agent, reviewer, or prompt remembers the same security rules, reliability checks, and product invariants, Koochi makes those invariants executable.

Each invariant runs as its own isolated agent. Koochi scopes the relevant git change, gives agents read-only code search, shares cached file/search results across the run, and reports deterministic pass/fail results.

That means you can run hundreds of narrow invariants quickly:

- security rules like missing authorization, SQL injection, secret leakage, and tenant data leaks
- reliability rules like missing retries, unbounded background work, cache stampedes, and unsafe file export
- codebase-specific invariants that reviewers normally keep in their heads

Koochi stays local, read-only, and fast. Agents do not get a shell; they get a focused search API. By default, Koochi reviews local changes first, then falls back to the top commit, then to the full tree outside git.

## Install

Install Koochi with Cargo:

```sh
cargo install --git https://github.com/sidhantchadda/Koochi
```

Prebuilt binaries and Homebrew support are planned.

Run Koochi from a repository with a `koochi.toml` file:

```sh
koochi
```

That is the main workflow: write the invariants once, run them from any repo, and get a review result focused on the current local changes or top commit.

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
provider = "openai"
model = "gpt-5-nano"
max_parallel_agents = 128
max_agent_steps = 32
max_parallel_llm_requests = 32
llm_max_retries = 2

[[invariant]]
id = "retry-api-calls"
instruction = "Fail if newly changed API calls can hang or fail transiently without timeout, retry, or backoff handling."
severity = "high"

[[invariant]]
id = "public-handler-auth"
instruction = "Fail if changed public handlers can access account, org, project, or tenant data without an authorization check."
severity = "critical"
```

Existing configs that use `[[test]]` or `tests = [...]` still work. New configs can use `[[invariant]]` or `invariants = [...]` for the same behavior.

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

Use `base_url` with OpenAI-compatible gateways.

For development, run the default local test suite:

```sh
cargo test
```

Live-provider integration tests spend provider quota and require `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`:

```sh
cargo integration
```

Next steps:

- Add commit/diff-aware review modes:
  - `koochi --commit <sha>` to review a specific committed snapshot.
  - `koochi --base <ref> --head <ref>` to review a diff range.
  - `koochi --changed-only` to scope search and evidence to changed files.
- Teach the search layer to read from git snapshots/diffs instead of only the current working tree.
- Include changed-line metadata in agent context so findings can be tied to a code review diff.

## License

Koochi is released under the MIT License. See [LICENSE](LICENSE).
