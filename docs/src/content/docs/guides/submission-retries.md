---
title: Handle submission failures without duplicating work
description: Distinguish safe retries, known rejections and ambiguous mutation acceptance.
---

A failed response does not prove a mutation failed to start. For example, a gateway
can return 503 after the backend accepted a create, Spark statement or job. Repeating
that submission may create duplicate resources or run work twice.

## The selected transport policy

- Generic Fabric JSON mutations (`post` and bodyless `post_empty`) no longer replay
  502/503/504 responses. Network errors, request timeouts and 5xx responses retain
  unknown-outcome evidence rather than granting automatic retry.
- Safe Fabric GETs and explicitly classified read-only Fabric POSTs retain their
  bounded gateway retries. Examples include catalog search, Activator activation
  queries and the shared definition-retrieval helper. Classification is explicit
  in code, not inferred from a URL/action-name substring.
- Existing authentication refresh after 401 and generic Fabric 429/430 rejection
  retries remain bounded, but contradictory operation/Location markers suppress
  mutation replay. Generic throttling uses at most three retries, honors
  `Retry-After` up to 300 seconds, and falls back to 10/20/30 seconds. This is
  separate from LRO polling's delay cap.
- Policies differ by sender. The [Gen1 sender](../gen1-upgrade-execution/) remains
  exactly one attempt even for authentication/throttling rejection. Notebook's
  one-attempt sender remains one attempt. Power BI JSON and generic job triggers
  do not gain gateway retries through this change.
- Existing command-level named-rejection loops for deployment name reservation,
  mirrored-database readiness and Git initialization now check the submission
  safety signal before trying again. A familiar message inside a 5xx response is
  not sufficient permission to replay.
- Git checkout does not automatically reconnect the previous branch after an
  uncertain reconnect response: that could undo an already accepted switch.
  Its existing compensation remains available after a definitive rejection.

No new mutation is declared idempotent and no unsupported idempotency key is sent.
Service guidance for clearly rejected requests is documented in
[Fabric troubleshooting](https://learn.microsoft.com/en-us/rest/api/fabric/articles/get-started/fabric-api-troubleshooting)
and [throttling](https://learn.microsoft.com/en-us/rest/api/fabric/articles/throttling).

## Read the error before taking action

Selected JSON mutation and job paths can now return an optional `error.submission`:

```json
{
  "error": {
    "code": "API_ERROR",
    "message": "Service unavailable",
    "hintType": "semantic_correction",
    "retriable": false,
    "submission": {
      "service": "fabric",
      "state": "unknown",
      "httpStatus": 503,
      "locationPresent": false,
      "serverRetriable": true
    }
  }
}
```

- `accepted` means HTTP 202 was received; `acknowledged` means another successful
  status was received but subsequent response/observation handling failed.
  `unknown` does not distinguish non-delivery from accepted work with a lost reply.
- API error code, parameters, details and trace metadata are retained. The API's
  retriability hint is preserved separately in `serverRetriable` when local safety
  forces `retriable:false`. It is evidence, not permission to resubmit.
- Requested workspace/target UUIDs are included when the route supplies them.
  A unique, valid nonzero `x-ms-operation-id` can be retained as `operationId`.
  Arbitrary names, request bodies, query strings and returned URLs are not copied
  into this evidence object; `locationPresent` is only a boolean. A trace ID or
  Location string is never promoted into an operation ID by this mechanism.

**This is not a verified scoped receipt, an assigned resource ID, a batch result
or proof of completion.** It does not reconstruct body-selected targets or make a
new durable ledger entry. Never use an operation ID as a local receipt ID.

Preserve the original exact request and error privately. Inspect the same known
operation/resource with read-only tooling for the correct configured service.
Do not guess identities, choose latest history, repeat accepted/unknown work,
automatically cancel it, or repeat a mutation merely to use a longer timeout.

## Deployment batch failures

`deploy apply` preserves the legacy `failures[].change`, `error` and `code`
fields. Typed failures also carry `errorDetails`, using the same canonical fields
as stderr, including `hintType`, API parameters/traces and optional `submission`.
The legacy `code` may be a strategy-specific value such as `BULK_IMPORT_FAILED`;
`errorDetails.code` remains the canonical underlying Fabio code.

Every failed action has `retryAllowed:false`. The batch also carries this flag
and dominant no-replay guidance when an action or a recorded hook fails. Do not
override it with a nested server `retriable:true` or a `diagnosticHint`: actions
can have successful substeps and deployment is not an atomic transaction.
Preserve the output privately, reconcile the existing resources read-only, and
review a new scoped plan before any further mutation. Do not automatically replay
the batch, switch strategy or disconnect Git after an uncertain submission.

For bulk import, evidence copied into multiple action records describes the **one
shared request**, not independently verified remote failures of those items.
Only a unique complete `Succeeded` member result with a valid nonzero item ID
counts as success. Missing, duplicate, contradictory or unknown results become
`BULK_RESULT_UNVERIFIED`; `Failed` and `SucceededDespiteFailures` remain failures.
No transport acknowledgement or operation identity is fabricated from these rows.
Source items rejected before payload assembly are reported once and not submitted.

Existing status/count/exit behavior remains: action failures yield
`partial_failure` and exit 1; nonfatal hook errors can coexist with
`status:succeeded` and exit 0. Check `post_hooks` and `retryAllowed`, not only the
status. Hook timing and best-effort helpers that discard errors internally are
unchanged; those discarded errors cannot reach `errorDetails`.

## Boundaries

This increment covers the shared Fabric JSON mutation path, configurable-LRO JSON,
job trigger paths and Power BI JSON/refresh paths. It is **not** a universal
exactly-once or end-to-end idempotence guarantee. Raw text, binary, multipart, ARM,
OneLake, custom HTTP/SDK paths and bespoke read-only definition callers are not all
migrated to the new evidence contract. Existing successful outputs remain unchanged.

Other job handlers' best-effort persistence, supported recovery/import and
post-upgrade readiness remain separate work. See [local receipt storage](../local-job-receipts/).

Other batch/workflow summaries still stringify per-action errors instead of exposing
the optional structured object. Absence of `submission` in such a summary—or in an
older/specialized transport—is not proof that no remote work was submitted.
