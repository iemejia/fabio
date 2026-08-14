---
name: fabio-data-science
description: >-
  Intent-scoped fabio skill for Fabric machine learning and data science: ML experiments (runs, metrics), ML models (registry, versions, endpoints, batch scoring), and anomaly detectors. Use to track experiments, register/version/serve models, score data, and configure anomaly detection. Triggers: "ml model", "ml experiment", "mlflow", "register model", "model version", "score model", "batch scoring", "model endpoint", "anomaly detection", "train model", "data science".
license: MIT
---

# fabio-data-science — Data Science — ML experiments, models, versions, scoring, anomaly detection

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating/listing ML experiments to organize training runs and metrics.
- Reading MLflow run data of an experiment (list-runs with --filter/--order-by, get-run, get-metric-history) to compare runs and inspect logged metrics.
- Registering ML models and managing versions (list-versions, get-version, activate-version, deactivate-version).
- Serving/scoring: get-endpoint/update-endpoint, score and score-version for batch inference.
- Creating and configuring anomaly detectors (get-definition/update-definition).

## When NOT to use (route elsewhere)
- Writing the training code itself (notebooks/Spark) -> use fabio-data-engineering.
- The lakehouse feature tables the model reads/writes -> use fabio-lakehouse.
- BI semantic models ('model' in a reporting sense) -> use fabio-bi (see disambiguate model).

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio ml-experiment
Manage ML experiments (data science)

| Command | Mutates | Description |
|---|---|---|
| `fabio ml-experiment create` | yes | Create a new ML experiment |
| `fabio ml-experiment delete` | yes | Delete an ML experiment |
| `fabio ml-experiment get-metric-history` | no | Get the logged history of a single metric across a run's steps |
| `fabio ml-experiment get-run` | no | Show details of a single run (info, parameters, metrics, tags) |
| `fabio ml-experiment list` | no | List ML experiments in a workspace |
| `fabio ml-experiment list-runs` | no | List runs in an experiment (`MLflow` tracking: parameters, metrics, status) |
| `fabio ml-experiment show` | no | Show details of an ML experiment |
| `fabio ml-experiment update` | yes | Update ML experiment properties (name and/or description) |

### fabio ml-model
Manage ML models (data science)

| Command | Mutates | Description |
|---|---|---|
| `fabio ml-model activate-version` | yes | Activate a specific endpoint version |
| `fabio ml-model create` | yes | Create a new ML model |
| `fabio ml-model deactivate-all-versions` | yes | Deactivate all endpoint versions |
| `fabio ml-model deactivate-version` | yes | Deactivate a specific endpoint version |
| `fabio ml-model delete` | yes | Delete an ML model |
| `fabio ml-model get-endpoint` | no | Get the ML model serving endpoint configuration |
| `fabio ml-model get-registry-version` | no | Get a specific `MLflow` model-registry version (source run, stage, status) |
| `fabio ml-model get-version` | no | Get a specific endpoint version |
| `fabio ml-model list` | no | List ML models in a workspace |
| `fabio ml-model list-registry-versions` | no | List the `MLflow` model-registry versions of an ML model (the trained model versions) |
| `fabio ml-model list-versions` | no | List endpoint versions |
| `fabio ml-model score` | no | Score against the ML model endpoint |
| `fabio ml-model score-version` | no | Score against a specific endpoint version |
| `fabio ml-model show` | no | Show details of an ML model |
| `fabio ml-model update` | yes | Update ML model properties (name and/or description) |
| `fabio ml-model update-endpoint` | yes | Update the ML model serving endpoint configuration |
| `fabio ml-model update-version` | yes | Update a specific endpoint version |

### fabio anomaly-detector
Manage anomaly detectors

| Command | Mutates | Description |
|---|---|---|
| `fabio anomaly-detector create` | yes | Create a new anomaly detector |
| `fabio anomaly-detector delete` | yes | Delete an anomaly detector |
| `fabio anomaly-detector get-definition` | no | Get the definition of an anomaly detector |
| `fabio anomaly-detector list` | no | List anomaly detectors in a workspace |
| `fabio anomaly-detector show` | no | Show details of an anomaly detector |
| `fabio anomaly-detector update` | yes | Update anomaly detector properties (name and/or description) |
| `fabio anomaly-detector update-definition` | yes | Update the definition of an anomaly detector |

## Must / Prefer / Avoid
### MUST
- Create/select an ML experiment to track runs before registering a model from the best run.
- Reference a specific model version for reproducible scoring (score-version), not just the active alias.
- Author anomaly-detector logic via its definition (get-definition/update-definition).

### PREFER
- Experiment tracking (ml-experiment) over ad-hoc metric logging so runs are comparable.
- Batch scoring via score/score-version over hand-rolled inference loops.
- Runtime introspection (context agent --group ml-model|ml-experiment) for exact flags.

### AVOID
- Confusing an ML model with a BI semantic model — different groups (see disambiguate model).
- Deactivating all model versions (deactivate-all-versions) without confirming nothing serves from them.
- Scoring against an unpinned version when reproducibility matters.

## Key gotchas
- ML models are versioned: a model item holds multiple versions; activate-version sets the serving alias while list-versions/get-version address specific ones.
- Scoring has two forms: score (active version) and score-version (a pinned version); endpoints are managed separately (get-endpoint/update-endpoint).
- Anomaly detectors carry their logic in a definition part (base64) — edit via update-definition, not plain flags.
- ml-experiment {list-runs,get-run,get-metric-history} read the per-workspace Fabric-hosted MLflow tracking server (the experiment item GUID is the MLflow experiment_id) — NOT the Fabric item API; --filter/--order-by take MLflow expressions.
- TWO kinds of model versions: 'ml-model list-registry-versions'/'get-registry-version' read the MLflow model REGISTRY (the TRAINED versions, keyed by the model's DISPLAY NAME) — distinct from 'list-versions'/'get-version' which are the serving-ENDPOINT versions. Use the registry commands to discover trained versions after the register/version-model tutorial step.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Scoring returns unexpected results after a retrain | The active version changed; pin with score-version <id>, or confirm which version is active via list-versions. |
| Model endpoint not responding | Check get-endpoint; the serving endpoint is managed separately from version activation. |
| Runs are hard to compare | Group them under an ml-experiment and log metrics per run rather than ad-hoc. |
| Anomaly detector changes ignored | Edit its definition via update-definition (base64 part); metadata-only update does not change logic. |

## Safety
- deactivate-all-versions / deactivate-version can take a served model offline — confirm nothing depends on it.
- Deleting an ml-model removes all its versions — irreversible; confirm with the user.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona data-scientist
- fabio context disambiguate model
- fabio context workflow lakehouse-etl
- fabio context blueprint basic-machine-learning-models
- fabio context persona data-solution-architect
