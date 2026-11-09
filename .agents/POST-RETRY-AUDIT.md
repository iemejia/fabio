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
| Fabric raw text / octet-stream / bytes / bytes-with-Accept | Auth refresh but no generic 5xx loop | Audited, not migrated to contextual JSON errors in this increment. Binary response/LRO routing remains separate work. |
| Power BI bytes / multipart | Bytes auth refresh; consumed multipart form is not retried | Audited, no new retry or evidence contract promised. |
| ARM POST | One attempt; optional GET-only LRO observation | No replay added; contextual ARM errors not covered here. |
| OneLake / custom HTTP / MCP / database SDK paths | Separate transport and operation semantics | Not certified by this audit. No blanket global HTTP retry switch or invented idempotency key. |

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

Some existing batch/workflow summaries (including deployment apply) stringify
per-action errors. Blocking the inspected internal loops does not imply structured
submission evidence survives every aggregate output. End-to-end propagation into
all batch summaries/receipts remains separate work; absence of the optional object
must never be treated as proof of non-submission.

The mock suite checks gateway failure after a possible acceptance, malformed success
bodies, contradictory rejection/ACK markers, accepted observation failure, local
transport timeout/disconnect, readonly/dry-run guards, safe read retries and a
command-level named-rejection loop. Existing Gen1 and Power BI receipt tests remain
regression coverage. Physical service failures and tenant mutations are not needed
to prove request counts.

## Authoritative rejection guidance

- [Fabric REST troubleshooting](https://learn.microsoft.com/en-us/rest/api/fabric/articles/get-started/fabric-api-troubleshooting): 401 means authentication/token validation failed; acquire a token and retry. This does not justify replay after an unrelated accepted/unknown response.
- [Fabric throttling](https://learn.microsoft.com/en-us/rest/api/fabric/articles/throttling): bounded 429 retries honor Retry-After. Generic 430 capacity-throttle handling is retained, not generalized into a gateway retry policy.
- [Fabric capacity optimization](https://learn.microsoft.com/en-us/fabric/enterprise/optimize-capacity): Spark can fail with 430 when capacity is unavailable.

These are documentation/source findings, not new live API behavior claims. No
mutating operation receives a newly verified idempotent exemption in this change.
