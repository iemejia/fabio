# Skill routing and safety evaluations

These single-turn promptfoo cases test the instructions an agent receives, not
Fabric API behavior. The default root skill and workload-specific sub-skills are
loaded from the repository. No generated command is executed against Fabric.

Generation **and rubric grading** use the same existing Microsoft Foundry
deployment. The current live-validated catalog model is **`gpt-6.1-sol`**. A YAML
anchor shares the complete provider configuration; changing only generation must
not leave the rubric grader on an obsolete model or endpoint.

## Configure a deployed model

Install Node.js 24+ and the verified promptfoo version:

```bash
npm install -g promptfoo@0.125.1
```

Find an existing, succeeded **chat** deployment with Azure CLI. Image and
embedding deployments cannot generate or grade CLI responses. Inspect the output
privately; do not copy resource names, endpoints, aliases, or IDs into this repo,
issues, or evaluation reports.

```bash
az login
az cognitiveservices account list --subscription "$AZURE_SUBSCRIPTION_ID"
az cognitiveservices account deployment list \
  --subscription "$AZURE_SUBSCRIPTION_ID" \
  --resource-group "$FOUNDRY_RESOURCE_GROUP" --name "$FOUNDRY_ACCOUNT"
```

Set `AZURE_OPENAI_DEPLOYMENT_NAME` to the chosen deployment alias (it need not
equal the public catalog model name), then configure the provider privately:

```bash
export AZURE_OPENAI_MODEL_NAME=gpt-6.1-sol
export AZURE_OPENAI_DEPLOYMENT_NAME="$FOUNDRY_DEPLOYMENT_NAME"
export AZURE_OPENAI_API_BASE_URL="$(az cognitiveservices account show \
  --subscription "$AZURE_SUBSCRIPTION_ID" \
  --resource-group "$FOUNDRY_RESOURCE_GROUP" --name "$FOUNDRY_ACCOUNT" \
  --query properties.endpoint -o tsv)"
export AZURE_OPENAI_API_KEY="$(az cognitiveservices account keys list \
  --subscription "$AZURE_SUBSCRIPTION_ID" \
  --resource-group "$FOUNDRY_RESOURCE_GROUP" --name "$FOUNDRY_ACCOUNT" \
  --query key1 -o tsv)"
```

The base URL must be an **HTTPS account origin**, not a project URL or a URL
ending in `/openai/v1`. The Azure provider adds the deployment path and the
`2024-10-21` API version. `modelName` identifies the public model separately from
the deployment alias, so promptfoo applies the right model capability rules.
`omitDefaults: true` avoids obsolete sampling/reasoning parameters; the output
budget is 4,096 completion tokens. Both generation and rubric requests were
validated live with these settings.

Alternatively keep these four assignments in a private, Git-ignored `.env` and
pass `--env-file .env` explicitly. Preserve unrelated existing entries and restrict
file permissions (for example `chmod 600 .env` on Unix; use a private account ACL
on Windows). The runner loads only the four evaluation variables as literal data:
it never executes shell code, expands variables or loads an env file implicitly.
Existing nonempty process-environment values take precedence. Malformed or
duplicate evaluation assignments fail without printing their values.

Use the existing account/deployment in the intended Azure CLI tenant. Do not
switch tenants, create resources, enable local-key auth, rotate keys or change
roles just to configure an evaluation. If local-key auth is disabled, do not
bypass that policy; this runner's API-key provider needs an authorized compatible
deployment. Local discovery/configuration does not update GitHub secrets.

## Run

From the repository root:

```bash
python3 -m unittest discover -s tests/eval -p 'test_*.py'
node tests/eval/check-config.mjs "$(npm root -g)"
python3 tests/eval/run.py --filter-pattern '^(Remediation hints:|Workspace diagnostics:|BI routing:)' --min-pass-rate 100
python3 tests/eval/run.py --summary /path/outside/repository/eval-summary.json
python3 tests/eval/run.py --env-file .env --filter-pattern '^(Deploy evidence:|Job receipt:|Receipt import:|Post-Gen1 acceptance:)' --min-pass-rate 100
```

The runner uses only Python's standard library. It validates required variables
before invoking promptfoo and disables telemetry, sharing, cache, and persistent
evaluation storage. Raw logs/results stay in a private temporary directory and
are removed afterward. Only numeric summary fields are printed or written.
For `--filter-pattern`, use simple regex syntax supported by both Python and
JavaScript (as in the example).

The default pass-rate threshold remains **70%**. Missing configuration, timeout,
operational errors, empty/incomplete/duplicate results, provider/grading errors,
broken JavaScript assertions, and truncated responses fail regardless of that
threshold. Assertion-only failures use the configured threshold. Use
`--min-pass-rate 100` for focused regressions.
`failedCaseIndices` identifies failures by zero-based position in the authored
configuration without publishing model text or provider details.

`check-config.mjs` loads the same YAML parser shipped with the installed
promptfoo. It checks the shared provider and executes all JavaScript assertions
against synthetic text. A multi-line assertion must explicitly `return` its
result; merely compiling the JavaScript does not catch an undefined return.
Regression fixtures also cover capitalization-independent routing, harmless JSON
whitespace, and discovery-first recovery. These checks require no credentials.

Direct `promptfoo eval` is available for private debugging, but its logs and JSON
reports can contain resolved provider configuration and credentials. Do not share
them or upload them as CI artifacts. An assertion that demands a flag absent from
the supplied skill should accept `context describe` discovery rather than
rewarding invented flags. Keep the behavioral safety checks intact.

## CI

`.github/workflows/eval.yml` pins the same promptfoo version, tests the runner,
and uploads **only a numeric summary**, never raw model output/configuration.
Configure these repository **secrets** for the selected account/deployment:

- `AZURE_OPENAI_API_KEY`
- `AZURE_OPENAI_API_BASE_URL`
- `AZURE_OPENAI_DEPLOYMENT_NAME`

The public `AZURE_OPENAI_MODEL_NAME` is set to `gpt-6.1-sol` in the workflow.
Secrets for an old account must be updated together; a new deployment alias alone
does not repair an obsolete endpoint or API key. Local Azure CLI discovery does
not change GitHub secrets or provision cloud resources.
