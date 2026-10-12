#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, realpathSync, statSync } from 'node:fs';
import { basename, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const defaultCommandsDir = resolve(
  root,
  'toolchain/target/wasm32-wasip1/release/commands',
);
const defaultManifestPath = resolve(
  root,
  'crates/executor-wasm-abi/assets/agentos-wasm-abi.json',
);
const expectedManifestSchemaVersion = 2;
const allowedImportStatuses = new Set(['canonical', 'compatibility']);

const args = process.argv.slice(2);
const printObserved = args.includes('--print-observed');
const printContract = args.includes('--print-contract');
const jsonOutput = args.includes('--json');

function option(name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  if (index + 1 >= args.length) throw new Error(`${name} requires a value`);
  return resolve(process.cwd(), args[index + 1]);
}

const commandsDir = option('--commands', defaultCommandsDir);
const manifestPath = option('--manifest', defaultManifestPath);

const valueTypes = new Map([
  [0x7f, 'i32'], [0x7e, 'i64'], [0x7d, 'f32'], [0x7c, 'f64'],
  [0x7b, 'v128'], [0x70, 'funcref'], [0x6f, 'externref'], [0x69, 'exnref'],
]);

// The import/type sections precede code and data. Reading them directly keeps
// the audit bounded even when a large program disassembles to >1 GiB of WAT.
function readImports(bytes, command) {
  let offset = 8;
  const decoder = new TextDecoder('utf-8', { fatal: true });
  function byte(end) {
    if (offset >= end) throw new Error(`${command}: truncated WASM section`);
    return bytes[offset++];
  }
  function uint(end) {
    let value = 0;
    for (let shift = 0; shift <= 28; shift += 7) {
      const next = byte(end);
      value += (next & 0x7f) * 2 ** shift;
      if ((next & 0x80) === 0) {
        if (value > 0xffffffff) throw new Error(`${command}: invalid WASM integer`);
        return value;
      }
    }
    throw new Error(`${command}: overlong WASM integer`);
  }
  function string(end) {
    const length = uint(end);
    if (length > end - offset) throw new Error(`${command}: truncated WASM name`);
    const value = decoder.decode(bytes.subarray(offset, offset + length));
    offset += length;
    return value;
  }
  function types(end) {
    const count = uint(end);
    if (count > end - offset) throw new Error(`${command}: invalid WASM type vector`);
    return Array.from({ length: count }, () => {
      const code = byte(end);
      const type = valueTypes.get(code);
      if (!type) throw new Error(`${command}: unsupported import value type 0x${code.toString(16)}`);
      return type;
    });
  }
  const signatures = [];
  const imports = [];
  while (offset < bytes.length) {
    const section = byte(bytes.length);
    const length = uint(bytes.length);
    const end = offset + length;
    if (end > bytes.length) throw new Error(`${command}: truncated WASM section`);
    if (section === 1) {
      const count = uint(end);
      for (let index = 0; index < count; index++) {
        if (byte(end) !== 0x60) throw new Error(`${command}: unsupported WASM function type`);
        signatures.push({ params: types(end), results: types(end) });
      }
      if (offset !== end) throw new Error(`${command}: malformed WASM type section`);
    } else if (section === 2) {
      const count = uint(end);
      for (let index = 0; index < count; index++) {
        const module = string(end);
        const name = string(end);
        const kind = byte(end);
        if (kind !== 0) {
          throw new Error(`${command}: non-function import ${module}.${name} is outside the AgentOS ABI`);
        }
        const signature = signatures[uint(end)];
        if (!signature) throw new Error(`${command}: import ${module}.${name} has an invalid type index`);
        imports.push({ module, name, ...signature });
      }
      if (offset !== end) throw new Error(`${command}: malformed WASM import section`);
      break;
    }
    offset = end;
  }
  return imports;
}

function inspectCommand(path, command) {
  const bytes = readFileSync(path);
  if (bytes.length < 8 || bytes.subarray(0, 4).toString('hex') !== '0061736d') {
    throw new Error(`${command}: expected a WebAssembly binary`);
  }
  const imports = readImports(bytes, command);
  imports.sort((a, b) =>
    `${a.module}\0${a.name}`.localeCompare(`${b.module}\0${b.name}`),
  );
  return {
    command,
    target: basename(realpathSync(path)),
    sha256: createHash('sha256').update(bytes).digest('hex'),
    imports,
  };
}

function importKey(entry) {
  return `${entry.module}.${entry.name}`;
}

function signatureText(entry) {
  return `(${entry.params.join(',')}) -> (${entry.results.join(',')})`;
}

function collectCommands() {
  let entries;
  try {
    entries = readdirSync(commandsDir, { withFileTypes: true });
  } catch (error) {
    throw new Error(
      `canonical command directory is unavailable (${relative(root, commandsDir)}); run just tools-rebuild first: ${error.message}`,
    );
  }
  const commandNames = entries
    .filter((entry) => entry.isFile() || entry.isSymbolicLink())
    .map((entry) => entry.name)
    .sort();
  if (commandNames.length === 0) {
    throw new Error(`no commands found in ${relative(root, commandsDir)}`);
  }
  return commandNames.map((command) => {
    const path = resolve(commandsDir, command);
    if (!statSync(path).isFile()) {
      throw new Error(`${command}: command target is not a file`);
    }
    return inspectCommand(path, command);
  });
}

function collectObserved(commands) {
  const observed = new Map();
  for (const command of commands) {
    for (const entry of command.imports) {
      const key = importKey(entry);
      const prior = observed.get(key);
      if (prior && signatureText(prior) !== signatureText(entry)) {
        throw new Error(
          `${key} has conflicting signatures: ${signatureText(prior)} vs ${signatureText(entry)} in ${command.command}`,
        );
      }
      const record = prior ?? { ...entry, commands: [] };
      record.commands.push(command.command);
      observed.set(key, record);
    }
  }
  return [...observed.values()].sort((a, b) =>
    importKey(a).localeCompare(importKey(b)),
  );
}

function loadManifest() {
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (
    manifest.schemaVersion !== expectedManifestSchemaVersion ||
    !Array.isArray(manifest.imports)
  ) {
    throw new Error(
      `ABI manifest must have schemaVersion ${expectedManifestSchemaVersion} and an imports array`,
    );
  }
  const declared = new Map();
  for (const entry of manifest.imports) {
    if (
      typeof entry.module !== 'string' ||
      typeof entry.name !== 'string' ||
      !Array.isArray(entry.params) ||
      !Array.isArray(entry.results) ||
      !allowedImportStatuses.has(entry.status)
    ) {
      throw new Error(
        'every ABI import requires a known status, module, name, params, and results',
      );
    }
    const key = importKey(entry);
    if (declared.has(key)) throw new Error(`duplicate ABI declaration ${key}`);
    declared.set(key, entry);
  }
  const aliases = new Map(Object.entries(manifest.moduleAliases ?? {}));
  for (const [alias, canonical] of aliases) {
    if (alias === canonical) throw new Error(`ABI module alias ${alias} is self-referential`);
  }
  return { manifest, declared, aliases };
}

function verifyObserved(observed, declared, aliases) {
  const failures = [];
  for (const entry of observed) {
    const key = importKey(entry);
    const canonicalModule = aliases.get(entry.module) ?? entry.module;
    const contract = declared.get(`${canonicalModule}.${entry.name}`);
    if (!contract) {
      failures.push(`${key}: undeclared import (${signatureText(entry)})`);
      continue;
    }
    if (signatureText(contract) !== signatureText(entry)) {
      failures.push(
        `${key}: expected ${signatureText(contract)}, observed ${signatureText(entry)}`,
      );
    }
  }
  if (failures.length > 0) {
    throw new Error(`WASM import audit failed:\n- ${failures.join('\n- ')}`);
  }
}

try {
  const commands = collectCommands();
  const observed = collectObserved(commands);
  if (printContract) {
    const contract = observed.map(({ commands: _commands, ...entry }) => entry);
    process.stdout.write(`${JSON.stringify(contract)}\n`);
    process.exit(0);
  }
  if (printObserved) {
    process.stdout.write(`${JSON.stringify(observed, null, 2)}\n`);
    process.exit(0);
  }

  const { manifest, declared, aliases } = loadManifest();
  verifyObserved(observed, declared, aliases);
  const distinctTargets = new Set(commands.map((entry) => entry.target)).size;
  const evidence = {
    schemaVersion: manifest.schemaVersion,
    abiVersion: manifest.abiVersion,
    commandEntries: commands.length,
    distinctModules: distinctTargets,
    observedImports: observed.length,
    commands,
  };
  if (jsonOutput) {
    process.stdout.write(`${JSON.stringify(evidence, null, 2)}\n`);
  } else {
    process.stdout.write(
      `WASM import audit passed: ${commands.length} commands, ${distinctTargets} modules, ${observed.length} imports\n`,
    );
  }
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}
