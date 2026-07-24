import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { escapeHtml, generateReference, inlineCode, renderFlagDescription, renderGroup } from "./generate-reference.mjs";

test("inlineCode escapes backticks", () => {
  assert.equal(inlineCode("a`b"), "`a\\`b`");
});

test("escapeHtml preserves placeholders as visible text", () => {
  assert.equal(escapeHtml("https://<id>/<name>?a=1&b=2"), "https://&lt;id&gt;/&lt;name&gt;?a=1&amp;b=2");
});

test("renderFlagDescription appends enum values and default", () => {
  assert.equal(
    renderFlagDescription({ description: "Target API", values: ["fabric", "powerbi"], default: "fabric" }),
    "Target API One of: `fabric`, `powerbi`. Default: `fabric`.",
  );
});

test("renderFlagDescription escapes pipes and handles missing description", () => {
  assert.equal(renderFlagDescription({ values: ["a|b"] }), "One of: `a\\|b`.");
});

test("renderGroup includes command metadata and flags", () => {
  const markdown = renderGroup("workspace", {
    description: "Manage workspaces",
    auth_scope: "fabric",
    examples: ["fabio workspace list"],
    subcommands: {
      create: {
        description: "Create a workspace",
        aliases: ["new"],
        flags: {
          "--name": { type: "string", required: true, description: "Workspace <name>" },
          "--api": { type: "enum", values: ["fabric", "powerbi"], default: "fabric", description: "Target API" },
        },
        mutates: true,
        returns: "object",
        output_fields: ["id", "displayName"],
        notes: "Idempotent when the workspace already exists.",
        hint: "Use --capacity-id to assign capacity.",
        examples: ['fabio workspace create --name "Analytics"'],
      },
    },
  });

  assert.match(markdown, /fabio workspace create --name <value>/);
  assert.match(markdown, /\| `--name` \| `string` \| Yes \| Workspace &lt;name&gt; \|/);
  assert.match(markdown, /One of: `fabric`, `powerbi`\. Default: `fabric`\./);
  assert.match(markdown, /Mutates state · Returns object/);
  assert.match(markdown, /\*\*Aliases:\*\* `new`/);
  assert.match(markdown, /\*\*Output fields:\*\* `id`, `displayName`/);
  assert.match(markdown, /:::note\nIdempotent when the workspace already exists\.\n:::/);
  assert.match(markdown, /:::tip\nUse --capacity-id to assign capacity\.\n:::/);
  // group-level examples render before the first command heading
  assert.ok(markdown.indexOf("fabio workspace list") < markdown.indexOf("## `create`"));
});

test("generateReference creates one sorted page per group", async () => {
  const directory = await mkdtemp(join(tmpdir(), "fabio-reference-"));
  const schemaPath = join(directory, "commands.json");
  const outputPath = join(directory, "output");
  await writeFile(
    schemaPath,
    JSON.stringify({
      workspace: { description: "Workspaces", subcommands: {} },
      auth: { description: "Authentication", subcommands: {} },
    }),
  );

  const count = await generateReference(schemaPath, outputPath);

  assert.equal(count, 2);
  assert.match(await readFile(join(outputPath, "auth.md"), "utf8"), /title: "auth"/);
  assert.match(await readFile(join(outputPath, "workspace.md"), "utf8"), /title: "workspace"/);
});
