#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const PINNED_NODE_COMMIT = '848430679556aed0bd073f2bc263331ad84fa119';
const POC_NODE_COMMIT = 'fbf82766d623fd9855fdb2fde32aeb6794af84e9';
const crateRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputPath = join(crateRoot, 'bindings/inventory.json');

function parseArgs(argv) {
  const options = { check: false };
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index];
    if (argument === '--check') options.check = true;
    else if (argument === '--node-src') options.nodeSrc = resolve(argv[++index]);
    else if (argument === '--poc-node-src') options.pocNodeSrc = resolve(argv[++index]);
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (!options.nodeSrc || !options.pocNodeSrc) {
    throw new Error('--node-src and --poc-node-src are required');
  }
  return options;
}

function git(source, ...args) {
  return execFileSync('git', ['-C', source, ...args], { encoding: 'utf8' }).trim();
}

function collectJavaScript(directory, files = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) collectJavaScript(path, files);
    else if (entry.isFile() && entry.name.endsWith('.js')) files.push(path);
  }
  return files.sort();
}

function inventory(source, expectedCommit) {
  const actualCommit = git(source, 'rev-parse', 'HEAD');
  if (actualCommit !== expectedCommit) {
    throw new Error(`${source} is ${actualCommit}; expected ${expectedCommit}`);
  }

  const lib = join(source, 'lib');
  const occurrences = [];
  const legacyProcessBindingOccurrences = [];
  const pattern = /\b(internalBinding|process\.binding)\s*\(\s*(['"])([^'"]+)\2\s*\)/g;
  for (const path of collectJavaScript(lib)) {
    const sourceText = readFileSync(path, 'utf8');
    for (const match of sourceText.matchAll(pattern)) {
      const offset = match.index ?? 0;
      const occurrence = {
        binding: match[3],
        kind: match[1],
        file: relative(lib, path).replaceAll('\\', '/'),
        line: sourceText.slice(0, offset).split('\n').length,
      };
      if (match[1] === 'internalBinding') occurrences.push(occurrence);
      else if (!match[3].includes('$')) legacyProcessBindingOccurrences.push(occurrence);
    }
  }
  occurrences.sort((left, right) =>
    left.binding.localeCompare(right.binding) ||
    left.file.localeCompare(right.file) ||
    left.line - right.line,
  );
  const names = [...new Set(occurrences.map(({ binding }) => binding))].sort();
  legacyProcessBindingOccurrences.sort((left, right) =>
    left.binding.localeCompare(right.binding) ||
    left.file.localeCompare(right.file) ||
    left.line - right.line,
  );
  return {
    commit: actualCommit,
    unique_count: names.length,
    occurrence_count: occurrences.length,
    names,
    occurrences,
    legacy_process_binding: {
      names: [...new Set(legacyProcessBindingOccurrences.map(({ binding }) => binding))].sort(),
      occurrences: legacyProcessBindingOccurrences,
    },
  };
}

function digest(value) {
  return createHash('sha256').update(JSON.stringify(value)).digest('hex');
}

const options = parseArgs(process.argv.slice(2));
const pinned = inventory(options.nodeSrc, PINNED_NODE_COMMIT);
const poc = inventory(options.pocNodeSrc, POC_NODE_COMMIT);
const pinnedNames = new Set(pinned.names);
const pocNames = new Set(poc.names);
const output = {
  schema: 1,
  generator: 'crates/node-stdlib/scripts/reconcile-bindings.mjs',
  pinned_node: pinned,
  v26_poc: poc,
  reconciliation: {
    added_in_pinned_v24: pinned.names.filter((name) => !pocNames.has(name)),
    absent_from_pinned_v24: poc.names.filter((name) => !pinnedNames.has(name)),
    common_count: pinned.names.filter((name) => pocNames.has(name)).length,
  },
};
output.content_sha256 = digest(output);

if (pinned.unique_count !== 69 || poc.unique_count !== 69) {
  throw new Error(`binding surface changed: v24=${pinned.unique_count}, v26-poc=${poc.unique_count}`);
}
const serialized = `${JSON.stringify(output, null, 2)}\n`;
if (options.check) {
  if (!existsSync(outputPath) || readFileSync(outputPath, 'utf8') !== serialized) {
    throw new Error('bindings/inventory.json is stale; rerun without --check');
  }
} else {
  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, serialized);
}
process.stdout.write(
  `bindings: v24=${pinned.unique_count}/${pinned.occurrence_count} ` +
    `v26-poc=${poc.unique_count}/${poc.occurrence_count} ` +
    `added=${output.reconciliation.added_in_pinned_v24.length} ` +
    `removed=${output.reconciliation.absent_from_pinned_v24.length}\n`,
);
