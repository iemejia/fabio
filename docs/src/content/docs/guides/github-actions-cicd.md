---
title: Deploy to Fabric from GitHub Actions
description: A branch-per-environment CI/CD pipeline that deploys Fabric items with Fabio — OIDC auth, plan/apply/verify, value-set activation, post-deploy ETL, and environment protection.
---

This guide shows a complete dev → test → prod CI/CD pipeline that deploys Microsoft Fabric items with Fabio. Git is the source of truth; each environment is a branch, and a merge triggers a deploy to the matching workspace.

For the strategy behind this model (why API-driven deploy, branch layout, parameterization) see `fabio context best-practices cicd-lifecycle`. For authentication details see [Authenticate Fabio](../authentication/).

## The model

| Branch | Workspace | How it deploys |
|--------|-----------|----------------|
| `dev` | Development | Git-connected (developers branch out and commit here) |
| `test` | Test | Fabio `deploy apply` via GitHub Actions |
| `main` | Production | Fabio `deploy apply` via GitHub Actions |

Only **Dev** is connected to Fabric Git integration. **Test** and **Prod** receive script/API deploys only — this keeps Fabric Git integration off the production path (avoiding "ghost commits" and Source Control drift) while still using Git as the single source of truth. This is the Fabio-recommended hybrid; see the `deploy-cicd` skill for the rationale.

## Prerequisites

- Three Fabric workspaces (Dev / Test / Prod) on a Fabric or Power BI Premium capacity.
- An Entra app registration with **Contributor** on the Test and Prod workspaces, and a [federated credential](https://learn.microsoft.com/en-us/entra/workload-id/workload-identity-federation) (audience `api://AzureADTokenExchange`) so no client secret is stored — see [Workload identity federation](../authentication/#workload-identity-federation-github-actions-oidc).
- Item definitions committed under `fabric-items/` (produced by `fabio deploy export`), plus a `parameters.json` for environment-specific values.
- Two [GitHub Environments](https://docs.github.com/en/actions/managing-workflow-runs-and-deployments/managing-deployments/managing-environments-for-deployment), `Test` and `Prod`, each holding `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, and `FABRIC_WORKSPACE_ID` (the target workspace differs per environment). Add required reviewers on `Prod`.

## A reusable deploy workflow

Factor the deploy steps into one [reusable workflow](https://docs.github.com/en/actions/sharing-automations/reusing-workflows) so Test and Prod share the same logic. The `environment:` key binds the job to the matching GitHub Environment, so its protection rules and scoped secrets apply automatically.

```yaml
# .github/workflows/reusable-deploy.yml
name: Deploy to Fabric

on:
  workflow_call:
    inputs:
      environment:
        required: true
        type: string   # "Test" or "Prod" — must match the Variable Library value-set name

permissions:
  id-token: write
  contents: read

jobs:
  deploy:
    runs-on: ubuntu-latest
    environment: ${{ inputs.environment }}   # applies protection rules + scoped secrets
    steps:
      - uses: actions/checkout@v6

      - name: Install fabio
        run: curl -fsSL https://raw.githubusercontent.com/iemejia/fabio/main/install.sh | bash

      - name: Sign in to Fabric with GitHub OIDC
        run: |
          OIDC_TOKEN=$(curl -sS -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
            "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=api://AzureADTokenExchange" | jq -r '.value')
          fabio auth login \
            --tenant "${{ secrets.AZURE_TENANT_ID }}" \
            --client-id "${{ secrets.AZURE_CLIENT_ID }}" \
            --federated-token "$OIDC_TOKEN"

      - name: Validate (offline)
        run: |
          fabio deploy validate --source ./fabric-items \
            --parameters ./parameters.json --env "${{ inputs.environment }}"

      - name: Plan
        run: |
          fabio deploy plan --source ./fabric-items \
            --workspace "${{ secrets.FABRIC_WORKSPACE_ID }}" \
            --parameters ./parameters.json --env "${{ inputs.environment }}"

      - name: Apply
        run: |
          fabio deploy apply --source ./fabric-items \
            --workspace "${{ secrets.FABRIC_WORKSPACE_ID }}" \
            --parameters ./parameters.json --env "${{ inputs.environment }}" \
            --verify --post-run-item "Load_Data"
```

What the steps do:

- **`auth login --federated-token`** exchanges the GitHub OIDC token for a Fabric token with no stored secret. (Alternatively run `azure/login` with OIDC and Fabio picks up the Azure CLI session automatically.)
- **`deploy validate`** runs offline pre-flight checks (unknown types, duplicate/logical-ID clashes, parameter coverage) with no API calls.
- **`deploy plan`** prints the create/update/delete/skip changeset. Inspect `data.destructive` before applying.
- **`deploy apply --verify`** deploys with content-hash convergence (unchanged items are skipped), auto-activates the Variable Library value set matching `--env`, and audits convergence. `--post-run-item` triggers a pipeline/notebook by name afterward to populate data.

## The environment triggers

Each stage is a thin caller that invokes the reusable workflow on a push to its branch:

```yaml
# .github/workflows/deploy-test.yml
name: Deploy Test
on:
  push:
    branches: [test]
    paths: ['fabric-items/**']     # skip docs-only commits
jobs:
  deploy:
    uses: ./.github/workflows/reusable-deploy.yml
    with:
      environment: Test
    secrets: inherit
```

```yaml
# .github/workflows/deploy-prod.yml
name: Deploy Prod
on:
  push:
    branches: [main]
    paths: ['fabric-items/**']
jobs:
  deploy:
    uses: ./.github/workflows/reusable-deploy.yml
    with:
      environment: Prod
    secrets: inherit
```

`secrets: inherit` forwards each environment's scoped secrets (notably the per-environment `FABRIC_WORKSPACE_ID`).

## Gate the pull requests

Add these checks on PRs so nothing unreviewed reaches an environment:

- **PR-readiness / environment hygiene** — assert the repo is bound to the expected environment and carries no stray IDs before merging:

  ```yaml
  - run: |
      fabio deploy validate --source ./fabric-items --pr-ready \
        --parameters ./parameters.json --expect-env dev \
        --allow-value-set Test,Prod
  ```

- **Branch protection** — require PRs into `dev`, `test`, and `main`; enforce the promotion path with source-branch restrictions (PRs into `test` must come from `dev`; into `main` from `test`) via [branch rulesets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets).
- **Deploy-time approval** — required reviewers on the `Prod` GitHub Environment pause the run until an approver clicks "Approve and deploy".

## Hardening

- **Pin actions to a full commit SHA** in production workflows (e.g. `actions/checkout@<sha> # v6`) to prevent a moved tag from injecting code — tags are used above for readability.
- **Least privilege** — the CI app is Contributor on its own workspace only; a Test compromise cannot reach Prod. Prefer one app registration per environment.
- **Non-Git-tracked items** — if the workspace contains item types Git integration doesn't support, `deploy export` flags them in a `tracking_note`; promote those with `fabio deployment-pipeline deploy`. See `fabio context best-practices item-tracking-categories`.

## Rolling back

A rollback is `git revert` + re-deploy the known-good source (`deploy apply --verify`). A definition rollback does **not** restore data — plan that separately. See `fabio context best-practices hotfix-rollback` and `fabio context workflow deploy-rollback`.

## See also

- [Authenticate Fabio](../authentication/) — OIDC and service-principal setup
- `fabio context workflow cicd-deploy` — the export → parameterize → plan → apply recipe
- `fabio context best-practices cicd-lifecycle` — full lifecycle strategy
- [`deploy` command reference](../../reference/commands/deploy/)
