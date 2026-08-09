// Verifies the built frontend against the security properties the desktop
// client claims. Run after `bun run build`:
//
//   bun run build && bun run verify:bundle
//
// These are checks on the *artifact*, not on intent: a dependency that starts
// pulling a CDN script, or a transform that emits `eval`, fails here even
// though the source looks clean.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const dist = join(root, "dist");

/** @param {string} dir @returns {string[]} */
function walk(dir) {
  /** @type {string[]} */
  const found = [];
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) found.push(...walk(path));
    else found.push(path);
  }
  return found;
}

/** @type {string[]} */
const failures = [];
/** @param {string} message */
const fail = (message) => failures.push(message);

let files;
try {
  files = walk(dist);
} catch {
  console.error("verify-bundle: dist/ is missing — run `bun run build` first.");
  process.exit(1);
}

// 1. No remote resource references anywhere in the shipped bundle.
const REMOTE = /(?:src|href)\s*=\s*["']https?:\/\//gi;
const REMOTE_IMPORT = /\bimport\s*\(\s*["']https?:\/\//gi;

// 2. No dynamic code evaluation.
const EVAL = /\beval\s*\(/g;
const NEW_FUNCTION = /\bnew\s+Function\s*\(/g;

for (const file of files) {
  if (!/\.(html|js|mjs|css)$/i.test(file)) continue;
  const text = readFileSync(file, "utf8");
  const name = relative(root, file);
  for (const [label, pattern] of /** @type {[string, RegExp][]} */ ([
    ["remote src/href", REMOTE],
    ["remote dynamic import", REMOTE_IMPORT],
    ["eval(", EVAL],
    ["new Function(", NEW_FUNCTION],
  ])) {
    const hits = text.match(pattern);
    if (hits) fail(`${name}: ${label} — ${hits.length} occurrence(s): ${hits[0]}`);
  }
}

// 3. The declared CSP must stay strict, and must not readmit inline or remote
//    script. `devCsp` is deliberately not checked: it is never shipped.
const config = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const csp = config?.app?.security?.csp;
if (typeof csp !== "string" || csp.length === 0) {
  fail("tauri.conf.json declares no app.security.csp");
} else {
  for (const directive of [
    "default-src 'self'",
    "script-src 'self'",
    "object-src 'none'",
    "base-uri 'none'",
    "frame-ancestors 'none'",
  ]) {
    if (!csp.includes(directive)) fail(`csp is missing \`${directive}\``);
  }
  for (const forbidden of ["unsafe-inline", "unsafe-eval", "*"]) {
    if (csp.includes(forbidden)) fail(`csp contains \`${forbidden}\``);
  }
}

// 4. No execution-capable Tauri plugin may appear in the crate manifest.
const manifest = readFileSync(join(root, "src-tauri", "Cargo.toml"), "utf8")
  .split("\n")
  .map((line) => line.split("#")[0])
  .join("\n");
for (const plugin of [
  "tauri-plugin-shell",
  "tauri-plugin-fs",
  "tauri-plugin-http",
  "tauri-plugin-process",
  "tauri-plugin-opener",
  "tauri-plugin-dialog",
]) {
  if (manifest.includes(plugin)) fail(`src-tauri/Cargo.toml depends on ${plugin}`);
}

// 5. Only the enumerated commands may be invoked, and only by literal name.
const commandNames = new Set(["app_status", "app_claims", "app_connection", "app_bundle_identity"]);
const clientSource = readFileSync(join(root, "src", "ipc", "client.ts"), "utf8");
for (const match of clientSource.matchAll(/invoke<[^>]*>\(\s*"([^"]+)"/g)) {
  if (!commandNames.has(match[1])) fail(`src/ipc/client.ts invokes unknown command ${match[1]}`);
}
const frontendFiles = walk(join(root, "src"));
for (const file of frontendFiles) {
  if (!/\.tsx?$/.test(file)) continue;
  if (file.endsWith(join("ipc", "client.ts"))) continue;
  const text = readFileSync(file, "utf8");
  if (/\binvoke\s*[<(]/.test(text)) {
    fail(`${relative(root, file)} calls invoke directly; go through src/ipc/client.ts`);
  }
}

if (failures.length > 0) {
  console.error("verify-bundle: FAILED");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`verify-bundle: OK (${files.length} built file(s) checked)`);
