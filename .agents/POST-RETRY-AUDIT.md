# POST submission retry audit

Source audit and hermetic fault-injection findings; no live submission was replayed
or mutated to manufacture an ambiguous failure. This is a bounded audit, not a
certificate that every workload-level fallback or external SDK is idempotent.

## Helper inventory and classification

| Transport/helper | Existing behavior found | Change / retained boundary |
|---|---|---|
| Fabric `post` / `post_empty` → `post_impl` | Shared unconditional 502/503/504 replay with read-only operations; 401 refresh and three throttling retries | Explicit Mutation vs ReadOnly semantics. Mutation gateway replay removed. Known auth/throttle rejection retries retained only without contradictory ACK markers. |
| Fabric `post_readonly` | Same generic gateway/throttle retry loop | Retained for explicitly read-only operations. Shared CRUD definition retrieval uses new read-only LRO helper; catalog search explicitly opts in. Other bespoke definition callers are conservatively not all migrated. |
| Fabric `post_fabric_once` | One attempt including 401/429/5xx; retains status/headers across body failure | Gen1 strict sender unchanged, no new rejection retry. |
| Fabric `post_with_timeout` | One auth refresh, no gateway retries; possible retry-encouraging poll failure hint | Shared JSON sender and contextual error evidence; accepted observation failure does not authorize replay. |
| Notebook / generic item / path-based job triggers | Notebook one attempt; other triggers one auth refresh; no gateway retries | Attempt policy retained. Typed submission errors retain accepted/unknown state and requested route IDs. Generic trigger delegates to the path transport instead of duplicating it. Job Location validation/correlation is not redesigned here. |
| Power BI JSON / metadata refresh | One auth refresh, no gateway retries; receipt-aware refresh already retains unreadable success ACKs | JSON sender distinguishes mutation from DAX read semantics. Mutation errors retain evidence and suppress replay hints; successful refresh acknowledgement handling remains intact. |
| Power BI DAX query | Explicit semantic read; auth refresh, no generic gateway replay | Existing read policy unchanged; no mutation evidence attached. |
| Fabric raw text / octet-stream / bytes / bytes-with-Accept | Auth refresh but no generic 5xx loop | Shared uncertainty evidence for transport/body/LRO failures; ACK markers block auth replay. Binary observation URLs are pinned to the configured service; accepted responses without a usable observation URL fail as unverified. |
| Power BI bytes / multipart | Bytes auth refresh; consumed multipart form is not retried | Typed uncertainty evidence retained, including multipart 401+ACK; no multipart replay. |
| ARM POST/PUT/PATCH/DELETE | One attempt; optional GET-only LRO observation | Shared uncertainty evidence; Azure-AsyncOperation is an ACK marker, not an operation ID. Poll URLs are pinned to the configured service; no accepted-without-observation success claim. |
| OneLake upload / copy / rename / delete | Upload ignored status failures; rename treated unrelated errors as unsupported; copy acceptance could trigger source deletion | Check every upload step and stop before the next on failure; retain compound no-replay guidance. Rename fallback requires a recognized definitive unsupported rejection. Copy needs unique x-ms-copy-status success before consumers can treat it as completed or delete a source. |
| Fabric JSON PATCH/PUT/DELETE and raw PUT | One auth refresh, no gateway retry | Shared uncertainty evidence and ACK-aware auth refresh. |
| Other OneLake / custom HTTP / MCP / database SDK paths | Separate transport and operation semantics | No blanket SDK/global retry switch or invented idempotency key; this audit does not certify external transports. |

## Command-level loops

Read-only classification is an internal semantic choice, not a user-selected URL
escape hatch. The shared definition getter validates workspace/item UUIDs before
constructing its read-only POST route, with no-request path/query-injection tests.

Three explicit mutation retry loops were found in the inspected command modules:
deployment item name reservation, new mirrored-database definition readiness, and
Git initialization. Their existing named rejection checks now additionally require
typed errors without submission uncertainty and without `retriable:false`. A 503
whose text happens to contain a familiar rejection marker cannot trigger another
POST. Bulk loops over distinct actions and GET-only polling are not automatically
equivalent to resubmitting the same mutation.

Git checkout also used to reconnect the previous branch after any failed reconnect
reply, including 5xx. That compensation is now blocked on ambiguity; it could
otherwise undo a switch the server already accepted. Hermetic tests preserve
rollback on a definitive 400 rejection and separately assert no retry, rollback or
initialization after an uncertain 503 reconnect.

## Evidence and preservation contract

See `src/client/submission.rs`, `errors::SubmissionEvidence`, and the user-facing
`submission-retries` guide/best-practice. Optional `error.submission` is bounded
transport evidence, not a verified scoped ledger receipt. API error metadata is
preserved while local replay safety overrides retriability; the server hint remains
separate. No raw request body, query string or returned URL is added to evidence.

Deployment apply now preserves typed action/recorded-hook errors through the same
canonical serializer as stderr (`errorDetails`), retaining legacy text/codes. Every
failed action/recorded hook carries `retryAllowed:false`; the batch no-replay hint
dominates contextual diagnostics and the final action-failure stderr is explicitly
non-retriable. Existing action counts/exit codes and nonfatal hook behavior remain.
Whole-request bulk evidence is shared request-level context, not a per-member remote
failure assertion. Uncertain bulk errors cannot suggest strategy switching or Git
disconnect. Documented complete member results and coherent legacy aliases are
validated conservatively; missing/duplicate/partial/unknown results are not success.
Locally rejected sources are not submitted or counted twice; no empty bulk is sent.

Deployment prerequisite errors (dependency create, takeover, inter-tier refresh and
SQL-endpoint discovery) now reach recorded hook failures and halt remaining actions,
including deletes, with `halted:true` and `notExecuted`. Successful inter-tier refresh
is not repeated by post-hooks. A hook failure stops subsequent hook mutations;
post-run orchestration is also suppressed on action failures. Shortcut reconciliation
requires a complete well-formed unique inventory/scope and stops on its first failed
mutation instead of treating failed reads as an empty inventory or claiming success.
Paginated shortcut inventories fail closed before mutation; explicit scoped shortcut
commands remain available after read-only reconciliation.

Remaining aggregation boundaries: other batch/workflow summaries can still stringify
errors; data transformation fallbacks are not a universal proof of readiness.
Parallel tasks are drained even with fail-fast; worker failures become unknown-outcome
action errors instead of discarding earlier results. Other pre-execution early returns
remain normal errors rather than completed-batch reports.
End-to-end propagation into ALL batches/receipts and durable registration remain
separate work. Absence of the optional object is never proof of non-submission.

The mock suite checks gateway failure after a possible acceptance, malformed success
bodies, contradictory rejection/ACK markers, accepted observation failure, local
transport timeout/disconnect, readonly/dry-run guards, safe read retries and a
command-level named-rejection loop. Existing Gen1 and Power BI receipt tests remain
regression coverage. Physical service failures and tenant mutations are not needed
to prove request counts.

`tests/e2e_specialized_submission.rs` covers raw/octet/binary/multipart/ARM senders,
ACK-bearing auth responses, untrusted binary/ARM poll URLs, OneLake rename/copy/delete
ordering and every upload step with local request counts. No live fault injection
or remote mutation was used to verify these failure boundaries.

`tests/e2e_deploy_failures.rs` adds local-only CLI regressions for sequential,
parallel, ordered and bulk failures, shared uncertain-request evidence, mixed and
unusable member results, no empty payload/double count, canonical-vs-legacy codes,
nonfatal schedule hooks, unchanged successful summaries and dry-run/readonly guards.
No live deployment or tenant mutation was performed for this increment. New model
evaluation cases are authored separately; no configured-provider run is claimed.

## Authoritative rejection guidance

- [Fabric REST troubleshooting](https://learn.microsoft.com/en-us/rest/api/fabric/articles/get-started/fabric-api-troubleshooting): 401 means authentication/token validation failed; acquire a token and retry. This does not justify replay after an unrelated accepted/unknown response.
- [Fabric throttling](https://learn.microsoft.com/en-us/rest/api/fabric/articles/throttling): bounded 429 retries honor Retry-After. Generic 430 capacity-throttle handling is retained, not generalized into a gateway retry policy.
- [Fabric capacity optimization](https://learn.microsoft.com/en-us/fabric/enterprise/optimize-capacity): Spark can fail with 430 when capacity is unavailable.

These are documentation/source findings, not new live API behavior claims. No
mutating operation receives a newly verified idempotent exemption in this change.
