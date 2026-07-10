# Security Audit — v1.0.0 (#5)

> **Date:** 2026-07-10 · **Scope:** `src/` full review — `unsafe`, secret
> handling, network/protocol, prompt injection, process/file, dependencies
> · **Target:** llm-kernel v0.17.0 (Edition 2024)
>
> Related: ROADMAP v1.0.0 #5 · `SECURITY.md` (policy)

## Verdict

**v1.0.0 security exit criterion (#5): met.** No High-severity findings. Two
Medium findings are documented limitations or have been mitigated in this pass;
none block a 1.0 release. `cargo audit` + gitleaks + SBOM already run in CI
(`ci.yml`).

## Findings

### Medium

**M1 — `BearerAuth::generate()` uses a non-cryptographic PRNG** (`src/mcp/auth.rs`)
The token is drawn from an xorshift PRNG seeded by `SystemTime` nanoseconds +
an `AtomicU64` counter. xorshift is not cryptographically secure; if the seed
is guessable the token is predictable. The code documents this as
"localhost-only transport" use, but `with_generated_auth()` is a public API.

- *Mitigation in place:* already documented as localhost-only in `auth.rs`
  (`generate`'s `# Security Note`); fixed-length token blunts the timing
  channel in `constant_time_eq` (L1).
- *Recommendation (non-blocking, future minor):* for any remote-exposed server,
  use `with_bearer_auth(<externally generated token>)`; a future minor may back
  `generate()` with `getrandom`.

**M2 — HTTP error bodies stored raw in `KernelError::Http`** (`src/llm/client.rs`)
A non-2xx response body was stored verbatim. Some API gateways/proxies echo the
request `Authorization` header inside error bodies, so a caller that logs the
`KernelError` could leak the API key.

- *Mitigated in this pass:* the four construction sites now route the body
  through `redact_http_body`, which applies `mask_secrets` (`src/safety/`)
  when the `safety` feature is enabled. With `safety` off the body passes
  through unchanged (the masking regex is opt-in) — documented limitation.

### Low / Informational

- **L1** — `constant_time_eq` returns early on length mismatch (`mcp/auth.rs`),
  leaking token length via timing. Acknowledged in-code; impact is low (length
  only) and fixed-length tokens neutralise it.
- **L2** — `search_nodes` passes user input straight to FTS5 `MATCH`
  (`graph/search.rs`). Bound parameters prevent SQL injection, but FTS5
  operators (`*`, `OR`, `NEAR`) can cause unexpected matches. Usability, not
  security.
- **I1** — the single `unsafe` block (`embedding/openai.rs:206`,
  `std::env::remove_var`) is inside `#[cfg(test)]` with a correct SAFETY
  comment; required by Rust 2024 for env-var mutation. **No `unsafe` exists in
  production code, no `unsafe fn`, no raw pointer deref.**
- **I2** — no `0.0.0.0` binding anywhere; `serve()` takes a caller-supplied
  `SocketAddr`, tests use `127.0.0.1:0`. Bind scope is the caller's call.
- **I3** — no `std::process::Command`; only `process::id()` (test naming) and
  `process::exit`/`ExitCode` (CLI).
- **I4** — SQL is consistently parameterised (`?`/`params!`); dynamic
  `WHERE`-clause assembly in `graph/search.rs` joins **static literals only**,
  user input goes through `escape_like()` + bind.

## What's done well

- **`SecretVault`** (`secrets/vault.rs`): read-time symlink check (TOCTOU),
  `tempfile`+rename atomic write, Unix `0o600`, `is_valid_env_key` validation,
  ANSI-C quoting encode/decode.
- **`mask_secrets`** (`safety/sanitize.rs`): single-pass regex covering
  Bearer/Basic, `key=value`, `sk-*`, `AKIA*`, `gh[posu]_*`; case-insensitive,
  multi-match.
- **`sanitize_output`**: strips Bidi overrides (U+202A–E), Plane-14 tag chars,
  null bytes, C1 controls, line/paragraph separators — defends against
  text-direction and LLM-injection vectors.
- **`detect_injection`**: low-false-positive design (drops bare `system(`/`eval(`
  rules, requires qualifiers on reveal-prompt); honestly labelled "coarse
  lexical heuristic, not adversarial".
- **LLM client**: connect + overall timeouts, 429 detection; clients do **not**
  derive `Debug`, so `{:?}` logging can't dump keys.
- **MCP auth**: identical Bearer gate on stdio and HTTP transports.

## CI security gates (already active)

| Gate | Where |
|---|---|
| `cargo audit` (advisory DB) | `ci.yml` `audit` job |
| gitleaks (secret scan) | `ci.yml` `secrets` job |
| CycloneDX SBOM | `ci.yml` `sbom` job |
| `cargo-semver-checks` | `semver.yml` (this PR) |
