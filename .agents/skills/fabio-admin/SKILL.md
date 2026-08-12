---
name: fabio-admin
description: >-
  Intent-scoped fabio skill for Fabric administration and workspace management: workspace lifecycle/roles/folders/networking/encryption, capacity lifecycle, tenant-wide inventory and settings, domains, gateways, connections, managed private endpoints, and sensitivity labels. Use for governance, connectivity, capacity, and workspace-level administration. Triggers: "workspace", "assign capacity", "workspace roles", "network policy", "capacity", "resume capacity", "suspend capacity", "tenant settings", "domain", "gateway", "connection", "private endpoint", "sensitivity label".
license: MIT
---

# fabio-admin — Administration — workspaces, capacity, tenant governance, connectivity, labels

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Workspace lifecycle and governance: create/assign-capacity, recover or permanently delete soft-deleted items, role assignments, folders, domains, networking/firewall/encryption policies, git outbound policy, OneLake settings.
- Capacity lifecycle: list, resume, suspend, create, delete (ARM-scoped).
- Tenant-wide inventory and settings (requires Fabric admin role).
- Governance: domains (group workspaces), sensitivity labels.
- Connectivity: gateways, connections, managed private endpoints.

## When NOT to use (route elsewhere)
- Building or transforming data -> use the data-engineer persona.
- Workspace-scoped item CRUD -> use the specific workload skill.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio workspace
Manage workspaces

| Command | Mutates | Description |
|---|---|---|
| `fabio workspace add-role-assignment` | yes | Add a role assignment to a workspace |
| `fabio workspace apply-tags` | yes | Apply tags to a workspace |
| `fabio workspace assign-capacity` | yes | Assign a workspace to a capacity |
| `fabio workspace assign-encryption` | yes | Assign a Customer-Managed Key (CMK) to a workspace, enabling or rotating encryption (Preview) |
| `fabio workspace assign-to-domain` | yes | Assign workspace to a domain |
| `fabio workspace clone` | yes | Clone workspace items from one workspace to another using bulk APIs |
| `fabio workspace create` | yes | Create a new workspace |
| `fabio workspace create-folder` | yes | Create a folder in a workspace |
| `fabio workspace delete` | yes | Delete a workspace |
| `fabio workspace delete-folder` | yes | Delete a workspace folder |
| `fabio workspace delete-recoverable-item` | yes | Permanently delete a recoverable item |
| `fabio workspace delete-role-assignment` | yes | Delete a workspace role assignment |
| `fabio workspace deprovision-identity` | yes | Deprovision a workspace identity |
| `fabio workspace export-lifecycle-policy` | no | Export `OneLake` lifecycle policy |
| `fabio workspace get-dataset-storage-format` | no | Get default dataset storage format via Power BI API |
| `fabio workspace get-encryption` | no | Get workspace Customer-Managed Key (CMK) encryption settings (Preview) |
| `fabio workspace get-firewall-rules` | no | Get workspace IP firewall rules |
| `fabio workspace get-git-outbound-policy` | no | Get workspace git outbound policy |
| `fabio workspace get-inbound-azure-resource-rules` | no | Get workspace inbound Azure resource instance rules |
| `fabio workspace get-inbound-external-data-shares-policy` | no | Get workspace inbound External Data Shares bypass policy |
| `fabio workspace get-network-policy` | no | Get workspace network communication policy |
| `fabio workspace get-onelake-settings` | no | Get `OneLake` settings for a workspace |
| `fabio workspace get-outbound-cloud-connection-rules` | no | Get workspace outbound cloud connection rules (requires OAP enabled) |
| `fabio workspace get-outbound-gateway-rules` | no | Get workspace outbound gateway rules (requires OAP enabled) |
| `fabio workspace get-settings` | no | Get workspace settings (properties including `automaticMetadataSync`) |
| `fabio workspace import-lifecycle-policy` | yes | Import `OneLake` lifecycle policy |
| `fabio workspace list` | no | List all workspaces |
| `fabio workspace list-folders` | no | List workspace folders |
| `fabio workspace list-recoverable-items` | no | List soft-deleted items that are still within their retention period |
| `fabio workspace list-role-assignments` | no | List workspace role assignments |
| `fabio workspace modify-default-tier` | yes | Modify `OneLake` default tier (Hot, Cool, or Cold) |
| `fabio workspace modify-diagnostics` | yes | Modify `OneLake` diagnostics configuration |
| `fabio workspace modify-immutability-policy` | yes | Modify `OneLake` immutability policy |
| `fabio workspace move-folder` | yes | Move a folder to another parent (or root) |
| `fabio workspace provision-identity` | yes | Provision a workspace identity (managed identity) |
| `fabio workspace recover-item` | yes | Recover a soft-deleted item and its recoverable descendants |
| `fabio workspace reset-encryption` | yes | Reset workspace encryption by removing the CMK configuration (reverts to Microsoft-managed keys) (Preview) |
| `fabio workspace reset-shortcut-cache` | yes | Reset `OneLake` shortcut cache for a workspace |
| `fabio workspace set-dataset-storage-format` | yes | Set default dataset storage format (Small or Large) via Power BI API |
| `fabio workspace set-firewall-rules` | yes | Set workspace IP firewall rules (replaces all existing rules) |
| `fabio workspace set-git-outbound-policy` | yes | Set workspace git outbound policy (requires Outbound Access Protection enabled) |
| `fabio workspace set-inbound-azure-resource-rules` | yes | Set workspace inbound Azure resource instance rules |
| `fabio workspace set-inbound-external-data-shares-policy` | yes | Set workspace inbound External Data Shares bypass policy (preview API, requires *admin* role) |
| `fabio workspace set-network-policy` | yes | Set workspace network communication policy |
| `fabio workspace set-outbound-cloud-connection-rules` | yes | Set workspace outbound cloud connection rules (requires OAP enabled) |
| `fabio workspace set-outbound-gateway-rules` | yes | Set workspace outbound gateway rules (requires OAP enabled) |
| `fabio workspace show` | no | Show details of a workspace |
| `fabio workspace show-folder` | no | Show details of a workspace folder |
| `fabio workspace show-role-assignment` | no | Show a specific workspace role assignment |
| `fabio workspace unapply-tags` | yes | Remove tags from a workspace |
| `fabio workspace unassign-capacity` | yes | Unassign a workspace from its capacity |
| `fabio workspace unassign-from-domain` | yes | Unassign workspace from its domain |
| `fabio workspace update` | yes | Update workspace properties (name and/or description) |
| `fabio workspace update-folder` | yes | Update a workspace folder |
| `fabio workspace update-role-assignment` | yes | Update a workspace role assignment |
| `fabio workspace update-settings` | yes | Update workspace settings (e.g. enable automatic metadata sync) |
| `fabio workspace url` | no | Get the Fabric portal URL for a workspace |

### fabio admin
Fabric tenant administration (settings, tags, workloads, users)

| Command | Mutates | Description |
|---|---|---|
| `fabio admin assign-domain-workspaces` | yes | Assign workspaces to a domain |
| `fabio admin assign-domain-workspaces-by-capacities` | yes | Assign workspaces to a domain by capacities |
| `fabio admin assign-domain-workspaces-by-principals` | yes | Assign workspaces to a domain by principals |
| `fabio admin bulk-assign-domain-roles` | yes | Bulk-assign roles to a domain |
| `fabio admin bulk-remove-labels` | yes | Bulk-remove sensitivity labels from items |
| `fabio admin bulk-remove-sharing-links` | yes | Bulk-remove sharing links |
| `fabio admin bulk-set-labels` | yes | Bulk-set sensitivity labels on items |
| `fabio admin bulk-unassign-domain-roles` | yes | Bulk-unassign roles from a domain |
| `fabio admin create-domain` | yes | Create a domain |
| `fabio admin create-tags` | yes | Bulk-create tags |
| `fabio admin create-workload-assignment` | yes | Create a workload assignment |
| `fabio admin delete-capacity-tenant-override` | yes | Delete a capacity delegated tenant setting override |
| `fabio admin delete-domain` | yes | Delete a domain |
| `fabio admin delete-tag` | yes | Delete a tag |
| `fabio admin delete-workload-assignment` | yes | Delete a workload assignment |
| `fabio admin grant-admin-access` | yes | Grant temporary admin access to a workspace |
| `fabio admin list-capacities-tenant-overrides` | no | List all capacities' delegated tenant setting overrides |
| `fabio admin list-capacity-tenant-overrides` | no | List delegated tenant setting overrides for a capacity |
| `fabio admin list-domain-role-assignments` | no | List role assignments for a domain |
| `fabio admin list-domain-workspaces` | no | List workspaces in a domain |
| `fabio admin list-domains` | no | List domains (admin view) |
| `fabio admin list-domains-tenant-overrides` | no | List all domains' delegated tenant setting overrides |
| `fabio admin list-external-data-shares` | no | List external data shares |
| `fabio admin list-git-connections` | no | List git connections across workspaces |
| `fabio admin list-item-users` | no | List users with access to an item (admin view) |
| `fabio admin list-items` | no | List items (admin view) |
| `fabio admin list-network-policies` | no | List network communication policies |
| `fabio admin list-tags` | no | List tags |
| `fabio admin list-tenant-settings` | no | List all tenant settings |
| `fabio admin list-user-access` | no | List access details for a user |
| `fabio admin list-workload-assignments` | no | List workload assignments |
| `fabio admin list-workloads` | no | List workloads |
| `fabio admin list-workspace-users` | no | List users in a workspace (admin view) |
| `fabio admin list-workspaces` | no | List workspaces (admin view) |
| `fabio admin list-workspaces-tenant-overrides` | no | List all workspaces' delegated tenant setting overrides |
| `fabio admin remove-admin-access` | yes | Remove temporary admin access from a workspace |
| `fabio admin remove-all-sharing-links` | yes | Remove all sharing links for specified items |
| `fabio admin restore-workspace` | yes | Restore a deleted workspace |
| `fabio admin revoke-external-data-share` | yes | Revoke an external data share |
| `fabio admin show-domain` | no | Show domain details |
| `fabio admin show-item` | no | Show item details (admin view) |
| `fabio admin show-workspace` | no | Show workspace details (admin view) |
| `fabio admin sync-domain-roles-to-subdomains` | yes | Sync domain role assignments to subdomains |
| `fabio admin unassign-all-domain-workspaces` | yes | Unassign all workspaces from a domain |
| `fabio admin unassign-domain-workspaces` | yes | Unassign workspaces from a domain |
| `fabio admin update-capacity-tenant-override` | yes | Update a capacity delegated tenant setting override |
| `fabio admin update-domain` | yes | Update a domain |
| `fabio admin update-tag` | yes | Update a tag |
| `fabio admin update-tenant-setting` | yes | Update a tenant setting |

### fabio capacity
List and inspect Fabric capacities

| Command | Mutates | Description |
|---|---|---|
| `fabio capacity check-name` | no | Check if a capacity name is available (ARM API) |
| `fabio capacity create` | yes | Create a new Fabric capacity (ARM API) |
| `fabio capacity delete` | yes | Delete a Fabric capacity (ARM API) |
| `fabio capacity list` | no | List capacities available to the caller (Fabric API) |
| `fabio capacity list-skus` | no | List available SKUs for Fabric capacities (ARM API) |
| `fabio capacity resume` | yes | Resume a suspended capacity (ARM API) |
| `fabio capacity show` | no | Show details of a specific capacity (Fabric API) |
| `fabio capacity suspend` | yes | Suspend (pause) a capacity (ARM API) |
| `fabio capacity update` | yes | Update an existing Fabric capacity (ARM API) |

### fabio domain
Manage domains (organize workspaces into business domains)

| Command | Mutates | Description |
|---|---|---|
| `fabio domain assign-by-capacity` | yes | Bulk-assign all workspaces by capacity to a domain |
| `fabio domain assign-by-principal` | yes | Bulk-assign all workspaces by principal to a domain |
| `fabio domain assign-workspaces` | yes | Assign workspaces to a domain |
| `fabio domain create` | yes | Create a new domain |
| `fabio domain delete` | yes | Delete a domain |
| `fabio domain list` | no | List domains in the tenant |
| `fabio domain list-workspaces` | no | List workspaces assigned to a domain |
| `fabio domain show` | no | Show details of a domain |
| `fabio domain unassign-workspaces` | yes | Unassign workspaces from a domain |
| `fabio domain update` | yes | Update domain properties |

### fabio gateway
Manage gateways (on-premises, `VNet`, members, role assignments)

| Command | Mutates | Description |
|---|---|---|
| `fabio gateway add-role-assignment` | yes | Add a role assignment to a gateway |
| `fabio gateway check-member-status` | no | Check the status of a gateway member (on-premises only) |
| `fabio gateway check-status` | no | Check the status of a gateway (`VNet` only) |
| `fabio gateway create` | yes | Create a new gateway (`VirtualNetwork` type) |
| `fabio gateway create-streaming` | yes | Create a new streaming virtual network gateway |
| `fabio gateway delete` | yes | Delete a gateway |
| `fabio gateway delete-member` | yes | Delete a gateway member |
| `fabio gateway delete-role-assignment` | yes | Delete a role assignment |
| `fabio gateway list` | no | List all gateways |
| `fabio gateway list-members` | no | List members of a gateway |
| `fabio gateway list-role-assignments` | no | List role assignments for a gateway |
| `fabio gateway restart` | yes | Restart a gateway (`VNet` only, LRO) |
| `fabio gateway show` | no | Show details of a gateway |
| `fabio gateway show-role-assignment` | no | Show a specific role assignment |
| `fabio gateway shutdown` | yes | Shut down a gateway (`VNet` only, LRO) |
| `fabio gateway update` | yes | Update gateway properties |
| `fabio gateway update-member` | yes | Update a gateway member |
| `fabio gateway update-role-assignment` | yes | Update a role assignment |

### fabio connection
Manage connections (cloud, on-premises, virtual network)

| Command | Mutates | Description |
|---|---|---|
| `fabio connection add-role-assignment` | yes | Add a role assignment to a connection |
| `fabio connection create` | yes | Create a new connection |
| `fabio connection delete` | yes | Delete a connection |
| `fabio connection delete-role-assignment` | yes | Delete a role assignment from a connection |
| `fabio connection list` | no | List all connections you have permission to access |
| `fabio connection list-role-assignments` | no | List role assignments for a connection |
| `fabio connection list-supported-types` | no | List supported connection types (gateway types catalog) |
| `fabio connection show` | no | Show details of a specific connection |
| `fabio connection show-role-assignment` | no | Show a specific role assignment for a connection |
| `fabio connection test-connection` | no | Test a connection (not supported for `StreamingVirtualNetworkGateway` connections) |
| `fabio connection update` | yes | Update a connection's name, credentials, or privacy level |
| `fabio connection update-role-assignment` | yes | Update a role assignment for a connection |

### fabio managed-private-endpoint
Manage workspace managed private endpoints

| Command | Mutates | Description |
|---|---|---|
| `fabio managed-private-endpoint create` | yes | Create a managed private endpoint |
| `fabio managed-private-endpoint delete` | yes | Delete a managed private endpoint |
| `fabio managed-private-endpoint list` | no | List managed private endpoints in a workspace |
| `fabio managed-private-endpoint show` | no | Show details of a managed private endpoint |

### fabio label
List and resolve sensitivity labels (from Microsoft Purview via Graph API)

| Command | Mutates | Description |
|---|---|---|
| `fabio label list` | no | List available sensitivity labels (from Microsoft Purview via Graph API) |

## Must / Prefer / Avoid
### MUST
- Call tenant-scoped commands WITHOUT --workspace (capacity, connection, gateway, domain, deployment-pipeline, admin).
- Have a Fabric admin role before using admin commands (they FORBIDDEN otherwise).

### PREFER
- Batch operations (workspace batch-assign-roles, domain batch-assign) over N single calls to reduce throttling.
- list APIs + client-side filter over repeated individual show calls.
- --dry-run before any destructive tenant change.

### AVOID
- Passing --workspace to tenant-scoped commands (they operate at tenant level).
- Suspending capacity during business hours without warning users about interrupted jobs.
- Bulk tenant-setting changes without confirming the scope with the user.

## Key gotchas
- capacity suspend/resume/create/delete use the ARM scope (management.azure.com), not the Fabric scope.
- label list resolves UUIDs to names via Microsoft Graph (needs M365 E5 + InformationProtection.Read).
- workspace recover-item can partially succeed: a failed child and its descendants remain soft-deleted while independent branches may recover.

## Troubleshooting
| Symptom | Fix |
|---|---|
| admin commands return FORBIDDEN | You need the Fabric administrator role; these are tenant-scoped operations. |
| Operations fail with CAPACITY_INACTIVE | Resume the capacity (fabio capacity resume --id $CAP) before running workloads on it. |
| label list shows UUIDs instead of names | Name resolution needs M365 E5 + InformationProtection.Read via Microsoft Graph. |
| capacity suspend/resume fails | These use the ARM scope (management.azure.com); ensure your identity has rights on the capacity resource. |

## Safety
- capacity suspend interrupts ALL running workloads (notebooks, pipelines, Spark jobs) on that capacity — warn about in-flight jobs.
- Tenant setting changes are broad — confirm scope before applying.
- Deleting a workspace is permanent and removes ALL items inside — warn and suggest --dry-run.
- workspace delete-recoverable-item permanently removes a soft-deleted item; always inspect list-recoverable-items and use --dry-run first.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices admin-apis` | fabio has both workspace-scoped commands (for regular users) and admin commands (for Fabric administrators). Use admin commands only when explicitly needed. |
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices sensitivity-labels` | Sensitivity labels from Microsoft Purview Information Protection are now returned inline by the Fabric Items API. Use them for governance automation, AI agent guardrails, and compliance inventory. |
| `fabio context best-practices tags` | Fabric organizational tags enable multi-dimensional classification of workspaces and items. Tags are returned inline in item/workspace responses and can be used for governance, inventory, and agent-based filtering. |
| `fabio context best-practices tenant-feature-gates` | Many Fabric features are gated by a tenant setting an admin can toggle. When a setting is disabled the API returns an opaque 403 FeatureNotAvailable; fabio turns this into an admin-aware teaching error that names the exact setting and (for admins) the command to enable it. Do NOT blindly retry a feature-disabled error. |

## See also
- fabio context persona fabric-admin
- fabio context best-practices admin-apis
- fabio context best-practices throttling
