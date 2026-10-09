---
title: Getting started
description: Install Fabio, authenticate, and create your first Microsoft Fabric workspace.
---

This tutorial takes you from a new machine to a working Microsoft Fabric workspace. Fabio is non-interactive after authentication and emits structured JSON by default, so the same commands work for people, scripts, and coding agents.

## 1. Install Fabio

### Linux or macOS

```bash
curl -fsSL https://raw.githubusercontent.com/iemejia/fabio/main/install.sh | bash
```

The installer places the binary in `~/.local/bin`. Open a new shell if that directory was added to your `PATH`.

### Windows

```powershell
irm https://raw.githubusercontent.com/iemejia/fabio/main/install.ps1 | iex
```

The installer places Fabio in `%LOCALAPPDATA%\fabio` and updates your user `PATH`.

### Docker

```bash
docker pull ghcr.io/iemejia/fabio:latest
docker run --rm ghcr.io/iemejia/fabio --help
```

Pre-built binaries for Linux, macOS, and Windows on x64 and arm64 are also available from [GitHub Releases](https://github.com/iemejia/fabio/releases).

Verify the installation:

```bash
fabio --version
```

## 2. Sign in

Use Azure CLI for local user authentication, then ask Fabio to validate that session:

```bash
az login --allow-no-subscriptions
fabio auth login
```

Fabio intentionally has no compiled Entra client ID because the project does not yet have a stable publisher-owned Entra registration. If your organization owns a public-client registration, you can use Fabio's native interactive flows explicitly:

```bash
fabio auth login --device-code --client-id <public-client-id>
fabio auth login --browser --client-id <public-client-id>
```

For CI, use a service principal or workload identity rather than an interactive login. See [Authentication](../guides/authentication/).

## 3. Inspect your tenant

```bash
fabio workspace list --limit 10
```

Fabio returns a stable list envelope:

```json
{"data":[{"id":"…","displayName":"Analytics"}],"count":1}
```

Use `-o table` when reading the result yourself, or keep the default JSON when composing commands.

## 4. Create a workspace

```bash
fabio workspace create --name "analytics"
```

Preview a mutation without sending it:

```bash
fabio workspace create --name "analytics" --dry-run
```

If the workspace needs capacity, assign it after creation:

```bash
fabio workspace assign-capacity --id <workspace-id> --capacity <capacity-id>
```

## 5. Let an agent use Fabio

Install Fabio's root skill and its focused workload skills:

```bash
npx skills add https://github.com/iemejia/fabio
```

The skill teaches compatible agents how to discover commands, preserve structured output, and handle destructive operations safely. Continue with [Use Fabio with coding agents](../guides/agents/).

## Next steps

- Create and load data in a [lakehouse](../reference/commands/lakehouse/).
- Learn the [output and piping model](../guides/output-and-piping/).
- Search the complete [CLI reference](../reference/).
