# Submission and receipt safety closeout — 2026-11-12

## Completed implementation increments

| Commit | Scope |
|---|---|
| `80463e8` | Preserve typed deployment failure evidence and dominant batch no-replay guidance. |
| `382de2f` | Fail closed on deployment prerequisites and shortcut inventory/mutation failures; retain inter-tier errors, stop later hooks and avoid duplicate framing. Drain already-started parallel outcomes. |
| `f369e4c` | Share exact-job receipt tracking across notebook/dataflow/dbt/scheduler/copy/pipeline/Spark runners. Surface persistence failures; retain acknowledgement and nonterminal cancellation requests. |
| `cabddd9` | Bounded, explicit-scope, append-only local import of original retained receipts. Never repair corrupt history, overwrite conflicts, submit work or promote imported claims to completion. |
| `9341664` | Preserve evidence in specialized raw/octet/binary/multipart/ARM senders and OneLake upload/copy/rename/delete stages; stop unsafe fallbacks and pin observation URLs. |
| `ea4d29c` | Publish a syntax-checked readonly post-Gen1 acceptance workflow using existing observation primitives, with explicit first-refresh/data/freshness/incremental/consumer gates. |
| `0d440de` | Reject contradictory job completion claims, bound cancellation observation and distinguish binary acceptance from completed downloads. |

No live upgrade, refresh, takeover, grant, resource creation, cancellation or tenant
configuration mutation was performed for these increments. Commits are local;
no push or release was requested or performed.

## Local validation

- `cargo fmt -- --check`: passed.
- `cargo clippy --tests -- -D warnings`: passed.
- `cargo test`: **2,967 passed, 0 failed, 1,126 ignored** across 116 suites. Ignored
  live/runtime audits are not included in the pass count.
- `npm run check`, `npm test` (13 tests), `npm run build` in `docs/`: passed.
- `python3 -m unittest discover -s tests/eval -p 'test_*.py'`: **16 passed**.
- Evaluation YAML, six targeted safety cases and their skill references parse.
  This is structural validation, **not an LLM evaluation**.
- Source audit: **zero discarded `let _ = JobLedger::...` results** remain.
- Final cross-target lint after completion-claim checks passed on Linux x64/ARM64,
  macOS x64/ARM64 and Windows x64: **5 passed, 0 failed, 0 skipped**.
- Pre-commit hooks ran and passed on each implementation commit. The installed
  prek version reports the pre-existing unrecognized `auto_update` configuration
  key; hooks were not bypassed.

## Blocked validation — not passed, not silently waived

### Model safety evaluation

Both selected safety-case and full-corpus evaluation preflights were attempted.
They exited 1 before contacting a provider; the numeric summaries remain
`accepted:false`. Required configuration is absent:

- `AZURE_OPENAI_API_BASE_URL`
- `AZURE_OPENAI_DEPLOYMENT_NAME`
- `AZURE_OPENAI_MODEL_NAME`
- `AZURE_OPENAI_API_KEY`

No model pass rate or generated/rubric result is claimed. Do not replace these with
self-grading, cached results, a fabricated provider or an unapproved deployment.

### Live functional acceptance

The earlier disposable upgrade fixtures were deleted and their upgrade approvals
were consumed. No current approved post-upgrade fixture plus retained baseline
and separately approved refresh/consumer scope was supplied for this run.
Consequently, actual first-refresh completion, loaded/fresh destination data,
incremental-setting parity and downstream reader/visual acceptance remain
**not_checked**. The readonly acceptance workflow is implemented and mock-tested;
that does not establish live business readiness.

Existing positive authorized admin-hygiene, ownership/provisioned-identity, Spark
activation and ARM resume validation also remain fixture/authorization-dependent.
No resources were provisioned or settings/roles changed to manufacture coverage.

## Deliberate contract boundaries

This closes the listed implementation increments, not a universal exactly-once
certificate for the entire CLI. See `.agents/POST-RETRY-AUDIT.md` for the precise
transport inventory. Custom HTTP/MCP/database SDK transports and other batch
aggregators retain their own contracts; no global idempotence claim is made.
Cross-compilation lint does not replace Windows/macOS runtime CI or hardware
power-loss testing. Local receipt import cannot establish missing remote identity
or remote completion, and the post-upgrade workflow never executes repairs.

The remaining acceptance blockers are explicit above. Never label this milestone
fully live-validated merely because deterministic checks and compilation pass.
