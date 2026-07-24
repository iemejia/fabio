# fabio documentation site

The user-facing documentation for [fabio](https://github.com/iemejia/fabio), built with
[Astro](https://astro.build) + [Starlight](https://starlight.astro.build) and organized with the
[Diátaxis](https://diataxis.fr) framework. Published to GitHub Pages at
<https://iemejia.github.io/fabio/>.

> Contributor process, deployment, and best practices are documented authoritatively in the
> **Documentation Website (MANDATORY)** section of the repo root `AGENTS.md`. This file is a quick start.

## Prerequisites

- Node 22.12+ (CI uses Node 24)

## Commands

Run everything from the `docs/` directory:

```bash
npm install          # first time only
npm run dev          # start the dev server (regenerates the command reference first)
npm run build        # production build to dist/
npm run check        # astro type-check + internal link validation (mirrors CI)
npm run check:links  # internal link validation only
npm test             # unit tests for the generator + link checker
```

## Structure

```
src/content/docs/
├── getting-started.md   Tutorial       (learning-oriented)
├── guides/*.md          How-to guides  (task-oriented)
├── explanation/*.md     Explanation    (understanding-oriented)
└── reference/
    ├── index.md            hand-authored reference overview
    ├── global-flags.md     hand-authored global-flags reference
    └── commands/*.md       GENERATED from commands.json — gitignored, never edit
```

## Generated vs. authored

- **Generated (never edit):** `src/content/docs/reference/commands/*.md` are produced by
  `scripts/generate-reference.mjs` from `../src/commands/context/data/agent/commands.json`. The
  directory is gitignored and rebuilt on every `dev`/`build`/`check`. To change the reference,
  change the CLI and regenerate `commands.json` (see the root `AGENTS.md` → Auto-Generated Files).
- **Authored (hand-maintained):** everything else — the tutorial, guides, explanation pages,
  `reference/index.md`, `reference/global-flags.md`, the landing page (`src/pages/index.astro`), and
  styles. Update these by hand when the behavior they describe changes.

## Adding a page

1. Create a Markdown file under the correct Diátaxis directory (`guides/` or `explanation/` auto-generate
   their sidebar; `getting-started` and `reference/*` are wired explicitly in `astro.config.mjs`).
2. Use **relative** links between pages (e.g. `../guides/agents/`). Root-absolute Markdown links are
   **not** rebased by Astro and break under the `/fabio` base path.
3. Run `npm run check` — the link validator fails if any internal link (page, command group, or public
   asset) does not resolve.

## Deployment

`.github/workflows/docs.yml` deploys to GitHub Pages on push to `main` (PRs build and validate only).
One-time repo setup: **Settings → Pages → Source = "GitHub Actions"**.

## Conventions

- Pin exact dependency versions in `package.json` (no `^`/`~`).
- Add/adjust unit tests in `scripts/*.test.mjs` when changing `scripts/*.mjs`.
