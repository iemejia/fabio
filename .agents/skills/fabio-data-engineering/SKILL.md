---
name: fabio-data-engineering
description: >-
  Intent-scoped fabio skill for Fabric data engineering with code and orchestration: notebooks, Spark (Livy sessions, job definitions), Spark environments/libraries, data pipelines, copy jobs, and job scheduling. Use to author and run transformation code and orchestrate it. Triggers: "notebook", "run notebook", "pyspark", "spark job", "livy", "spark environment", "data pipeline", "copy job", "schedule job", "orchestrate", "etl code".
license: MIT
---

# fabio-data-engineering — Data Engineering — notebooks, Spark, pipelines, scheduling

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating/updating notebooks and running them (with parameters, --wait).
- Running Spark job definitions or interactive Livy sessions.
- Managing Spark environments (libraries, compute settings) and publishing them.
- Building and running data pipelines; copy jobs for data movement.
- Scheduling recurring runs of notebooks/pipelines/SJDs via the job scheduler.
- Orchestrating with Apache Airflow jobs, running dbt (data-build-tool) jobs, or mounting an Azure Data Factory.

## When NOT to use (route elsewhere)
- Loading files/tables into a lakehouse -> use fabio-lakehouse.
- Low-code Power Query ETL -> use fabio-dataflows.
- T-SQL transformation in a warehouse -> use fabio-warehouse-sql.
- CI/CD promotion of these items across environments -> use fabio-deploy-cicd.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio notebook
Manage notebooks

| Command | Mutates | Description |
|---|---|---|
| `fabio notebook create` | yes | Create a new notebook |
| `fabio notebook delete` | yes | Delete a notebook |
| `fabio notebook get-definition` | no | Get the definition (source code) of a notebook |
| `fabio notebook get-job-instance` | no | Get details of a specific job instance |
| `fabio notebook get-livy-session` | no | Get details of a Livy session |
| `fabio notebook list` | no | List notebooks in a workspace |
| `fabio notebook list-livy-sessions` | no | List Livy sessions for a notebook |
| `fabio notebook run` | yes | Run a notebook |
| `fabio notebook show` | no | Show details of a notebook |
| `fabio notebook status` | no | Check the status of a notebook run |
| `fabio notebook stop` | yes | Stop a running notebook |
| `fabio notebook update` | yes | Update notebook properties (name and/or description) |
| `fabio notebook update-definition` | yes | Update the definition (source code) of a notebook |

### fabio spark
Manage Spark compute (settings, custom pools)

| Command | Mutates | Description |
|---|---|---|
| `fabio spark create-capacity-pool` | yes | Create a custom Spark pool in a capacity |
| `fabio spark create-pool` | yes | Create a custom Spark pool |
| `fabio spark delete-capacity-pool` | yes | Delete a capacity Spark pool |
| `fabio spark delete-pool` | yes | Delete a custom Spark pool |
| `fabio spark get-advice` | no | Get Spark advisor real-time advice for a Spark application (monitoring API) |
| `fabio spark get-capacity-pool` | no | Get details of a capacity Spark pool |
| `fabio spark get-capacity-settings` | no | Get capacity-level Spark settings |
| `fabio spark get-livy-session` | no | Get details of a Livy session |
| `fabio spark get-logs` | no | Get Spark driver / executor / livy logs for a Spark application (monitoring API) |
| `fabio spark get-pool` | no | Show details of a custom Spark pool |
| `fabio spark get-resource-usage` | no | Get the resource usage timeline for a Spark application (monitoring API) |
| `fabio spark get-settings` | no | Get workspace-level Spark settings (custom pools, starter pools, etc.) |
| `fabio spark list-capacity-pools` | no | List custom Spark pools in a capacity |
| `fabio spark list-livy-sessions` | no | List Livy sessions in a workspace |
| `fabio spark list-pools` | no | List custom Spark pools in a workspace |
| `fabio spark update-capacity-pool` | yes | Update a capacity Spark pool |
| `fabio spark update-capacity-settings` | yes | Update capacity-level Spark settings |
| `fabio spark update-pool` | yes | Update a custom Spark pool |
| `fabio spark update-settings` | yes | Update workspace-level Spark settings |

### fabio spark-job-definition
Manage Spark job definitions (batch Spark jobs)

| Command | Mutates | Description |
|---|---|---|
| `fabio spark-job-definition create` | yes | Create a new Spark job definition |
| `fabio spark-job-definition delete` | yes | Delete a Spark job definition |
| `fabio spark-job-definition get-definition` | no | Get the definition of a Spark job definition |
| `fabio spark-job-definition get-livy-session` | no | Get details of a Livy (Spark) session for a Spark job definition |
| `fabio spark-job-definition list` | no | List Spark job definitions in a workspace |
| `fabio spark-job-definition list-livy-sessions` | no | List Livy (Spark) sessions for a Spark job definition |
| `fabio spark-job-definition run` | yes | Run a Spark job definition |
| `fabio spark-job-definition show` | no | Show details of a Spark job definition |
| `fabio spark-job-definition update` | yes | Update Spark job definition properties (name and/or description) |
| `fabio spark-job-definition update-definition` | yes | Update the definition of a Spark job definition |

### fabio environment
Manage environments (Spark compute, libraries, publish)

| Command | Mutates | Description |
|---|---|---|
| `fabio environment cancel-publish` | yes | Cancel a pending publish operation |
| `fabio environment create` | yes | Create a new environment |
| `fabio environment delete` | yes | Delete an environment |
| `fabio environment delete-staging-library` | yes | Delete a staging library by name |
| `fabio environment export-libraries` | no | Export external libraries configuration (published) |
| `fabio environment export-staging-libraries` | no | Export external libraries configuration (staging) |
| `fabio environment get-definition` | no | Get the definition of an environment |
| `fabio environment get-spark-settings` | no | Get the published Spark settings (compute/pool/driver/executor) |
| `fabio environment get-staging-spark-settings` | no | Get the staging (draft) Spark settings |
| `fabio environment import-staging-libraries` | yes | Import external libraries configuration into staging |
| `fabio environment list` | no | List environments in a workspace |
| `fabio environment list-libraries` | no | List published libraries of an environment |
| `fabio environment list-staging-libraries` | no | List staging libraries of an environment |
| `fabio environment publish` | yes | Publish staged changes to an environment |
| `fabio environment remove-staging-library` | yes | Remove an external library from staging |
| `fabio environment show` | no | Show details of an environment |
| `fabio environment update` | yes | Update environment properties (name and/or description) |
| `fabio environment update-definition` | yes | Update the definition of an environment |
| `fabio environment update-staging-spark-compute` | yes | Update staging Spark compute configuration |
| `fabio environment upload-staging-library` | yes | Upload a custom library file into staging |

### fabio data-pipeline
Manage data pipelines (orchestration, scheduling)

| Command | Mutates | Description |
|---|---|---|
| `fabio data-pipeline create` | yes | Create a new data pipeline |
| `fabio data-pipeline create-schedule` | yes | Create a schedule for a data pipeline |
| `fabio data-pipeline delete` | yes | Delete a data pipeline |
| `fabio data-pipeline delete-schedule` | yes | Delete an execute schedule for a data pipeline |
| `fabio data-pipeline get-definition` | no | Get the definition of a data pipeline |
| `fabio data-pipeline get-instance` | no | Get a specific execute job instance for a data pipeline |
| `fabio data-pipeline get-schedule` | no | Get a specific execute schedule for a data pipeline |
| `fabio data-pipeline list` | no | List data pipelines in a workspace |
| `fabio data-pipeline list-instances` | no | List execute job instances for a data pipeline |
| `fabio data-pipeline list-schedules` | no | List execute schedules for a data pipeline |
| `fabio data-pipeline run` | yes | Run a data pipeline |
| `fabio data-pipeline show` | no | Show details of a data pipeline |
| `fabio data-pipeline update` | yes | Update data pipeline properties (name and/or description) |
| `fabio data-pipeline update-definition` | yes | Update the definition of a data pipeline |
| `fabio data-pipeline update-schedule` | yes | Update an execute schedule for a data pipeline |

### fabio copy-job
Manage copy jobs (data movement)

| Command | Mutates | Description |
|---|---|---|
| `fabio copy-job create` | yes | Create a new copy job |
| `fabio copy-job delete` | yes | Delete a copy job |
| `fabio copy-job get-definition` | no | Get the definition of a copy job |
| `fabio copy-job list` | no | List copy jobs in a workspace |
| `fabio copy-job reset` | yes | Reset a copy job (all entities or selected entities) |
| `fabio copy-job run` | yes | Run a copy job on demand |
| `fabio copy-job show` | no | Show details of a copy job |
| `fabio copy-job update` | yes | Update copy job properties (name and/or description) |
| `fabio copy-job update-definition` | yes | Update the definition of a copy job |

### fabio job-scheduler
Manage item job scheduling (run, cancel, schedules)

| Command | Mutates | Description |
|---|---|---|
| `fabio job-scheduler cancel-instance` | yes | Cancel a running job instance |
| `fabio job-scheduler create-schedule` | yes | Create a schedule for an item job type |
| `fabio job-scheduler delete-schedule` | yes | Delete a schedule |
| `fabio job-scheduler get-instance` | no | Show details of a job instance |
| `fabio job-scheduler get-schedule` | no | Show details of a specific schedule |
| `fabio job-scheduler list-instances` | no | List job instances for an item |
| `fabio job-scheduler list-schedules` | no | List schedules for an item job type |
| `fabio job-scheduler run-on-demand` | yes | Run an on-demand job for an item |
| `fabio job-scheduler update-schedule` | yes | Update an existing schedule |

### fabio apache-airflow-job
Manage Apache Airflow jobs (DAGs, environments, pools)

| Command | Mutates | Description |
|---|---|---|
| `fabio apache-airflow-job create` | yes | Create a new Apache Airflow job |
| `fabio apache-airflow-job create-pool-template` | yes | Create a pool template |
| `fabio apache-airflow-job delete` | yes | Delete an Apache Airflow job |
| `fabio apache-airflow-job delete-file` | yes | Delete a file from the Airflow job |
| `fabio apache-airflow-job delete-pool-template` | yes | Delete a pool template |
| `fabio apache-airflow-job deploy-requirements` | yes | Deploy requirements.txt to the environment |
| `fabio apache-airflow-job get-compute` | no | Get environment compute information |
| `fabio apache-airflow-job get-definition` | no | Get the definition of an Apache Airflow job |
| `fabio apache-airflow-job get-environment` | no | Get environment status |
| `fabio apache-airflow-job get-file` | no | Get (download) a file from the Airflow job |
| `fabio apache-airflow-job get-pool-template` | no | Get a pool template |
| `fabio apache-airflow-job get-settings` | no | Get environment settings |
| `fabio apache-airflow-job get-workspace-settings` | no | Get workspace-level Airflow settings |
| `fabio apache-airflow-job list` | no | List Apache Airflow jobs in a workspace |
| `fabio apache-airflow-job list-files` | no | List files (DAGs) in the Airflow job |
| `fabio apache-airflow-job list-libraries` | no | List installed libraries in the environment |
| `fabio apache-airflow-job list-pool-templates` | no | List pool templates |
| `fabio apache-airflow-job show` | no | Show details of an Apache Airflow job |
| `fabio apache-airflow-job start-environment` | yes | Start the Airflow environment |
| `fabio apache-airflow-job stop-environment` | yes | Stop the Airflow environment |
| `fabio apache-airflow-job update` | yes | Update Apache Airflow job properties (name and/or description) |
| `fabio apache-airflow-job update-compute` | yes | Update the compute configuration for the Airflow job environment (pool template) |
| `fabio apache-airflow-job update-definition` | yes | Update the definition of an Apache Airflow job |
| `fabio apache-airflow-job update-settings` | yes | Update environment settings |
| `fabio apache-airflow-job update-workspace-settings` | yes | Update workspace-level Airflow settings |
| `fabio apache-airflow-job upload-file` | yes | Upload a file to the Airflow job |

### fabio data-build-tool-job
Manage data build tool jobs (dbt-style transformations) [preview]

| Command | Mutates | Description |
|---|---|---|
| `fabio data-build-tool-job create` | yes | Create a new data build tool job [preview] |
| `fabio data-build-tool-job delete` | yes | Delete a data build tool job [preview] |
| `fabio data-build-tool-job get-definition` | no | Get the definition of a data build tool job [preview] |
| `fabio data-build-tool-job list` | no | List data build tool jobs in a workspace [preview] |
| `fabio data-build-tool-job run` | yes | Run a data build tool job on-demand [preview] |
| `fabio data-build-tool-job show` | no | Show details of a data build tool job [preview] |
| `fabio data-build-tool-job update` | yes | Update data build tool job properties (name and/or description) [preview] |
| `fabio data-build-tool-job update-definition` | yes | Update the definition of a data build tool job [preview] |

### fabio mounted-data-factory
Manage Mounted Data Factories (ADF integration)

| Command | Mutates | Description |
|---|---|---|
| `fabio mounted-data-factory create` | yes | Create a new Mounted Data Factory |
| `fabio mounted-data-factory delete` | yes | Delete a Mounted Data Factory |
| `fabio mounted-data-factory get-definition` | no | Get the definition of a Mounted Data Factory |
| `fabio mounted-data-factory list` | no | List Mounted Data Factorys in a workspace |
| `fabio mounted-data-factory show` | no | Show details of a Mounted Data Factory |
| `fabio mounted-data-factory update` | yes | Update Mounted Data Factory properties |
| `fabio mounted-data-factory update-definition` | yes | Update the definition of a Mounted Data Factory |

## Must / Prefer / Avoid
### MUST
- Use --file for notebook source (.py and .ipynb are auto-detected); fabio wraps the format — do NOT hand-build ipynb JSON.
- Use --wait/--timeout on notebook/pipeline/SJD runs to observe completion.
- Bind a notebook to its default lakehouse with --lakehouse at create time when it reads/writes lakehouse tables.

### PREFER
- --file (a written code file) over --content, which is only for small inline snippets.
- job-scheduler create-schedule for recurring runs over external cron.
- Spark environments to manage libraries once instead of per-notebook installs.
- Runtime introspection (context agent --group notebook|spark|data-pipeline) over guessing flags.

### AVOID
- Constructing ipynb JSON manually — pass the file and let fabio handle wrapping.
- Assuming a run is done without --wait (async jobs return before completion).
- Ignoring cold-start latency on small capacity (first Spark run takes minutes).

## Key gotchas
- Watch a running job to completion with 'job-scheduler get-instance --follow': fabio polls the instance on --interval (default 5s), streams NDJSON per cycle, and STOPS EARLY when the status is terminal (Completed/Failed/Cancelled/Deduped) — or when --max-duration / Ctrl-C bounds it (reason 'complete' vs 'max_duration'). Use it to watch an already-running instance (from list-instances or a run-on-demand without --wait); run-on-demand --wait already blocks for jobs you start. Same terminal-aware bounded-follow works on 'operation get-state --follow' (LRO watching: Succeeded/Failed/Canceled/Undefined).
- Notebook source cells must be a list of strings in ipynb; fabio builds this for you from --file/--content.
- First Spark run on a small capacity is a 2-5 min cold start; LRO may report 430 TooManyRequestsForCapacity when the capacity is saturated.
- Environment library changes require a publish (staging/publish) before they take effect.
- Fabric Runtime 2.0 (Spark 4.1 / Delta 4.2, Python 3.13) is GA but OPT-IN — 1.3 (Spark 3.5) is still the default until ~late Sep 2026. Set it with 'environment update-staging-spark-compute --runtime-version 2.0' then publish. The Python 3.13 jump is a BREAKING change for python/wheel libraries: after switching, remove all libraries, publish, re-add them, and publish again (a 'republish the environment' LibraryManagementError otherwise). Delta 4.2 features (Liquid Clustering, Z-order via native engine) are experimental and Spark-experience-only — don't enable them on tables shared across other Fabric workloads.
- Spark monitoring (spark get-advice / get-resource-usage / get-logs) needs --app-id + --attempt-id from 'spark get-livy-session' (sparkApplicationId / attemptNumber); --item-type maps to notebook|spark-job-definition|lakehouse. 'get-logs --type livy' needs no --app-id, but driver/executor logs do (--meta returns log-file metadata JSON, otherwise raw text). Livy sessions are also listable per-item via 'spark-job-definition list-livy-sessions'/'get-livy-session'.
- On-demand job triggers (notebook/data-pipeline/copy-job/dataflow/spark-job-definition run) return 202 with the instance id in the Location header (fabio reads it); --wait polls to completion. 'copy-job run' job type is Execute.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Notebook run returns before the work is done | Add --wait --timeout <secs>; the run is async (202 + polling). |
| Spark job fails with 430 TooManyRequestsForCapacity | Capacity is saturated; retry later, reduce concurrency, or use a larger capacity. |
| Notebook can't find its lakehouse tables | Create it with --lakehouse $LH so the default lakehouse (trident) metadata is injected. |
| Installed library not available at runtime | Publish the Spark environment (environment staging/publish) and attach it to the notebook. |

## Safety
- Deleting a notebook/pipeline/SJD removes its definition — confirm with the user.
- Scheduling a recurring job commits ongoing capacity consumption — confirm cadence and timezone.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona data-engineer
- fabio context workflow lakehouse-etl
- fabio context workflow rti-pipeline
- fabio context blueprint medallion
- fabio context blueprint lambda
- fabio context persona data-solution-architect
