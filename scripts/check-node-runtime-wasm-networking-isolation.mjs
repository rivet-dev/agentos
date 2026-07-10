#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = resolve(
  repoRoot,
  "docs-internal/node-runtime-wasm-networking-isolation.json",
);
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

function runJj(args, encoding = "utf8") {
  const result = spawnSync("jj", args, {
    cwd: repoRoot,
    encoding,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) {
    const stderr = Buffer.isBuffer(result.stderr)
      ? result.stderr.toString("utf8")
      : result.stderr;
    throw new Error(`jj ${args.join(" ")} failed: ${stderr}`);
  }
  return result.stdout;
}

function expandBraces(pattern) {
  const open = pattern.indexOf("{");
  if (open === -1) return [pattern];
  const close = pattern.indexOf("}", open + 1);
  if (close === -1) throw new Error(`unclosed brace in protected glob: ${pattern}`);
  const choices = pattern.slice(open + 1, close).split(",");
  return choices.flatMap((choice) =>
    expandBraces(`${pattern.slice(0, open)}${choice}${pattern.slice(close + 1)}`),
  );
}

function globRegex(pattern) {
  let expression = "^";
  for (let index = 0; index < pattern.length; index += 1) {
    const character = pattern[index];
    if (character === "*" && pattern[index + 1] === "*") {
      expression += ".*";
      index += 1;
    } else if (character === "*") {
      expression += "[^/]*";
    } else if ("\\^$+?.()|[]".includes(character)) {
      expression += `\\${character}`;
    } else {
      expression += character;
    }
  }
  return new RegExp(`${expression}$`);
}

const protectedMatchers = manifest.protectedGlobs.flatMap((pattern) =>
  expandBraces(pattern).map(globRegex),
);

const failures = [];
for (const entry of manifest.baselineFiles) {
  const contents = runJj(
    ["file", "show", "-r", manifest.baselineRevision, entry.path],
    null,
  );
  const actual = createHash("sha256").update(contents).digest("hex");
  if (actual !== entry.sha256) {
    failures.push(
      `baseline hash mismatch for ${entry.path}: expected ${entry.sha256}, got ${actual}`,
    );
  }
}

const changedPaths = runJj([
  "diff",
  "-r",
  `${manifest.baselineRevision}..@`,
  "--name-only",
])
  .trim()
  .split("\n")
  .filter(Boolean);

for (const path of changedPaths) {
  if (protectedMatchers.some((matcher) => matcher.test(path))) {
    failures.push(`Node stack changes protected networking path: ${path}`);
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(failure);
  process.exit(1);
}

console.log(
  `networking isolation OK: ${manifest.baselineFiles.length} baseline hashes and ${manifest.protectedGlobs.length} protected globs`,
);

