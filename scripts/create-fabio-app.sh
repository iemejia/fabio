#!/usr/bin/env bash
# Create a customer-owned Entra public client for Fabio-managed interactive login.
# Fabio intentionally ships no default client ID. This app supports device code,
# browser PKCE, and Windows WAM across Fabio's seven token audiences.

set -euo pipefail

APP_NAME="Fabio CLI"
ADMIN_CONSENT=0

GRAPH_APP_ID="00000003-0000-0000-c000-000000000000"
POWER_BI_APP_ID="00000009-0000-0000-c000-000000000000"
STORAGE_APP_ID="e406a681-f3d4-42a8-90b6-c2b029497af1"
SQL_APP_ID="022907d3-0f1b-48f7-badc-1ba6abab6d66"
ARM_APP_ID="797f4846-ba00-4fd7-ba43-dac1f8f63013"
KUSTO_APP_ID="2746ea77-4702-4b45-80ca-3c97e680e8b7"
COSMOS_APP_ID="00000007-0000-0000-c000-000000000000"
FABRIC_RESOURCE_URI="https://api.fabric.microsoft.com"

FABRIC_SCOPE_VALUES=(
  "Workspace.ReadWrite.All"
  "Item.ReadWrite.All"
  "Item.Execute.All"
  "Item.Reshare.All"
  "Capacity.ReadWrite.All"
  "Connection.ReadWrite.All"
  "Gateway.ReadWrite.All"
  "OneLake.ReadWrite.All"
  "Tenant.ReadWrite.All"
  "Dataset.ReadWrite.All"
  "Report.ReadWrite.All"
  "PaginatedReport.ReadWrite.All"
  "Dashboard.ReadWrite.All"
  "Dataflow.ReadWrite.All"
)

usage() {
  printf '%s\n' \
    "Usage: $0 [--name <display-name>] [--admin-consent]" \
    "" \
    "Creates a customer-owned multitenant public client for explicit Fabio login." \
    "The script never modifies Fabio source code."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name) APP_NAME="${2:?--name requires a value}"; shift 2 ;;
    --admin-consent) ADMIN_CONSENT=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

command -v az >/dev/null 2>&1 || { printf 'ERROR: az CLI is not installed.\n' >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { printf 'ERROR: jq is not installed.\n' >&2; exit 1; }

resource_access_json() {
  local app_id="$1"
  shift

  if ! az ad sp show --id "$app_id" -o none 2>/dev/null; then
    az ad sp create --id "$app_id" -o none >/dev/null
  fi

  local scopes_json
  scopes_json=$(az ad sp show --id "$app_id" \
    --query "oauth2PermissionScopes[?isEnabled].{value:value,id:id}" -o json)

  local output="[]" value id
  for value in "$@"; do
    id=$(jq -r --arg value "$value" 'first(.[] | select(.value==$value) | .id) // empty' <<<"$scopes_json")
    if [[ -z "$id" ]]; then
      printf "ERROR: required scope '%s' is not published by resource %s.\n" "$value" "$app_id" >&2
      return 1
    fi
    output=$(jq --arg id "$id" '. + [{id: $id, type: "Scope"}]' <<<"$output")
  done
  printf '%s\n' "$output"
}

TENANT_ID=$(az account show --query tenantId -o tsv)
printf '=== Create Fabio customer public client ===\n'
printf '[1/6] Tenant: %s\n' "$TENANT_ID"

printf '[2/6] Creating app registration...\n'
APP_ID=$(az ad app create \
  --display-name "$APP_NAME" \
  --sign-in-audience AzureADMultipleOrgs \
  --is-fallback-public-client true \
  --public-client-redirect-uris "http://localhost" \
  --query appId -o tsv)
printf '       App ID: %s\n' "$APP_ID"

printf '[3/6] Configuring public-client redirects...\n'
az ad app update --id "$APP_ID" --public-client-redirect-uris \
  "http://localhost" \
  "https://login.microsoftonline.com/common/oauth2/nativeclient" \
  "ms-appx-web://microsoft.aad.brokerplugin/${APP_ID}" \
  -o none

printf '[4/6] Creating home-tenant service principal...\n'
az ad sp create --id "$APP_ID" -o none 2>/dev/null || true

printf '[5/6] Adding delegated permissions for seven audiences...\n'
FABRIC_RESOURCE_APP_ID=$(az ad sp list --all \
  --filter "servicePrincipalNames/any(n:n eq '${FABRIC_RESOURCE_URI}')" \
  --query "[0].appId" -o tsv 2>/dev/null || true)
if [[ -z "$FABRIC_RESOURCE_APP_ID" || "$FABRIC_RESOURCE_APP_ID" == "None" ]]; then
  FABRIC_RESOURCE_APP_ID="$POWER_BI_APP_ID"
fi

FABRIC_ACCESS=$(resource_access_json "$FABRIC_RESOURCE_APP_ID" "${FABRIC_SCOPE_VALUES[@]}")
STORAGE_ACCESS=$(resource_access_json "$STORAGE_APP_ID" "user_impersonation")
SQL_ACCESS=$(resource_access_json "$SQL_APP_ID" "user_impersonation")
ARM_ACCESS=$(resource_access_json "$ARM_APP_ID" "user_impersonation")
KUSTO_ACCESS=$(resource_access_json "$KUSTO_APP_ID" "user_impersonation")
GRAPH_ACCESS=$(resource_access_json "$GRAPH_APP_ID" "User.Read" "InformationProtectionPolicy.Read")
COSMOS_ACCESS=$(resource_access_json "$COSMOS_APP_ID" "user_impersonation")

RRA_FILE=$(mktemp)
trap 'rm -f "$RRA_FILE"' EXIT
jq -n \
  --arg fabric "$FABRIC_RESOURCE_APP_ID" \
  --arg storage "$STORAGE_APP_ID" \
  --arg sql "$SQL_APP_ID" \
  --arg arm "$ARM_APP_ID" \
  --arg kusto "$KUSTO_APP_ID" \
  --arg graph "$GRAPH_APP_ID" \
  --arg cosmos "$COSMOS_APP_ID" \
  --argjson fabricAccess "$FABRIC_ACCESS" \
  --argjson storageAccess "$STORAGE_ACCESS" \
  --argjson sqlAccess "$SQL_ACCESS" \
  --argjson armAccess "$ARM_ACCESS" \
  --argjson kustoAccess "$KUSTO_ACCESS" \
  --argjson graphAccess "$GRAPH_ACCESS" \
  --argjson cosmosAccess "$COSMOS_ACCESS" '
  [
    {resourceAppId: $fabric, resourceAccess: $fabricAccess},
    {resourceAppId: $storage, resourceAccess: $storageAccess},
    {resourceAppId: $sql, resourceAccess: $sqlAccess},
    {resourceAppId: $arm, resourceAccess: $armAccess},
    {resourceAppId: $kusto, resourceAccess: $kustoAccess},
    {resourceAppId: $graph, resourceAccess: $graphAccess},
    {resourceAppId: $cosmos, resourceAccess: $cosmosAccess}
  ] | map(select((.resourceAccess | length) > 0))' > "$RRA_FILE"
az ad app update --id "$APP_ID" --required-resource-accesses @"$RRA_FILE" -o none
if [[ $(jq 'length' "$RRA_FILE") -ne 7 ]]; then
  printf 'ERROR: generated permission manifest does not contain all seven audiences.\n' >&2
  exit 1
fi

if [[ "$ADMIN_CONSENT" -eq 1 ]]; then
  printf '[6/6] Granting admin consent...\n'
  for attempt in 1 2 3; do
    if az ad app permission admin-consent --id "$APP_ID" -o none 2>/dev/null; then
      break
    fi
    [[ "$attempt" -eq 3 ]] && { printf 'Admin consent failed after replication retries.\n' >&2; exit 1; }
    sleep 10
  done
else
  printf '[6/6] Skipping admin consent; pass --admin-consent if required.\n'
fi

printf '\nApp (client) ID: %s\nTenant ID: %s\n\n' "$APP_ID" "$TENANT_ID"
printf 'Use it with either:\n'
printf '  FABIO_CLIENT_ID=%s fabio auth login --device-code\n' "$APP_ID"
printf '  fabio auth login --device-code --client-id %s\n\n' "$APP_ID"
printf 'Delete it later with:\n  az ad app delete --id %s\n' "$APP_ID"
