import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const SCAN_DIRS = ["ui", "scripts"];
const JS_EXTENSIONS = new Set([".js", ".mjs"]);

function collectJsModules(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") || entry.isSymbolicLink()) {
      continue;
    }
    const abs = join(dir, entry.name);
    if (entry.isDirectory()) {
      collectJsModules(abs, out);
    } else if (entry.isFile() && JS_EXTENSIONS.has(extname(entry.name))) {
      out.push(abs);
    }
  }
  return out;
}

const targets = [];
for (const dir of SCAN_DIRS) {
  collectJsModules(join(ROOT, dir), targets);
}
targets.sort();

if (targets.length === 0) {
  console.error("Syntax check found no JS/MJS modules under ui/ or scripts/.");
  process.exit(1);
}

let failed = false;
for (const abs of targets) {
  const rel = relative(ROOT, abs).replaceAll("\\", "/");
  const res = spawnSync(process.execPath, ["--check", abs], { encoding: "utf8" });
  if (res.error) {
    failed = true;
    console.error(`Syntax check failed to execute for ${rel}: ${res.error.message}`);
  } else if (res.status !== 0) {
    failed = true;
    console.error(`Syntax check failed for ${rel}:\n${(res.stderr || res.stdout).trim()}`);
  }
}

if (failed) {
  process.exit(1);
}

console.log(`Syntax check passed (${targets.length} JS/MJS modules verified via node --check).`);
