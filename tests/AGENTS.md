# Live Provider Tests

Live provider tests exercise Koochi against fixture repositories with a real LLM provider. They are intentionally more expensive and less routine than unit tests.

## Layout

- Harness entrypoint: `tests/live_provider.rs`
- Harness modules: `tests/live_provider/`
- Fixture projects: `tests/codebases/{language}/{project}`
- Every fixture project root must contain the exact `koochi.toml` or `KOOCHI.TOML` used by its test.

Keep mixed-language fixtures under `tests/codebases/polyglot/{project}`.

## Running

List the live provider tests:

```sh
cargo test -q --test live_provider --features integration-tests -- --list
```

Run one focused live test:

```sh
cargo test -q live_provider_uses_multiple_llm_requests_for_tool_observations --test live_provider --features integration-tests -- --nocapture
```

Run the full live provider target:

```sh
cargo test -q --test live_provider --features integration-tests -- --nocapture
```

The fixture config chooses provider, model, concurrency, step limits, and invariants. The harness only loads the API key named by that fixture config. Set the key in the environment or in `.env.local`.

## Adding A Fixture

1. Create `tests/codebases/{language}/{project}`.
2. Add source files and the fixture-owned `koochi.toml` or `KOOCHI.TOML`.
3. Add a live provider test that references the fixture with `Fixture::Copy { language, name }`.
4. Add the fixture to the referenced fixture list in `tests/live_provider/support.rs`.
5. Keep generated artifacts out of the fixture tree: no fixture-local `target/`, `.koochi/`, or unneeded `Cargo.lock`.

Every fixture project must be referenced by a live provider test. Unused fixtures make the suite harder to reason about and should be removed.
