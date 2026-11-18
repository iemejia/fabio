import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

const require = createRequire(import.meta.url);
const picomatch = require("picomatch");

test("every installed picomatch resolution uses the exact reviewed security fork", async () => {
  const lock = JSON.parse(await readFile(new URL("../package-lock.json", import.meta.url), "utf8"));
  assert.equal(lock.packages["node_modules/picomatch"].name, "@ryotsu/picomatch");
  for (const [path, metadata] of Object.entries(lock.packages)) {
    if (!path.endsWith("/picomatch")) continue;
    assert.equal(metadata.name, "@ryotsu/picomatch", path);
    assert.equal(metadata.version, "4.0.7-a", path);
    assert.equal(metadata.integrity, "sha512-JroBFEmaejC7szGSfqOhF1QZHLKvX2wTA1+fWyfkpy3BjzdQKNPLKa2FtgeKVUDy5/UPUNkXEeCHJ5ZRBMLTVA==", path);
  }
  for (const consumer of ["astro", "tinyglobby", "fdir", "anymatch", "vite", "@astrojs/internal-helpers/create-filter"]) {
    const resolveForConsumer = createRequire(require.resolve(consumer));
    assert.equal(resolveForConsumer.resolve("picomatch"), require.resolve("picomatch"), consumer);
  }
});

test("CVE-2026-112366 single-branch attacks finish in a bounded child process", () => {
  // Never run a potentially regressed ReDoS matcher in the test runner itself.
  const script = `
    const assert = require("node:assert/strict");
    const pm = require(${JSON.stringify(require.resolve("picomatch"))});
    const cases = [
      ["+(*a)", "a".repeat(80) + "!"], ["*(*a)", "a".repeat(80) + "!"],
      ["+(*)", "a".repeat(80) + "/"], ["*(*)", "a".repeat(80) + "/"],
      ["+(a*)", "a".repeat(80) + "/"], ["*(a*)", "a".repeat(80) + "/"],
      ["+(*)b", "a".repeat(80) + "!"], ["*(*)b", "a".repeat(80) + "!"],
      ["+(a*b)", "ab".repeat(40) + "!"], ["+(@(*a))", "a".repeat(80) + "!"],
      ["+(?(*a))", "a".repeat(80) + "!"], ["+(**a)", "a".repeat(80) + "!"]
    ];
    for (const options of [{}, { windows: true }, { capture: true }]) {
      for (const [pattern, input] of cases) assert.equal(pm.isMatch(input, pattern, options), false, pattern);
    }
  `;
  const child = spawnSync(process.execPath, ["-e", script], { encoding: "utf8", timeout: 5000, windowsHide: true });
  assert.ifError(child.error);
  assert.equal(child.status, 0, child.stderr);
});

test("docs globs, extension alternation and glob discovery retain expected behavior", async () => {
  assert.equal(picomatch.isMatch("src/content/docs/guides/test.md", "src/content/docs/**/*.md"), true);
  assert.equal(picomatch.isMatch("src/content/docs/guides/test.mdx", "**/*.{md,mdx}"), true);
  assert.equal(picomatch.isMatch("src/styles/site.css", "**/*.{md,mdx}"), false);
  assert.equal(picomatch.isMatch("src/pages/index.astro", "**/*.@(astro|md)"), true);
  assert.equal(picomatch.isMatch("aa", "+(*a)"), true);
  assert.equal(picomatch.isMatch("axbayb", "+(a*b)"), true);
  const { glob } = await import("tinyglobby");
  const paths = await glob("scripts/*.test.mjs", { cwd: fileURLToPath(new URL("..", import.meta.url)) });
  assert.equal(paths.some((path) => path.endsWith("glob-security.test.mjs")), true);
});
