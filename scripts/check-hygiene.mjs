import { existsSync, lstatSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { basename, dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));

const SKIP_DIRS = new Set([
  ".git",
  ".planar",
  ".planar-agent",
  "target",
  "node_modules",
  "dist",
  "coverage",
  "__pycache__",
  "gen",
]);

const SKIP_PREFIXES = [".agents/skills/rust-", ".claude/skills/rust-"];
const SKIP_FILES = new Set(["Cargo.lock", "package-lock.json", "scripts/check-hygiene.mjs"]);
const TEXT_EXTENSIONS = new Set([
  ".rs",
  ".toml",
  ".js",
  ".mjs",
  ".cjs",
  ".ts",
  ".css",
  ".html",
  ".json",
  ".yml",
  ".yaml",
  ".md",
  ".py",
  ".sh",
  ".txt",
  ".gitignore",
]);

const PRIVACY_RULES = [
  {
    name: "personal macOS home path",
    pattern: /\/Users\/(?!demo\b|you\b|your-user\b|username\b|<)[A-Za-z0-9._-]+(?:\/|$)/,
  },
  {
    name: "personal Linux home path",
    pattern: /\/home\/(?!demo\b|runner\b|ubuntu\b|you\b|your-user\b|username\b|<)[A-Za-z0-9._-]+(?:\/|$)/,
  },
  {
    name: "personal Windows home path",
    pattern: /C:\\Users\\(?!demo\b|runneradmin\b|you\b|<)[A-Za-z0-9._-]+\\/,
  },
  {
    name: "Obsidian iCloud vault path",
    pattern: /iCloud~md~obsidian/,
  },
  {
    name: "OpenAI or Anthropic API key",
    pattern: /\bsk-(?:ant-|proj-|live-|test-)?[A-Za-z0-9_-]{24,}\b/,
  },
  {
    name: "Google API key",
    pattern: /\bAIza[0-9A-Za-z_-]{35}\b/,
  },
  {
    name: "GitHub token",
    pattern: /\bgh[pousr]_[A-Za-z0-9]{30,}\b/,
  },
  {
    name: "private key block",
    pattern: /-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/,
  },
];

const failures = [];

function fail(category, message) {
  failures.push(`[${category}] ${message}`);
}

function walkFiles(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const abs = join(dir, entry.name);
    const rel = relative(ROOT, abs).replaceAll("\\", "/");
    if (entry.isSymbolicLink()) {
      continue;
    }
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) {
        continue;
      }
      if (SKIP_PREFIXES.some((prefix) => rel.startsWith(prefix))) {
        continue;
      }
      walkFiles(abs, out);
    } else if (entry.isFile()) {
      if (SKIP_FILES.has(rel)) {
        continue;
      }
      if (SKIP_PREFIXES.some((prefix) => rel.startsWith(prefix))) {
        continue;
      }
      out.push({ abs, rel });
    }
  }
  return out;
}

function isScannableText(rel) {
  const ext = extname(rel);
  if (TEXT_EXTENSIONS.has(ext)) {
    return true;
  }
  if (rel.startsWith(".githooks/")) {
    return true;
  }
  const base = basename(rel);
  return base === "LICENSE" || base === ".gitignore";
}

function checkPrivacy(files) {
  for (const { abs, rel } of files) {
    if (!isScannableText(rel)) {
      continue;
    }
    const content = readFileSync(abs, "utf8");
    const lines = content.split("\n");
    for (let i = 0; i < lines.length; i += 1) {
      const line = lines[i];
      for (const rule of PRIVACY_RULES) {
        if (rule.pattern.test(line)) {
          fail("privacy", `${rel}:${i + 1} matches ${rule.name}`);
        }
      }
    }
  }
}

function stripCodeFromMarkdown(markdown) {
  // Remove fenced code blocks first, then inline backtick code spans.
  const withoutFences = markdown.replace(/```[\s\S]*?```/g, "");
  return withoutFences.replace(/`[^`\n]+`/g, "");
}

function checkMarkdownLinks() {
  const roots = [
    "README.md",
    "README.zh-CN.md",
    "AGENTS.md",
    "CONTRIBUTING.md",
    "REVIEW.md",
  ];
  const mdFiles = [];
  for (const file of roots) {
    const abs = join(ROOT, file);
    if (existsSync(abs)) {
      mdFiles.push({ abs, rel: file });
    }
  }
  for (const dir of ["contexts", "skills", ".agents/skills/planar-dev", ".github"]) {
    const absDir = join(ROOT, dir);
    if (!existsSync(absDir)) {
      continue;
    }
    for (const file of walkFiles(absDir)) {
      if (file.rel.endsWith(".md")) {
        mdFiles.push(file);
      }
    }
  }

  const linkRegex = /!?\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;

  for (const { abs, rel } of mdFiles) {
    const raw = readFileSync(abs, "utf8");
    const stripped = stripCodeFromMarkdown(raw);
    for (const match of stripped.matchAll(linkRegex)) {
      const rawTarget = match[1];
      if (
        rawTarget.startsWith("http://") ||
        rawTarget.startsWith("https://") ||
        rawTarget.startsWith("mailto:") ||
        rawTarget.startsWith("file://") ||
        rawTarget.startsWith("#")
      ) {
        continue;
      }
      const cleanTarget = rawTarget.split("#")[0].split("?")[0];
      if (!cleanTarget) {
        continue;
      }
      const resolved = resolve(dirname(abs), cleanTarget);
      if (!existsSync(resolved)) {
        fail("links", `${rel} has broken relative link -> ${rawTarget}`);
      }
    }
  }
}

function checkContextMapRegistration() {
  const mapPath = join(ROOT, "contexts", "CONTEXT_MAP.md");
  if (!existsSync(mapPath)) {
    fail("contexts", "Missing contexts/CONTEXT_MAP.md");
    return;
  }
  const mapContent = readFileSync(mapPath, "utf8");
  const contextFiles = walkFiles(join(ROOT, "contexts")).filter(
    ({ rel }) => rel.endsWith(".md") && rel !== "contexts/CONTEXT_MAP.md",
  );
  for (const { rel } of contextFiles) {
    const base = basename(rel);
    const parentSegment = `${rel.split("/")[1]}/`;
    if (!mapContent.includes(base) || !mapContent.includes(parentSegment)) {
      fail(
        "contexts",
        `${rel} is not registered in contexts/CONTEXT_MAP.md (add it to the directory tree)`,
      );
    }
  }
}

function checkVersionLockstep() {
  const cargoToml = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const workspaceSection = cargoToml.split("[workspace.package]")[1]?.split("\n[")[0] ?? "";
  const cargoMatch = workspaceSection.match(/^\s*version\s*=\s*"([^"]+)"/m);
  const cargoVersion = cargoMatch?.[1];

  const pkgJson = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  const pkgVersion = pkgJson.version;

  const tauriConf = JSON.parse(readFileSync(join(ROOT, "src-tauri", "tauri.conf.json"), "utf8"));
  const tauriVersion = tauriConf.version;

  if (!cargoVersion || !pkgVersion || !tauriVersion) {
    fail("version", "Could not read version from Cargo.toml, package.json, or tauri.conf.json");
    return;
  }
  if (cargoVersion !== pkgVersion || cargoVersion !== tauriVersion) {
    fail(
      "version",
      `Version mismatch: Cargo.toml=${cargoVersion}, package.json=${pkgVersion}, tauri.conf.json=${tauriVersion}`,
    );
  }

  for (const win of tauriConf.app?.windows ?? []) {
    if (win.dragDropEnabled !== false) {
      fail(
        "tauri",
        `src-tauri/tauri.conf.json window '${win.label ?? "main"}' must set dragDropEnabled: false`,
      );
    }
  }
}

function checkSymlink(linkRel, targetRel) {
  const linkAbs = join(ROOT, linkRel);
  const targetAbs = join(ROOT, targetRel);
  if (!existsSync(linkAbs)) {
    fail("symlink", `${linkRel} is missing (expected symlink to ${targetRel})`);
    return;
  }
  const stat = lstatSync(linkAbs);
  if (!stat.isSymbolicLink()) {
    fail("symlink", `${linkRel} must be a symlink to ${targetRel}, not a regular file/dir`);
    return;
  }
  if (realpathSync(linkAbs) !== realpathSync(targetAbs)) {
    fail("symlink", `${linkRel} does not resolve to ${targetRel}`);
  }
}

const allFiles = walkFiles(ROOT);
checkPrivacy(allFiles);
checkMarkdownLinks();
checkContextMapRegistration();
checkVersionLockstep();
checkSymlink("CLAUDE.md", "AGENTS.md");
checkSymlink(".claude/skills", ".agents/skills");

if (failures.length > 0) {
  console.error(`Planar hygiene check failed (${failures.length} issue(s)):`);
  for (const item of failures) {
    console.error(`  - ${item}`);
  }
  process.exit(1);
}

console.log(
  `Planar hygiene check passed (${allFiles.length} files scanned, links/contexts/versions/symlinks OK).`,
);
