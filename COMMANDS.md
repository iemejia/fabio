# Commands

> **AI agents**: Instead of parsing this file, run `fabio agent-context` to get a machine-readable command schema with flags, types, mutability, and examples.

## Global Flags

All commands accept these global flags:

| Flag | Short | Description |
|------|-------|-------------|
| `--output` | `-o` | Output format: `json`, `table`, `plain`, `csv`, `tsv` |
| `--json` | | Shorthand for `--output json` |
| `--query` | `-q` | JMESPath expression (see [jmespath.org](https://jmespath.org/)) |
| `--quiet` | | Suppress all stdout output |
| `--verbose` | `-v` | HTTP/LRO/auth diagnostic tracing on stderr (debugging only) |
| `--dry-run` | | Preview mutations without executing |
| `--limit` | | Maximum items for list commands |
| `--all` | | Fetch all pages (auto-paginate) |
| `--continuation-token` | | Resume from a previous page |
| `--profile` | | Named profile for default settings |
| `--lro-timeout` | | LRO polling timeout in seconds (default: 120) |
| `--force` | | Skip confirmation prompts |

> **Note on `--verbose`**: This flag is for diagnostic and debugging purposes only. Agents must not use `--verbose` in normal operation — it emits high-volume HTTP request/response traces to stderr that are not machine-parseable. Use it only when troubleshooting failures (e.g., unexpected 4xx/5xx errors, auth issues, LRO timeouts).

## Core

```
fabio auth login             Log in to Microsoft Fabric
                             Device code (default): fabio auth login
                             Browser PKCE:          --browser (opens system browser; SSO on macOS)
                             Service principal:     --service-principal --tenant <T> --client-id <C>
                               + --client-secret <S>           (client secret)
                               + --certificate <path>          (PEM/PFX cert)
                               + --federated-token <jwt>       (OIDC assertion)
                               + --federated-token-file <path> (OIDC from file)
                             WAM broker (Windows only): --wam
fabio auth logout            Log out and clear cached credentials
fabio auth status            Show current authentication status and credential source

fabio workspace list         List all workspaces
fabio workspace show         Show details of a workspace
fabio workspace create       Create a new workspace
fabio workspace update       Update workspace properties (name/description)
fabio workspace delete       Delete a workspace
fabio workspace assign-capacity      Assign a workspace to a capacity
fabio workspace unassign-capacity    Unassign a workspace from its capacity
fabio workspace provision-identity   Provision workspace managed identity
fabio workspace deprovision-identity Deprovision workspace managed identity
fabio workspace list-role-assignments  List workspace role assignments
fabio workspace show-role-assignment   Show a specific role assignment
fabio workspace add-role-assignment    Add a role assignment
fabio workspace update-role-assignment Update a role assignment
fabio workspace delete-role-assignment Delete a role assignment
fabio workspace list-folders       List workspace folders
fabio workspace create-folder      Create a folder in a workspace
fabio workspace show-folder        Show folder details
fabio workspace update-folder      Update a folder
fabio workspace delete-folder      Delete a folder
fabio workspace move-folder        Move a folder to another parent (or root)
fabio workspace apply-tags         Apply tags to a workspace
fabio workspace unapply-tags       Remove tags from a workspace
fabio workspace assign-to-domain   Assign workspace to a domain
fabio workspace unassign-from-domain Unassign workspace from its domain
fabio workspace get-onelake-settings Get OneLake settings
fabio workspace modify-default-tier  Modify OneLake default tier (Hot/Cold)
fabio workspace modify-diagnostics   Modify OneLake diagnostics configuration
fabio workspace modify-immutability-policy Modify OneLake immutability policy
fabio workspace export-lifecycle-policy Export OneLake lifecycle policy
fabio workspace import-lifecycle-policy Import OneLake lifecycle policy
fabio workspace reset-shortcut-cache   Reset OneLake shortcut cache
fabio workspace get-network-policy     Get network communication policy
fabio workspace set-network-policy     Set network communication policy

fabio item list              List items in a workspace (--type, --folder, --recursive)
fabio item show              Show item details
fabio item create            Create a new item
fabio item update            Update item properties (name/description)
fabio item delete            Delete an item (--hard-delete for permanent)
fabio item copy              Copy an item to another workspace
fabio item move              Move an item to another workspace (copy + delete)
fabio item move-to-folder    Move an item to a folder (or root)
fabio item get-definition    Get item definition (source code/content)
fabio item update-definition Update item definition from file(s)
fabio item list-connections  List connections used by an item
fabio item exists            Check if an item exists (returns {exists: true/false})
fabio item url               Get Fabric portal URL for an item
fabio item inspect           Get metadata + definition + connections in one call
fabio item apply-tags        Apply tags to an item
fabio item unapply-tags      Remove tags from an item
fabio item bulk-create       Create multiple items in parallel
fabio item bulk-delete       Delete multiple items in parallel
fabio item bulk-export-definitions Bulk export item definitions (LRO)
fabio item bulk-import-definitions Bulk import item definitions (LRO)
fabio item bulk-move         Bulk move items to another workspace (LRO)
fabio item list-external-data-shares   List external data shares for an item
fabio item create-external-data-share  Create an external data share
fabio item show-external-data-share    Show external data share details
fabio item revoke-external-data-share  Revoke an external data share
fabio item delete-external-data-share  Delete an external data share
fabio item assign-identity   Assign a managed identity to an item
fabio item get-invitation    Get an external data share invitation
fabio item accept-invitation Accept an external data share invitation

fabio lakehouse list         List lakehouses in a workspace
fabio lakehouse show         Show lakehouse details
fabio lakehouse create       Create a new lakehouse
fabio lakehouse update       Update a lakehouse (rename/redescribe)
fabio lakehouse delete       Delete a lakehouse
fabio lakehouse list-tables  List tables in a lakehouse
fabio lakehouse list-files   List files in a lakehouse
fabio lakehouse upload       Upload files (supports glob patterns, parallel)
fabio lakehouse download     Download a file from a lakehouse
fabio lakehouse upload-table Upload a file and load it into a Delta table (one step)
fabio lakehouse load-table   Load an existing file into a Delta table (--schema)
fabio lakehouse query        Execute T-SQL via SQL analytics endpoint
fabio lakehouse table-schema Read Delta table schema from OneLake (no Spark)
fabio lakehouse optimize-table Run V-Order + Z-Order optimization
fabio lakehouse vacuum-table Remove old files (retention period)
fabio lakehouse copy-file    Copy files between lakehouses (glob, parallel)
fabio lakehouse move-file    Move files (atomic rename for same-item, copy+delete for cross-item)
fabio lakehouse delete-file  Delete a file
fabio lakehouse copy-table   Copy a table between lakehouses
fabio lakehouse move-table   Move a table (atomic rename for same-item, copy+delete for cross-item)
fabio lakehouse delete-table Delete a table
fabio lakehouse sync         Sync files between lakehouses or from local (--local) with ETag/MD5, dedup, rename detection, include/exclude, rsync-inspired flags
fabio lakehouse create-shortcut      Create a shortcut (OneLake/ADLS/S3, --conflict-policy)
fabio lakehouse get-shortcut         Get shortcut details
fabio lakehouse delete-shortcut      Delete a shortcut
fabio lakehouse bulk-create-shortcuts Bulk-create multiple shortcuts (LRO)
fabio lakehouse get-definition       Get lakehouse definition
fabio lakehouse update-definition    Update lakehouse definition
fabio lakehouse refresh-materialized-views Trigger materialized view refresh
fabio lakehouse create-materialized-views-schedule Create refresh schedule
fabio lakehouse update-materialized-views-schedule Update refresh schedule
fabio lakehouse delete-materialized-views-schedule Delete refresh schedule
fabio lakehouse run-table-maintenance Run table maintenance job
fabio lakehouse list-livy-sessions   List Livy sessions
fabio lakehouse get-livy-session     Get Livy session details

fabio capacity list          List available capacities
fabio capacity show          Show capacity details
fabio capacity suspend       Suspend (pause) a capacity
fabio capacity resume        Resume a suspended capacity
fabio capacity create        Create a new capacity (ARM)
fabio capacity update        Update capacity properties (ARM)
fabio capacity delete        Delete a capacity (ARM)
fabio capacity list-skus     List available SKUs and regions
fabio capacity check-name    Check capacity name availability

fabio catalog search         Search items across the tenant
```

## Data & Compute

```
fabio notebook list          List notebooks in a workspace
fabio notebook show          Show notebook details
fabio notebook create        Create a new notebook (--lakehouse for binding)
fabio notebook update        Update notebook properties (name/description)
fabio notebook delete        Delete a notebook
fabio notebook get-definition   Get notebook source code (--strip-output)
fabio notebook update-definition Update notebook source
fabio notebook run           Run a notebook (--wait, --timeout, --parameters)
fabio notebook status        Check run status
fabio notebook get-job-instance Get details of a specific job instance
fabio notebook stop          Stop a running notebook
fabio notebook list-livy-sessions List Livy sessions for a notebook
fabio notebook get-livy-session   Get Livy session details

fabio warehouse list         List warehouses in a workspace
fabio warehouse show         Show warehouse details
fabio warehouse create       Create a warehouse
fabio warehouse update       Update warehouse properties (name/description)
fabio warehouse delete       Delete a warehouse
fabio warehouse query        Execute SQL (--sql, @file, or stdin)
fabio warehouse connection-string Get TDS connection string
fabio warehouse get-sql-pools-config Get SQL pools configuration
fabio warehouse update-sql-pools-config Update SQL pools configuration
fabio warehouse get-audit-settings Get SQL audit settings
fabio warehouse update-audit-settings Update SQL audit settings
fabio warehouse set-audit-actions Set audit actions and groups
fabio warehouse list-restore-points List restore points
fabio warehouse create-restore-point Create a restore point
fabio warehouse show-restore-point Show restore point details
fabio warehouse update-restore-point Update a restore point
fabio warehouse delete-restore-point Delete a restore point
fabio warehouse restore-to-point Restore a warehouse to a point

fabio warehouse-snapshot list   List warehouse snapshots
fabio warehouse-snapshot show   Show snapshot details
fabio warehouse-snapshot create Create a snapshot (--warehouse-id)
fabio warehouse-snapshot update Update snapshot properties
fabio warehouse-snapshot delete Delete a snapshot

fabio sql-database list      List SQL databases in a workspace
fabio sql-database show      Show SQL database details
fabio sql-database create    Create a SQL database
fabio sql-database update    Update SQL database properties
fabio sql-database delete    Delete a SQL database
fabio sql-database query     Execute SQL (--sql, @file, or stdin) via TDS
fabio sql-database connection-string Get TDS connection string
fabio sql-database import    Import CSV/JSON into a table (type inference)
fabio sql-database get-definition   Get definition (dacpac/sqlproj format)
fabio sql-database update-definition Update definition
fabio sql-database start-mirroring Start mirroring
fabio sql-database stop-mirroring  Stop mirroring
fabio sql-database revalidate-cmk  Revalidate Customer-Managed Key
fabio sql-database get-audit-settings Get SQL audit settings
fabio sql-database update-audit-settings Update SQL audit settings
fabio sql-database list-deleted List restorable deleted databases

fabio sql-endpoint list      List SQL analytics endpoints
fabio sql-endpoint show      Show endpoint details
fabio sql-endpoint connection-string Get TDS connection string
fabio sql-endpoint refresh-metadata  Refresh table sync metadata (LRO)
fabio sql-endpoint get-audit-settings  Get audit configuration
fabio sql-endpoint update-audit-settings Update audit settings
fabio sql-endpoint set-audit-actions    Set audit action groups

fabio data-agent list        List data agents
fabio data-agent show        Show data agent details
fabio data-agent create      Create a new data agent
fabio data-agent update      Update name/description
fabio data-agent delete      Delete a data agent
fabio data-agent query       Chat with a published data agent
fabio data-agent get-definition   Get definition (configuration, data sources)
fabio data-agent update-definition Update definition (instructions, data sources)
fabio data-agent publish     Publish a data agent (promotes draft to published)

fabio ontology list          List ontologies
fabio ontology show          Show ontology details
fabio ontology create        Create an ontology
fabio ontology update        Update ontology properties
fabio ontology delete        Delete an ontology
fabio ontology get-definition   Get definition (--decode for readable output)
fabio ontology update-definition Update definition (--dir for folder format)

fabio environment list       List environments in a workspace
fabio environment show       Show environment details
fabio environment create     Create an environment
fabio environment update     Update environment properties
fabio environment delete     Delete an environment
fabio environment publish    Publish staged changes
fabio environment cancel-publish Cancel a pending publish
fabio environment get-spark-settings Get published Spark settings
fabio environment get-staging-spark-settings Get staging (draft) settings
fabio environment upload-staging-library Upload a library to staging (.whl/.jar/.tar.gz)
fabio environment get-definition   Get environment definition
fabio environment update-definition Update environment definition
fabio environment list-libraries   List published libraries
fabio environment export-libraries Export external libraries config (published)
fabio environment list-staging-libraries List staging libraries
fabio environment delete-staging-library Delete a staging library
fabio environment export-staging-libraries Export external libraries (staging)
fabio environment import-staging-libraries Import external libraries into staging
fabio environment remove-staging-library Remove external library from staging
fabio environment update-staging-spark-compute Update staging Spark config

fabio data-pipeline list     List data pipelines
fabio data-pipeline show     Show pipeline details
fabio data-pipeline create   Create a data pipeline
fabio data-pipeline update   Update pipeline properties
fabio data-pipeline delete   Delete a data pipeline
fabio data-pipeline run      Run a data pipeline
fabio data-pipeline get-definition   Get pipeline definition
fabio data-pipeline update-definition Update pipeline definition
fabio data-pipeline create-schedule  Create a pipeline schedule

fabio copy-job list          List copy jobs
fabio copy-job show          Show copy job details
fabio copy-job create        Create a copy job
fabio copy-job update        Update copy job properties
fabio copy-job delete        Delete a copy job
fabio copy-job get-definition   Get copy job definition
fabio copy-job update-definition Update copy job definition
fabio copy-job reset         Reset copy job entities (--all or --entity-ids)

fabio dataflow list          List dataflows
fabio dataflow show          Show dataflow details
fabio dataflow create        Create a dataflow
fabio dataflow update        Update dataflow properties
fabio dataflow delete        Delete a dataflow
fabio dataflow get-definition   Get dataflow definition
fabio dataflow update-definition Update dataflow definition
fabio dataflow discover-parameters Discover M parameters
fabio dataflow run           Run a dataflow (--wait, --job-type execute|apply-changes)
fabio dataflow execute-query Execute a named query (returns Arrow IPC binary)

fabio app-backend list       List app backends in a workspace [preview]
fabio app-backend show       Show app backend details
fabio app-backend create     Create an app backend (LRO)
fabio app-backend update     Update app backend properties (name/description)
fabio app-backend delete     Delete an app backend (--hard-delete for permanent)

fabio data-build-tool-job list List data build tool jobs [preview]
fabio data-build-tool-job show Show data build tool job details
fabio data-build-tool-job create Create a data build tool job
fabio data-build-tool-job update Update data build tool job properties
fabio data-build-tool-job delete Delete a data build tool job (--hard-delete)
fabio data-build-tool-job get-definition   Get definition
fabio data-build-tool-job update-definition Update definition
fabio data-build-tool-job run  Run a data build tool job (--wait, --timeout, --cancel-on-timeout)

fabio org-app list           List organizational apps
fabio org-app show           Show org app details
fabio org-app create         Create an org app
fabio org-app update         Update org app properties
fabio org-app delete         Delete an org app (--hard-delete)
fabio org-app get-definition Get org app definition
fabio org-app update-definition Update org app definition

fabio org-app-audience list  List org app audiences
fabio org-app-audience show  Show org app audience details
fabio org-app-audience create Create an org app audience
fabio org-app-audience update Update org app audience properties
fabio org-app-audience delete Delete an org app audience (--hard-delete)
fabio org-app-audience get-definition Get org app audience definition
fabio org-app-audience update-definition Update org app audience definition
```

## Analytics & Reporting

```
fabio report list            List reports
fabio report show            Show report details
fabio report create          Create a report (--dataset to bind semantic model)
fabio report update          Update report properties
fabio report delete          Delete a report
fabio report get-definition  Get report definition
fabio report update-definition Update report definition
fabio report publish-to-web  Publish report to the web (public embed URL)

fabio semantic-model list    List semantic models
fabio semantic-model show    Show semantic model details
fabio semantic-model create  Create from TMDL or model.bim
fabio semantic-model update  Update properties
fabio semantic-model delete  Delete a semantic model
fabio semantic-model get-definition    Get definition
fabio semantic-model update-definition Update definition
fabio semantic-model query   Execute a DAX query
fabio semantic-model bind-connection Bind to a connection
fabio semantic-model unbind-connection Unbind from a connection
fabio semantic-model refresh Refresh (frame Direct Lake models)
fabio semantic-model takeover Convert definition-managed to service-managed
fabio semantic-model list-parameters   List M parameters (Power BI API)
fabio semantic-model update-parameters Update M parameters
fabio semantic-model list-datasources  List data sources
fabio semantic-model update-datasources Update data sources
fabio semantic-model list-users        List dataset permissions
fabio semantic-model add-user          Add a user/principal
fabio semantic-model delete-user       Remove a user/principal
fabio semantic-model refresh-status    View refresh history
fabio semantic-model list-upstream     Show upstream dependencies
fabio semantic-model clone             Clone a dataset (same/cross-workspace)
fabio semantic-model export-pbix       Download as .pbix binary
fabio semantic-model import-pbix       Upload .pbix file

fabio paginated-report list  List paginated reports
fabio paginated-report update Update paginated report properties

fabio dashboard list         List dashboards

fabio datamart list          List datamarts
```

## Real-Time Intelligence

```
fabio eventhouse list        List eventhouses
fabio eventhouse show        Show eventhouse details
fabio eventhouse create      Create an eventhouse
fabio eventhouse update      Update eventhouse properties
fabio eventhouse delete      Delete an eventhouse
fabio eventhouse get-definition   Get definition
fabio eventhouse update-definition Update definition

fabio eventstream list       List eventstreams
fabio eventstream show       Show eventstream details
fabio eventstream create     Create an eventstream
fabio eventstream update     Update eventstream properties
fabio eventstream delete     Delete an eventstream
fabio eventstream get-definition   Get definition
fabio eventstream update-definition Update definition
fabio eventstream get-topology     Get eventstream topology
fabio eventstream pause      Pause the entire eventstream
fabio eventstream resume     Resume the entire eventstream
fabio eventstream get-source Get source details
fabio eventstream get-source-connection Get source connection info
fabio eventstream pause-source   Pause a source
fabio eventstream resume-source  Resume a source
fabio eventstream get-destination Get destination details
fabio eventstream get-destination-connection Get destination connection info
fabio eventstream pause-destination Pause a destination
fabio eventstream resume-destination Resume a destination
fabio eventstream add-source Add a source (fetches definition, merges, updates)
fabio eventstream add-destination Add a destination (same pattern)

fabio kql-database list      List KQL databases
fabio kql-database show      Show KQL database details
fabio kql-database create    Create a KQL database (--eventhouse-id)
fabio kql-database update    Update KQL database properties
fabio kql-database delete    Delete a KQL database
fabio kql-database query     Execute KQL queries (--kql)
fabio kql-database get-definition   Get definition
fabio kql-database update-definition Update definition
fabio kql-database list-shortcuts   List shortcuts in a KQL database
fabio kql-database create-shortcut  Create a shortcut
fabio kql-database get-shortcut     Get shortcut details
fabio kql-database delete-shortcut  Delete a shortcut
fabio kql-database bulk-create-shortcuts Bulk-create shortcuts (LRO)

fabio kql-queryset list      List KQL querysets
fabio kql-queryset show      Show KQL queryset details
fabio kql-queryset create    Create a KQL queryset
fabio kql-queryset update    Update KQL queryset properties
fabio kql-queryset delete    Delete a KQL queryset
fabio kql-queryset get-definition   Get definition
fabio kql-queryset update-definition Update definition
fabio kql-queryset run       Run a saved query tab against its data source

fabio kql-dashboard list     List KQL dashboards
fabio kql-dashboard show     Show KQL dashboard details
fabio kql-dashboard create   Create a KQL dashboard
fabio kql-dashboard update   Update KQL dashboard properties
fabio kql-dashboard delete   Delete a KQL dashboard
fabio kql-dashboard get-definition   Get definition
fabio kql-dashboard update-definition Update definition

fabio reflex list            List reflexes (Data Activator)
fabio reflex show            Show reflex details
fabio reflex create          Create a reflex
fabio reflex update          Update reflex properties
fabio reflex delete          Delete a reflex
fabio reflex get-definition  Get definition (ReflexEntities.json)
fabio reflex update-definition Update definition
fabio reflex configure-kql-source Configure a KQL data source

fabio anomaly-detector list  List anomaly detectors
fabio anomaly-detector show  Show anomaly detector details
fabio anomaly-detector create Create an anomaly detector
fabio anomaly-detector update Update properties
fabio anomaly-detector delete Delete an anomaly detector
fabio anomaly-detector get-definition   Get definition
fabio anomaly-detector update-definition Update definition

fabio event-schema-set list  List event schema sets
fabio event-schema-set show  Show event schema set details
fabio event-schema-set create Create an event schema set
fabio event-schema-set update Update properties
fabio event-schema-set delete Delete an event schema set
fabio event-schema-set get-definition   Get definition
fabio event-schema-set update-definition Update definition

fabio rti nl-to-kql          Translate natural language to KQL (AI-powered)
```

## Data Science & AI

```
fabio ml-model list          List ML models
fabio ml-model show          Show ML model details
fabio ml-model create        Create an ML model
fabio ml-model update        Update ML model properties
fabio ml-model delete        Delete an ML model
fabio ml-model get-endpoint  Get model serving endpoint configuration
fabio ml-model update-endpoint Update model serving endpoint
fabio ml-model score         Score (invoke) a deployed model
fabio ml-model list-versions List endpoint versions
fabio ml-model get-version   Get version details
fabio ml-model update-version Update a version
fabio ml-model activate-version Activate a version
fabio ml-model deactivate-version Deactivate a version
fabio ml-model score-version Score a specific version
fabio ml-model deactivate-all-versions Deactivate all versions

fabio ml-experiment list     List ML experiments
fabio ml-experiment show     Show ML experiment details
fabio ml-experiment create   Create an ML experiment
fabio ml-experiment update   Update ML experiment properties
fabio ml-experiment delete   Delete an ML experiment

fabio operations-agent list  List operations agents
fabio operations-agent show  Show operations agent details
fabio operations-agent create Create an operations agent
fabio operations-agent update Update properties
fabio operations-agent delete Delete an operations agent
fabio operations-agent get-definition   Get definition (Configurations.json)
fabio operations-agent update-definition Update definition
```

## Spark

```
fabio spark get-settings     Get workspace-level Spark settings
fabio spark update-settings  Update workspace-level Spark settings
fabio spark list-pools       List custom Spark pools in a workspace
fabio spark get-pool         Get pool details
fabio spark create-pool      Create a custom Spark pool
fabio spark update-pool      Update a custom pool
fabio spark delete-pool      Delete a custom pool
fabio spark get-capacity-settings Get capacity-level Spark settings
fabio spark update-capacity-settings Update capacity-level Spark settings
fabio spark list-capacity-pools List custom Spark pools in a capacity
fabio spark create-capacity-pool Create a capacity Spark pool
fabio spark get-capacity-pool Get capacity pool details
fabio spark update-capacity-pool Update a capacity pool
fabio spark delete-capacity-pool Delete a capacity pool
fabio spark list-livy-sessions List Livy sessions in a workspace
fabio spark get-livy-session Get Livy session details

fabio spark-job-definition list   List Spark job definitions
fabio spark-job-definition show   Show details
fabio spark-job-definition create Create a Spark job definition
fabio spark-job-definition update Update properties
fabio spark-job-definition delete Delete a Spark job definition
fabio spark-job-definition get-definition   Get definition
fabio spark-job-definition update-definition Update definition
fabio spark-job-definition run    Run a Spark job

fabio apache-airflow-job list         List Airflow jobs
fabio apache-airflow-job show         Show Airflow job details
fabio apache-airflow-job create       Create an Airflow job
fabio apache-airflow-job update       Update Airflow job properties
fabio apache-airflow-job delete       Delete an Airflow job
fabio apache-airflow-job get-definition    Get definition
fabio apache-airflow-job update-definition Update definition
fabio apache-airflow-job start-environment Start Airflow runtime
fabio apache-airflow-job stop-environment  Stop Airflow runtime
fabio apache-airflow-job get-environment   Get environment status
fabio apache-airflow-job list-libraries    List installed libraries
fabio apache-airflow-job deploy-requirements Deploy pip requirements
fabio apache-airflow-job get-settings      Get environment settings
fabio apache-airflow-job update-settings   Update environment settings
fabio apache-airflow-job get-compute       Get compute configuration
fabio apache-airflow-job list-files        List DAG files
fabio apache-airflow-job get-file          Download a file
fabio apache-airflow-job upload-file       Upload a file
fabio apache-airflow-job delete-file       Delete a file
fabio apache-airflow-job get-workspace-settings Get workspace Airflow settings
fabio apache-airflow-job update-workspace-settings Update workspace settings
fabio apache-airflow-job list-pool-templates List pool templates
fabio apache-airflow-job create-pool-template Create a pool template
fabio apache-airflow-job get-pool-template   Get a pool template
fabio apache-airflow-job delete-pool-template Delete a pool template
```

## Graph & Digital Twins

```
fabio graphql-api list       List GraphQL APIs
fabio graphql-api show       Show GraphQL API details
fabio graphql-api create     Create a GraphQL API
fabio graphql-api update     Update GraphQL API properties
fabio graphql-api delete     Delete a GraphQL API
fabio graphql-api get-definition   Get definition (schema.graphql)
fabio graphql-api update-definition Update definition
fabio graphql-api query      Execute a GraphQL query

fabio graph-model list       List graph models
fabio graph-model show       Show graph model details
fabio graph-model create     Create a graph model (--ontology)
fabio graph-model update     Update graph model properties
fabio graph-model delete     Delete a graph model
fabio graph-model get-definition   Get definition
fabio graph-model update-definition Update definition
fabio graph-model refresh    Trigger a graph refresh job
fabio graph-model execute-query Run a graph query (KQL)
fabio graph-model get-queryable-graph-type Get queryable type
fabio graph-model initialize Initialize a graph model for querying

fabio graph-query-set list   List graph query sets
fabio graph-query-set show   Show graph query set details
fabio graph-query-set create Create a graph query set
fabio graph-query-set update Update properties
fabio graph-query-set delete Delete a graph query set
fabio graph-query-set get-definition   Get definition
fabio graph-query-set update-definition Update definition

fabio digital-twin-builder list   List digital twin builders
fabio digital-twin-builder show   Show details
fabio digital-twin-builder create Create a digital twin builder
fabio digital-twin-builder update Update properties
fabio digital-twin-builder delete Delete a digital twin builder
fabio digital-twin-builder get-definition   Get definition
fabio digital-twin-builder update-definition Update definition

fabio digital-twin-builder-flow list   List DTB flows
fabio digital-twin-builder-flow show   Show flow details
fabio digital-twin-builder-flow create Create a flow (--dtb-id)
fabio digital-twin-builder-flow update Update flow properties
fabio digital-twin-builder-flow delete Delete a flow
fabio digital-twin-builder-flow get-definition   Get definition
fabio digital-twin-builder-flow update-definition Update definition

fabio map list               List maps (geospatial visualization)
fabio map show               Show map details
fabio map create             Create a map
fabio map update             Update map properties
fabio map delete             Delete a map
fabio map get-definition     Get definition (map.json)
fabio map update-definition  Update definition
```

## Mirroring & External Data

```
fabio mirrored-database list   List mirrored databases
fabio mirrored-database show   Show mirrored database details
fabio mirrored-database create Create a mirrored database
fabio mirrored-database update Update properties
fabio mirrored-database delete Delete a mirrored database
fabio mirrored-database get-definition   Get definition
fabio mirrored-database update-definition Update definition
fabio mirrored-database start  Start mirroring
fabio mirrored-database stop   Stop mirroring
fabio mirrored-database status Get mirroring status
fabio mirrored-database table-status Get table mirroring status

fabio mirrored-catalog list  List mirrored catalogs
fabio mirrored-catalog show  Show mirrored catalog details
fabio mirrored-catalog create Create a mirrored catalog
fabio mirrored-catalog update Update properties
fabio mirrored-catalog delete Delete a mirrored catalog
fabio mirrored-catalog get-definition   Get definition
fabio mirrored-catalog update-definition Update definition
fabio mirrored-catalog refresh-metadata Refresh catalog metadata
fabio mirrored-catalog list-scopes      List catalog mirroring scopes
fabio mirrored-catalog list-tables      List catalog mirroring tables
fabio mirrored-catalog mirroring-status Get mirroring status
fabio mirrored-catalog tables-mirroring-status Get tables mirroring status

fabio mirrored-databricks-catalog list   List Databricks catalogs
fabio mirrored-databricks-catalog show   Show catalog details
fabio mirrored-databricks-catalog create Create a Databricks catalog
fabio mirrored-databricks-catalog update Update properties
fabio mirrored-databricks-catalog delete Delete a catalog
fabio mirrored-databricks-catalog get-definition   Get definition
fabio mirrored-databricks-catalog update-definition Update definition
fabio mirrored-databricks-catalog refresh-metadata Refresh catalog metadata
fabio mirrored-databricks-catalog discover-catalogs Discover available catalogs
fabio mirrored-databricks-catalog discover-schemas  Discover schemas in a catalog
fabio mirrored-databricks-catalog discover-tables   Discover tables in a schema

fabio mirrored-warehouse list  List mirrored warehouses

fabio cosmos-db-database list  List Cosmos DB databases
fabio cosmos-db-database show  Show Cosmos DB database details
fabio cosmos-db-database create Create a Cosmos DB database
fabio cosmos-db-database update Update properties
fabio cosmos-db-database delete Delete a Cosmos DB database
fabio cosmos-db-database get-definition   Get definition
fabio cosmos-db-database update-definition Update definition

fabio snowflake-database list  List Snowflake databases
fabio snowflake-database show  Show Snowflake database details
fabio snowflake-database create Create a Snowflake database
fabio snowflake-database update Update properties
fabio snowflake-database delete Delete a Snowflake database
fabio snowflake-database get-definition   Get definition
fabio snowflake-database update-definition Update definition

fabio mounted-data-factory list  List mounted data factories
fabio mounted-data-factory show  Show details
fabio mounted-data-factory create Create (--adf-resource-id)
fabio mounted-data-factory update Update properties
fabio mounted-data-factory delete Delete a mounted data factory
fabio mounted-data-factory get-definition   Get definition
fabio mounted-data-factory update-definition Update definition

fabio variable-library list  List variable libraries
fabio variable-library show  Show variable library details
fabio variable-library create Create a variable library
fabio variable-library update Update properties
fabio variable-library delete Delete a variable library
fabio variable-library get-definition   Get definition
fabio variable-library update-definition Update definition

fabio user-data-function list  List user data functions
fabio user-data-function show  Show function details
fabio user-data-function create Create a user data function
fabio user-data-function update Update properties
fabio user-data-function delete Delete a function
fabio user-data-function get-definition   Get definition
fabio user-data-function update-definition Update definition
```

## Integration & DevOps

```
fabio git status             Show workspace Git status (changes, conflicts)
fabio git commit             Commit workspace changes to remote
fabio git pull               Pull remote changes into workspace
fabio git connect            Connect a workspace to a Git repo
fabio git disconnect         Disconnect a workspace from Git
fabio git init               Initialize Git connection (required after connect)
fabio git checkout           Switch to a different branch (disconnect + connect + init)
fabio git connection show    Show Git connection details
fabio git credentials show   Show Git credentials configuration
fabio git credentials update Update Git credentials configuration
fabio git show-tracked       Show tracked items and Git sync status

fabio connection list        List all connections
fabio connection show        Show connection details
fabio connection create      Create a new connection
fabio connection update      Update connection (name, credentials, privacy)
fabio connection delete      Delete a connection
fabio connection list-supported-types List supported connection types
fabio connection test-connection Test a connection
fabio connection list-role-assignments List role assignments
fabio connection add-role-assignment Add a role assignment
fabio connection show-role-assignment Show a role assignment
fabio connection update-role-assignment Update a role assignment
fabio connection delete-role-assignment Delete a role assignment

fabio deployment-pipeline list   List deployment pipelines
fabio deployment-pipeline show   Show pipeline details
fabio deployment-pipeline create Create a pipeline
fabio deployment-pipeline update Update pipeline properties
fabio deployment-pipeline delete Delete a pipeline
fabio deployment-pipeline list-stages List stages
fabio deployment-pipeline show-stage  Show stage details
fabio deployment-pipeline update-stage Update stage configuration
fabio deployment-pipeline list-stage-items List items in a stage
fabio deployment-pipeline assign-workspace   Assign workspace to a stage
fabio deployment-pipeline unassign-workspace Unassign workspace
fabio deployment-pipeline list-operations   List deploy operation history
fabio deployment-pipeline show-operation    Show deploy operation details
fabio deployment-pipeline list-role-assignments List role assignments
fabio deployment-pipeline add-role-assignment   Add a role assignment
fabio deployment-pipeline delete-role-assignment Delete a role assignment
fabio deployment-pipeline deploy Deploy items between stages

fabio domain list            List domains in the tenant
fabio domain show            Show domain details
fabio domain create          Create a domain
fabio domain update          Update domain properties
fabio domain delete          Delete a domain
fabio domain list-workspaces List workspaces in a domain
fabio domain assign-workspaces   Assign workspaces to a domain
fabio domain unassign-workspaces Unassign workspaces
fabio domain assign-by-capacity  Bulk-assign workspaces by capacity
fabio domain assign-by-principal Bulk-assign workspaces by principal

fabio job-scheduler list-instances List job instances for an item
fabio job-scheduler get-instance   Get job instance details
fabio job-scheduler run-on-demand  Run an on-demand job
fabio job-scheduler cancel-instance Cancel a running instance
fabio job-scheduler list-schedules List schedules for an item
fabio job-scheduler get-schedule   Get schedule details
fabio job-scheduler create-schedule Create a schedule
fabio job-scheduler update-schedule Update a schedule
fabio job-scheduler delete-schedule Delete a schedule

fabio deploy plan            Plan deployment (diff source directory vs live workspace)
fabio deploy apply           Apply deployment (create/update/rename/delete items)
fabio deploy export          Export a workspace to a source directory
fabio deploy init-params     Generate parameter file from GUIDs/diffs
fabio deploy validate        Validate source directory offline (no API calls)
```

## Security & Governance

```
fabio onelake-security list  List data access roles
fabio onelake-security show  Show a data access role
fabio onelake-security create Create a data access role (--conflict-policy)
fabio onelake-security upsert Create or replace all roles
fabio onelake-security delete Delete a data access role

fabio managed-private-endpoint list   List managed private endpoints
fabio managed-private-endpoint show   Show endpoint details
fabio managed-private-endpoint create Create a managed private endpoint
fabio managed-private-endpoint delete Delete a managed private endpoint

fabio gateway list           List gateways
fabio gateway show           Show gateway details
fabio gateway create         Create a VNet gateway
fabio gateway update         Update gateway properties
fabio gateway delete         Delete a gateway
fabio gateway list-members   List gateway members
fabio gateway update-member  Update a gateway member
fabio gateway delete-member  Delete a gateway member
fabio gateway list-role-assignments   List role assignments
fabio gateway add-role-assignment     Add a role assignment
fabio gateway show-role-assignment    Show a role assignment
fabio gateway update-role-assignment  Update a role assignment
fabio gateway delete-role-assignment  Delete a role assignment
fabio gateway check-status   Check gateway connectivity status
fabio gateway check-member-status Check gateway member connectivity status
fabio gateway restart        Restart a gateway (LRO, requires Admin)
fabio gateway shutdown       Shut down a gateway (LRO, requires Admin)
```

## Administration

```
fabio admin list-tenant-settings List all tenant settings
fabio admin update-tenant-setting Update a tenant setting
fabio admin list-capacities-tenant-overrides List all capacity overrides
fabio admin list-capacity-tenant-overrides List overrides for a capacity
fabio admin delete-capacity-tenant-override Delete a capacity override
fabio admin update-capacity-tenant-override Update a capacity override
fabio admin list-domains-tenant-overrides List all domain overrides
fabio admin list-workspaces-tenant-overrides List all workspace overrides
fabio admin list-tags        List tags
fabio admin create-tags      Bulk-create tags
fabio admin update-tag       Update a tag
fabio admin delete-tag       Delete a tag
fabio admin list-workloads   List workloads
fabio admin list-workload-assignments List workload assignments
fabio admin create-workload-assignment Create a workload assignment
fabio admin delete-workload-assignment Delete a workload assignment
fabio admin list-workspaces  List workspaces (admin view)
fabio admin show-workspace   Show workspace details (admin)
fabio admin list-workspace-users List users in a workspace (admin)
fabio admin list-git-connections List git connections across workspaces
fabio admin grant-admin-access Grant temporary admin access
fabio admin remove-admin-access Remove temporary admin access
fabio admin restore-workspace Restore a deleted workspace
fabio admin list-network-policies List network policies
fabio admin list-items       List items (admin view)
fabio admin show-item        Show item details (admin)
fabio admin list-item-users  List users with access to an item (admin)
fabio admin bulk-set-labels  Bulk-set sensitivity labels on items
fabio admin bulk-remove-labels Bulk-remove sensitivity labels
fabio admin list-external-data-shares List external data shares
fabio admin revoke-external-data-share Revoke an external data share
fabio admin remove-all-sharing-links Remove all sharing links for items
fabio admin bulk-remove-sharing-links Bulk-remove sharing links
fabio admin list-domains     List domains (admin view)
fabio admin create-domain    Create a domain
fabio admin show-domain      Show domain details
fabio admin update-domain    Update a domain
fabio admin delete-domain    Delete a domain
fabio admin list-domain-workspaces List workspaces in a domain
fabio admin assign-domain-workspaces Assign workspaces to a domain
fabio admin unassign-domain-workspaces Unassign workspaces from domain
fabio admin unassign-all-domain-workspaces Unassign all from domain
fabio admin list-domain-role-assignments List domain role assignments
fabio admin bulk-assign-domain-roles Bulk-assign domain roles
fabio admin bulk-unassign-domain-roles Bulk-unassign domain roles
fabio admin sync-domain-roles-to-subdomains Sync roles to subdomains
fabio admin assign-domain-workspaces-by-capacities Assign by capacities
fabio admin assign-domain-workspaces-by-principals Assign by principals
fabio admin list-user-access List access details for a user
```

## Configuration & Tooling

```
fabio rest call              Raw REST API passthrough (Fabric or Power BI)

fabio profile save           Save a named profile with default settings
fabio profile use            Set the active profile
fabio profile list           List all saved profiles
fabio profile show           Show profile details
fabio profile delete         Delete a profile

fabio jobs list              List recent jobs from local ledger
fabio jobs get               Get details of a specific job
fabio jobs prune             Remove completed/failed jobs

fabio feedback send          Record feedback about CLI friction
fabio feedback list          List recorded feedback entries

fabio operation get-state    Get state of a long-running operation
fabio operation get-result   Get result of a completed operation

fabio agent-context          Machine-readable command schema for AI agents

fabio completions <shell>    Generate shell completion scripts (bash/zsh/fish/powershell/elvish)
```
