import { readFile, readdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const defaultDocsDirectory = resolve(scriptDirectory, "../src/content/docs");
const defaultSchema = resolve(scriptDirectory, "../../src/commands/context/data/agent/commands.json");
const defaultPublicDirectory = resolve(scriptDirectory, "../public");
const defaultLandingPage = resolve(scriptDirectory, "../src/pages/index.astro");

// Generated command pages live here and are not authored by hand.
const GENERATED_SEGMENT = "reference/commands/";

/** Map an authored content file (relative posix path) to its Starlight route. */
export function routeForContentFile(relativePath) {
  let route = relativePath.replaceAll("\\", "/").replace(/\.md$/, "");
  route = route.replace(/(^|\/)index$/, "$1"); // `foo/index` → `foo/`
  route = `/${route}`.replaceAll(/\/+/g, "/");
  return route.endsWith("/") ? route : `${route}/`;
}

/** Extract link targets from Markdown `[text](target)` syntax. */
export function extractMarkdownLinks(text) {
  const links = [];
  const pattern = /\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
  let match;
  while ((match = pattern.exec(text)) !== null) {
    links.push(match[1]);
  }
  return links;
}

/** Extract `link("…")` targets from the landing page (base-relative). */
export function extractLandingLinks(text) {
  const links = [];
  const pattern = /\blink\("([^"]*)"\)/g;
  let match;
  while ((match = pattern.exec(text)) !== null) {
    links.push(match[1]);
  }
  return links;
}

export function isExternal(target) {
  return /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith("//");
}

/** Resolve an internal link against the page it appears on. Returns a pathname, or null to skip. */
export function resolveInternal(fromRoute, target) {
  const withoutHash = target.split("#")[0].split("?")[0];
  if (withoutHash === "") {
    return null; // same-page hash/query only
  }
  return new URL(withoutHash, `https://links.local${fromRoute}`).pathname;
}

function isAssetPath(pathname) {
  const lastSegment = pathname.split("/").pop() ?? "";
  return lastSegment.includes(".");
}

function withTrailingSlash(pathname) {
  return pathname.endsWith("/") ? pathname : `${pathname}/`;
}

async function collectMarkdownFiles(directory) {
  const entries = await readdir(directory, { recursive: true, withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
    .map((entry) => resolve(entry.parentPath ?? entry.path, entry.name));
}

/**
 * Validate every internal link in the authored docs and the landing page against
 * the set of real routes (authored pages + generated command groups) and public
 * assets. Returns an array of `{ file, target, resolved, reason }` for broken links.
 */
export async function findBrokenLinks({
  docsDirectory = defaultDocsDirectory,
  schemaPath = defaultSchema,
  publicDirectory = defaultPublicDirectory,
  landingPage = defaultLandingPage,
} = {}) {
  const markdownFiles = await collectMarkdownFiles(docsDirectory);
  const authored = markdownFiles.filter(
    (file) => !file.replaceAll("\\", "/").includes(`/${GENERATED_SEGMENT}`),
  );

  // Build the universe of valid routes.
  const routes = new Set(["/"]); // landing page
  for (const file of authored) {
    const relativePath = file.slice(docsDirectory.length + 1);
    routes.add(routeForContentFile(relativePath));
  }
  const schema = JSON.parse(await readFile(schemaPath, "utf8"));
  routes.add("/reference/commands/");
  for (const group of Object.keys(schema)) {
    routes.add(`/reference/commands/${group}/`);
  }

  // Build the set of public assets (`/name`).
  const publicEntries = await readdir(publicDirectory, { withFileTypes: true });
  const assets = new Set(
    publicEntries.filter((entry) => entry.isFile()).map((entry) => `/${entry.name}`),
  );

  const broken = [];
  const check = (file, fromRoute, target) => {
    if (isExternal(target)) {
      return;
    }
    const resolved = resolveInternal(fromRoute, target);
    if (resolved === null) {
      return;
    }
    if (isAssetPath(resolved)) {
      if (!assets.has(resolved)) {
        broken.push({ file, target, resolved, reason: "missing asset" });
      }
      return;
    }
    if (!routes.has(withTrailingSlash(resolved))) {
      broken.push({ file, target, resolved, reason: "no matching page" });
    }
  };

  for (const file of authored) {
    const relativePath = file.slice(docsDirectory.length + 1);
    const fromRoute = routeForContentFile(relativePath);
    const text = await readFile(file, "utf8");
    for (const target of extractMarkdownLinks(text)) {
      check(relativePath, fromRoute, target);
    }
  }

  // Landing page: `link("x")` targets are base-relative (resolve against `/`).
  const landingText = await readFile(landingPage, "utf8");
  for (const target of extractLandingLinks(landingText)) {
    check("src/pages/index.astro", "/", target);
  }

  return broken;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const broken = await findBrokenLinks();
  if (broken.length > 0) {
    console.error(`Found ${broken.length} broken internal link(s):`);
    for (const { file, target, resolved, reason } of broken) {
      console.error(`  ${file}: ${target} → ${resolved} (${reason})`);
    }
    process.exit(1);
  }
  console.log("All internal links resolve to real pages or assets.");
}
