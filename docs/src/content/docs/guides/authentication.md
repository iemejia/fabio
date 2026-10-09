---
title: Authenticate Fabio
description: Choose interactive, service-principal, workload-identity, or cached authentication.
---

Fabio resolves credentials from several sources so local development and automation can use the same commands.

## Interactive sign-in

Azure CLI is the default local-user credential. It owns browser/WAM/device-code authentication, MFA, Conditional Access, refresh tokens, and its secure cache:

```bash
az login --allow-no-subscriptions
fabio auth login
```

Fabio does not ship a default Entra client ID because the project does not yet have a stable, long-lived Entra tenant and publisher registration. For native Fabio login, use a customer-owned public-client registration:

```bash
fabio auth login --device-code --client-id <public-client-id>
fabio auth login --browser --client-id <public-client-id>
```

On Windows, Web Account Manager provides native single sign-on:

```powershell
fabio auth login --wam --client-id <public-client-id>
```

`--client-id` takes precedence over `FABIO_CLIENT_ID`; `AZURE_CLIENT_ID` remains reserved for workload identities. Create a compatible customer registration with `./scripts/create-fabio-app.sh`. Inspect authentication with `fabio auth status`. `fabio auth logout` clears only Fabio's cache; it does not sign out Azure CLI or other ambient credentials.

## Service principal

For non-interactive CI/CD:

```bash
fabio auth login --service-principal \
  --tenant <tenant-id> \
  --client-id <client-id> \
  --client-secret <client-secret>
```

Prefer environment variables or your CI secret store over command-line secrets. Fabio also supports certificate and federated-token authentication; run `fabio auth login --help` for the exact flags.

## Workload identity federation (GitHub Actions OIDC)

Avoid storing a client secret entirely by exchanging a short-lived GitHub OIDC token for a Fabric token (Microsoft's [recommended](https://learn.microsoft.com/en-us/azure/well-architected/security/identity-access) secretless pattern). Configure a [federated credential](https://learn.microsoft.com/en-us/entra/workload-id/workload-identity-federation) on your app registration (audience `api://AzureADTokenExchange`), then pass the GitHub OIDC token straight to Fabio — no Azure CLI required in the runner:

```yaml
permissions:
  id-token: write   # allow the job to request a GitHub OIDC token
  contents: read

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - name: Install fabio
        run: curl -fsSL https://raw.githubusercontent.com/iemejia/fabio/main/install.sh | bash
      - name: Sign in to Fabric with GitHub OIDC
        run: |
          OIDC_TOKEN=$(curl -sS \
            -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
            "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=api://AzureADTokenExchange" | jq -r '.value')
          fabio auth login \
            --tenant "${{ secrets.AZURE_TENANT_ID }}" \
            --client-id "${{ secrets.AZURE_CLIENT_ID }}" \
            --federated-token "$OIDC_TOKEN"
      - run: fabio deploy apply --source ./fabric-items --workspace "Production" --env prod
```

Alternatively, run `azure/login@v3` with OIDC first — Fabio's credential chain then picks up the Azure CLI session automatically (no `fabio auth login` step needed). The native `--federated-token` flow above avoids the Azure CLI dependency and binds the exchange directly to your app registration.

## Credential precedence

Fabio checks an explicit `FABIO_ACCESS_TOKEN`, its managed-login cache, Azure environment credentials, managed identity, Azure CLI, and Azure Developer CLI. A command may require a separate audience for Fabric, OneLake storage, SQL, ARM, Kusto, Microsoft Graph, or Cosmos. Legacy caches without an issuing client ID are never refreshed under a replacement app.

## Per-scope static tokens

An OAuth access token is minted for one audience and cannot be exchanged for another — a Fabric token is rejected by Azure SQL. The credential chain (Azure CLI, service principal, managed identity, login cache) mints a correct token per audience automatically. The static-token path (used in Fabric Notebooks and constrained CI, where interactive login is unavailable) instead reuses `FABIO_ACCESS_TOKEN` for every audience, which fails for anything outside Fabric.

To use a static token with a non-Fabric command, set the matching scope-specific variable — it takes precedence over `FABIO_ACCESS_TOKEN` for its scope and is only read when a command needs that audience:

| Variable | Audience | Needed for |
|----------|----------|------------|
| `FABIO_ACCESS_TOKEN` | Fabric | All Fabric REST and deploy operations |
| `FABIO_SQL_ACCESS_TOKEN` | Azure SQL | T-SQL/TDS: warehouse, SQL database, SQL endpoint, and lakehouse queries and insights; `semantic-model generate`; `digital-twin-builder query`; `ontology generate` from a lakehouse |
| `FABIO_STORAGE_ACCESS_TOKEN` | Azure Storage | OneLake file operations, when the Fabric token is not accepted |
| `FABIO_ARM_ACCESS_TOKEN` | Azure Resource Manager | Capacity lifecycle |
| `FABIO_GRAPH_ACCESS_TOKEN` | Microsoft Graph | `label list` |
| `FABIO_COSMOS_ACCESS_TOKEN` | Azure Cosmos DB | Cosmos database data-plane commands |

In a Fabric Notebook, obtain each token with `notebookutils.credentials.getToken(<resource>)` and set the matching variable. If a T-SQL command fails with a login error while only `FABIO_ACCESS_TOKEN` is set, Fabio's error hint tells you to set `FABIO_SQL_ACCESS_TOKEN`.

If an error reports `AUTH_REQUIRED`, follow its `hint` rather than changing the requested operation.
