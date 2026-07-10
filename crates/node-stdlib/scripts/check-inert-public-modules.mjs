#!/usr/bin/env node

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const crateRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const libRoot = join(crateRoot, 'vendor/lib');
const sources = Object.create(null);

function collect(dir, base = libRoot, prefix = '') {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      collect(path, base, prefix);
    } else if (path.endsWith('.js')) {
      sources[`${prefix}${relative(base, path).replaceAll('\\', '/').slice(0, -3)}`] =
        readFileSync(path, 'utf8');
    }
  }
}

collect(libRoot);
const depsRoot = join(crateRoot, 'vendor/deps');
for (const dependency of ['acorn', 'undici']) {
  collect(join(depsRoot, dependency), depsRoot, 'internal/deps/');
}
const context = vm.createContext({
  AbortController,
  TextDecoder,
  TextEncoder,
  URL,
  URLSearchParams,
  WebAssembly,
  console,
  structuredClone,
});
context.globalThis = context;
context.__agentOSNodeSources = sources;
context.__agentOSNodeLoadAll = true;
context.process = {
  platform: 'linux',
  arch: 'wasm32',
  argv: [],
  execArgv: [],
  execPath: '/opt/agentos/bin/node',
  env: Object.create(null),
  versions: { node: '24.15.0', openssl: '3.5.5' },
};

try {
  vm.runInContext(readFileSync(join(crateRoot, 'adapter/inert-loader.js'), 'utf8'), context, {
    filename: 'agentos-node-inert-loader.js',
    timeout: 30_000,
  });
} catch (error) {
  console.error(error);
  if (error.cause?.stack) console.error(error.cause.stack);
  process.exit(1);
}

const { loaded, publicIds } = context.__agentOSNodeStdlib;
if (loaded.length !== publicIds.length) {
  throw new Error(`ERR_NODE_STDLIB_INERT_LOAD_SET: loaded ${loaded.length}/${publicIds.length}`);
}
console.log(JSON.stringify({
  node: '24.15.0',
  sourceModules: Object.keys(sources).length,
  publicModules: publicIds.length,
  loadedModules: loaded.length,
  loaded: [...loaded],
}, null, 2));
