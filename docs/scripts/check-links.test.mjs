import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  extractLandingLinks,
  extractMarkdownLinks,
  findBrokenLinks,
  isExternal,
  resolveInternal,
  routeForContentFile,
} from "./check-links.mjs";

test("routeForContentFile maps files to Starlight routes", () => {
  assert.equal(routeForContentFile("getting-started.md"), "/getting-started/");
  assert.equal(routeForContentFile("guides/agents.md"), "/guides/agents/");
  assert.equal(routeForContentFile("reference/index.md"), "/reference/");
});

test("extractMarkdownLinks pulls targets and ignores titles", () => {
  assert.deepEqual(extractMarkdownLinks('See [a](../x/) and [b](/y/ "t").'), ["../x/", "/y/"]);
});

test("extractLandingLinks pulls link() targets", () => {
  assert.deepEqual(extractLandingLinks('href={link("getting-started/")} x={link("")}'), [
    "getting-started/",
    "",
  ]);
});

test("isExternal recognizes schemes and protocol-relative URLs", () => {
  assert.equal(isExternal("https://example.com"), true);
  assert.equal(isExternal("mailto:a@b.c"), true);
  assert.equal(isExternal("//cdn.example.com/x"), true);
  assert.equal(isExternal("../guides/agents/"), false);
});

test("resolveInternal resolves relative links and skips pure hashes", () => {
  assert.equal(resolveInternal("/getting-started/", "../guides/agents/"), "/guides/agents/");
  assert.equal(resolveInternal("/reference/", "./global-flags/"), "/reference/global-flags/");
  assert.equal(resolveInternal("/getting-started/", "../reference/commands/lakehouse/"), "/reference/commands/lakehouse/");
  assert.equal(resolveInternal("/x/", "#section"), null);
});

test("findBrokenLinks flags missing pages, groups, and assets but passes valid ones", async () => {
  const root = await mkdtemp(join(tmpdir(), "fabio-links-"));
  const docs = join(root, "docs");
  const guides = join(docs, "guides");
  const pages = join(root, "pages");
  const publicDir = join(root, "public");
  await mkdir(guides, { recursive: true });
  await mkdir(pages, { recursive: true });
  await mkdir(publicDir, { recursive: true });

  await writeFile(join(guides, "agents.md"), "# Agents\n");
  await writeFile(
    join(docs, "getting-started.md"),
    [
      "Valid page [a](../guides/agents/).",
      "Valid group [b](/reference/commands/lakehouse/).",
      "Valid asset [c](/logo.png).",
      "External [d](https://example.com/).",
      "Broken page [e](../guides/missing/).",
      "Broken group [f](/reference/commands/nope/).",
      "Broken asset [g](/missing.svg).",
      "",
    ].join("\n"),
  );
  await writeFile(join(pages, "index.astro"), 'href={link("getting-started/")} img={link("logo.png")}');
  await writeFile(join(publicDir, "logo.png"), "x");
  const schemaPath = join(root, "commands.json");
  await writeFile(schemaPath, JSON.stringify({ lakehouse: { subcommands: {} } }));

  const broken = await findBrokenLinks({
    docsDirectory: docs,
    schemaPath,
    publicDirectory: publicDir,
    landingPage: join(pages, "index.astro"),
  });

  const targets = broken.map((entry) => entry.target).sort();
  assert.deepEqual(targets, ["../guides/missing/", "/missing.svg", "/reference/commands/nope/"]);
});

test("findBrokenLinks treats the generated blog index as a valid route when posts exist", async () => {
  const root = await mkdtemp(join(tmpdir(), "fabio-links-blog-"));
  const docs = join(root, "docs");
  const blog = join(docs, "blog");
  const pages = join(root, "pages");
  const publicDir = join(root, "public");
  await mkdir(blog, { recursive: true });
  await mkdir(pages, { recursive: true });
  await mkdir(publicDir, { recursive: true });

  await writeFile(join(blog, "hello.md"), "# Hello\n");
  await writeFile(
    join(docs, "getting-started.md"),
    "Blog index [a](/blog/) and tags [b](/blog/tags/).\n",
  );
  await writeFile(join(pages, "index.astro"), 'href={link("blog/")}');
  const schemaPath = join(root, "commands.json");
  await writeFile(schemaPath, JSON.stringify({}));

  const broken = await findBrokenLinks({
    docsDirectory: docs,
    schemaPath,
    publicDirectory: publicDir,
    landingPage: join(pages, "index.astro"),
  });

  assert.deepEqual(broken, []);
});
