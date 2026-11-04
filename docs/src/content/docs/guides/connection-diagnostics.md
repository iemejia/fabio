---
title: Diagnose connections and workspace identity
description: Inspect scoped connection configuration without testing credentials, changing ownership, or granting access.
---

Use `connection diagnostics` to separate four questions:

1. Is the requested connection metadata visible?
2. What effective role does the API report for **the caller**?
3. What credential type is configured?
4. For workspace-identity credentials, what identity metadata is reported in the
   explicitly selected workspace?

None of these proves that stored credentials work, that a source has granted
access, or that a workload can run.

## Choose an explicit scope

```bash
# One connection, without a tenant/global connection inventory
fabio --wrap-untrusted --readonly connection diagnostics --id "$CONNECTION_ID"

# Add explicit workspace identity context; this does NOT prove a binding
fabio --wrap-untrusted --readonly connection diagnostics \
  --id "$CONNECTION_ID" --workspace "$WS"

# Inspect only the connections referenced by one selected item
fabio --wrap-untrusted --readonly connection diagnostics \
  --workspace "$WS" --item "$ITEM" \
  --connection-limit 10 --page-limit 3 --timeout 30
```

`--id` and `--item` are mutually exclusive. Item mode requires `--workspace`.
All resource identifiers must be non-zero UUIDs. `--dry-run` previews the read
scope offline, after input validation.

The item-connections API needs **item read/write permission**, despite using GET.
A protected sensitivity label can also block it. Connection/gateway access,
workspace access, and source access are separate permission boundaries.

## Interpret the report

- **`ready`** means only that the requested configuration observations succeeded.
  `credentialHealth` is always **`not_checked`**, and `notChecked` lists other
  gaps. A recent use timestamp or `skipTestConnection: false` is not a current
  credential-health result.
- **`needs_action`** includes a reported `PendingOwner`, an explicit null workspace
  identity when identity credentials need one, or an unbound item reference.
  Pending owners must review/update credentials before ownership is effective;
  diagnostics does not perform that transition.
- **`unknown`** includes missing/future roles, forbidden/not-found metadata,
  malformed or mismatched IDs, unsupported identity references, partial reference
  scans, and unavailable workspace context. Not-found does not prove deletion.
- An **`Automatic`/SSO** reference can legitimately have no connection ID.
  Diagnostics does not fabricate an ID, search global connections, or infer
  source authorization from it. `None` denotes an unbound reference.

The caller's connection role is **not the workspace identity's source grant**.
An observed application/service-principal ID proves configuration, not token
acquisition or source permission. An omitted identity field remains unknown;
diagnostics does not provision an identity because metadata is missing.

Add `--strict` for an automation exit-code gate: the report is still printed,
but any requested non-ready check produces a non-zero exit. This is a
configuration gate, never a credential-health gate.

## Bounds and safety

`--connection-limit` bounds binding rows (default 10, maximum 20);
`--page-limit` bounds item-reference pages (default 3, maximum 20). Duplicate
explicit IDs are inspected once. `--timeout` is a **total read budget**, including
retries (default 30 seconds, maximum 300). `--all` does not override these bounds.

Only GET requests are made. Diagnostics does not test connections, contact source
URLs, enumerate tenant/global connections or role assignments, call admin APIs,
grant access, provision identities, transfer ownership, or write credentials.
Names, paths, credentials, and raw error bodies are excluded from report evidence.

Follow the inspection/review `nextSteps` as guidance, not authorization to change
security settings. See [remediation hints](../remediation-hints/) and
[workspace/capacity diagnostics](../workspace-diagnostics/) for the complementary
configuration checks.
