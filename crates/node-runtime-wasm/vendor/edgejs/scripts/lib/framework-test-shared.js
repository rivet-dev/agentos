'use strict';

const fs = require('node:fs');
const http = require('node:http');
const https = require('node:https');
const os = require('node:os');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');

function create(options) {
  const ROOT_DIR = options.rootDir || path.resolve(__dirname, '..', '..');
  const TOOL_NAME = options.toolName || 'framework-test';
  const STATE_DIR = path.join(ROOT_DIR, options.stateDirName || '.framework-test');
  const LOG_DIR = path.join(STATE_DIR, 'logs');
  const PNPM_STORE_DIR = path.join(STATE_DIR, 'pnpm-store');
  const EXAMPLES_DIR = path.join(ROOT_DIR, 'wasmer-examples');
  const DEFAULT_RUNNER = options.defaultRunner
    ? (path.isAbsolute(options.defaultRunner)
      ? options.defaultRunner
      : path.join(ROOT_DIR, options.defaultRunner))
    : path.join(ROOT_DIR, 'build-edge', 'edge');
  const DEFAULT_HOST = '127.0.0.1';
  const ROUTES_JSON_BASENAME = 'routes.json';
  const DEFAULT_ROUTE_MATRIX = {
    version: 1,
    routes: [{
      path: '/',
      expect: {
        contentType: 'html',
      },
    }],
  };
  const SUBMODULE_HINT = 'git submodule update --init --recursive wasmer-examples';
  const NODE_HINT = 'Install Node.js and make sure `node` is on PATH before running framework-test.';
  const PNPM_HINT = 'Install pnpm and make sure it is on PATH. For example: corepack enable pnpm';
  const SERVER_READY_TIMEOUT_MS = 45 * 1000;
  const HTTP_REQUEST_TIMEOUT_MS = 5 * 1000;
  const PROCESS_SHUTDOWN_TIMEOUT_MS = 5 * 1000;
  const HTTP_POLL_INTERVAL_MS = 500;
  const MAX_HTTP_REDIRECTS = 5;
  const MAX_RESPONSE_BODY_BYTES = 64 * 1024;
  const PORT_BASE = Number(process.env.FRAMEWORK_TEST_PORT_BASE || '4300');
  const PORT_BLOCK_SIZE = Number(process.env.FRAMEWORK_TEST_PORT_BLOCK_SIZE || '10');
  const USE_COLOR = Boolean(process.stdout.isTTY && !process.env.NO_COLOR);
  const ANSI = {
    blue: '\u001b[34m',
    bold: '\u001b[1m',
    cyan: '\u001b[36m',
    dim: '\u001b[2m',
    gray: '\u001b[90m',
    green: '\u001b[32m',
    magenta: '\u001b[35m',
    red: '\u001b[31m',
    reset: '\u001b[0m',
    yellow: '\u001b[33m',
  };
  const STATUS_COLOR = {
    ERROR: 'red',
    FAIL: 'red',
    INFO: 'gray',
    PASS: 'green',
    SKIP: 'yellow',
    WARN: 'yellow',
  };

  function formatPrefix(level) {
    return `[${new Date().toISOString()}] [${TOOL_NAME}] [${level}]`;
  }

  function colorize(text, colorName, modifiers) {
    if (!USE_COLOR || !colorName) {
      return text;
    }

    const modifierList = Array.isArray(modifiers) ? modifiers : [];
    const prefix = modifierList
      .map((modifier) => ANSI[modifier] || '')
      .join('') + (ANSI[colorName] || '');
    if (!prefix) {
      return text;
    }

    return `${prefix}${text}${ANSI.reset}`;
  }

  function writeLog(level, message, useStderr) {
    const stream = useStderr ? process.stderr : process.stdout;
    const prefix = colorize(formatPrefix(level), STATUS_COLOR[level] || null, level === 'PASS' ? ['bold'] : []);
    stream.write(`${prefix} ${message}${os.EOL}`);
  }

  function log(message) {
    writeLog('INFO', message);
  }

  function logSuccess(message) {
    writeLog('PASS', message);
  }

  function logSkip(message) {
    writeLog('SKIP', message);
  }

  function logWarn(message) {
    writeLog('WARN', message);
  }

  function logError(message) {
    writeLog('ERROR', message, true);
  }

  function logProgress(current, total, message) {
    log(`[${current}/${total}] ${message}`);
  }

  function fail(message, options) {
    const error = new Error(message);
    if (options && typeof options === 'object') {
      Object.assign(error, options);
    }
    throw error;
  }

  function delay(durationMs) {
    return new Promise((resolve) => {
      setTimeout(resolve, durationMs);
    });
  }

  function ensureDir(dirPath) {
    fs.mkdirSync(dirPath, { recursive: true });
  }

  function isExecutable(filePath) {
    try {
      fs.accessSync(filePath, fs.constants.X_OK);
      return true;
    } catch (error) {
      return false;
    }
  }

  function shellQuote(value) {
    return `'${String(value).replace(/'/g, `'\"'\"'`)}'`;
  }

  function readJsonFile(filePath) {
    try {
      return JSON.parse(fs.readFileSync(filePath, 'utf8'));
    } catch (error) {
      fail(`failed to parse JSON at ${filePath}: ${error.message}`);
    }
  }

  function parseSelector(args) {
    if (args.length > 1) {
      fail(`expected at most one framework selector, got: ${args.join(' ')}`);
    }

    if (args.length === 0) {
      return null;
    }

    const selector = args[0].replace(/\/+$/, '');
    return selector.startsWith('wasmer-examples/') ? path.basename(selector) : selector;
  }

  function formatDuration(durationMs) {
    if (durationMs < 1000) {
      return `${durationMs}ms`;
    }

    const seconds = durationMs / 1000;
    if (seconds < 60) {
      return `${seconds.toFixed(1)}s`;
    }

    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = Math.round(seconds % 60);
    return `${minutes}m ${remainingSeconds}s`;
  }

  function printSection(title, colorName, subtitle) {
    const line = '='.repeat(24);
    process.stdout.write(os.EOL);
    process.stdout.write(`${colorize(`${line} ${title} ${line}`, colorName, ['bold'])}${os.EOL}`);
    if (subtitle) {
      process.stdout.write(`${colorize(subtitle, 'gray')}${os.EOL}`);
    }
  }

  function printStageSummary(result) {
    const testedCount = result.testedProjects.length;
    log(`${result.stage.label} tested ${testedCount} framework${testedCount === 1 ? '' : 's'}`);

    if (result.passed.length > 0) {
      logSuccess(`${result.stage.label} passed (${result.passed.length}): ${result.passed.map((entry) => entry.project.name).join(', ')}`);
    } else {
      logSkip(`${result.stage.label} passed (0): none`);
    }

    if (result.failed.length > 0) {
      logError(`${result.stage.label} failed (${result.failed.length}): ${result.failed.map((entry) => entry.project.name).join(', ')}`);
      for (const failure of result.failed) {
        process.stderr.write(`${colorize('  FAIL', 'red', ['bold'])} ${failure.project.name}: ${failure.detail}${failure.logPath ? ` [log: ${failure.logPath}]` : ''}${os.EOL}`);
      }
    } else {
      logSuccess(`${result.stage.label} failed (0): none`);
    }

    if (result.skipped.length > 0) {
      logSkip(`${result.stage.label} skipped (${result.skipped.length}): ${result.skipped.map((entry) => entry.project.name).join(', ')}`);
      for (const skipped of result.skipped) {
        process.stderr.write(`${colorize('  SKIP', 'yellow', ['bold'])} ${skipped.project.name}: ${skipped.reason || 'skipped'}${os.EOL}`);
      }
    }
  }

  function printMatrixSummary(stageResults, allProjects, summaryTitle) {
    printSection(summaryTitle || 'Framework Summary', 'blue', `${allProjects.length} discovered`);

    for (const result of stageResults) {
      const passedNames = result.passed.map((entry) => entry.project.name);
      const failedNames = result.failed.map((entry) => entry.project.name);
      const skippedNames = result.skipped.map((entry) => entry.project.name);

      process.stdout.write(`${colorize(result.stage.label, result.stage.color, ['bold'])}${os.EOL}`);
      process.stdout.write(`  ${colorize('PASS', 'green', ['bold'])} ${passedNames.length > 0 ? passedNames.join(', ') : 'none'}${os.EOL}`);
      process.stdout.write(`  ${colorize('FAIL', 'red', ['bold'])} ${failedNames.length > 0 ? failedNames.join(', ') : 'none'}${os.EOL}`);
      process.stdout.write(`  ${colorize('SKIP', 'yellow', ['bold'])} ${skippedNames.length > 0 ? skippedNames.join(', ') : 'none'}${os.EOL}`);
    }

    for (let index = 1; index < stageResults.length; index += 1) {
      const previousResult = stageResults[index - 1];
      const currentResult = stageResults[index];
      printSection('Framework Delta', currentResult.stage.color, `${previousResult.stage.label} -> ${currentResult.stage.label}`);

      const regressions = currentResult.failed.filter((failure) =>
        previousResult.passed.some((entry) => entry.project.name === failure.project.name));

      if (regressions.length === 0) {
        logSuccess(`no regressions between ${previousResult.stage.label} and ${currentResult.stage.label}`);
        continue;
      }

      logError(`regressions (${regressions.length}) where ${previousResult.stage.label} passed but ${currentResult.stage.label} failed`);
      for (const regression of regressions) {
        process.stderr.write(`${colorize('  FAIL', 'red', ['bold'])} ${regression.project.name}: ${regression.detail}${regression.logPath ? ` [log: ${regression.logPath}]` : ''}${os.EOL}`);
      }
    }
  }

  function buildRunnerDisplay(runnerCommandParts) {
    return normalizeRunnerCommandParts(runnerCommandParts)
      .map(shellQuote)
      .join(' ');
  }

  function supportsSafeRunner(targetPath) {
    const baseName = path.basename(targetPath);
    if (baseName === 'edge-wasix-framework-runner.sh') {
      return false;
    }

    if (path.resolve(targetPath) === path.resolve(DEFAULT_RUNNER)) {
      return true;
    }

    return /(^|[^a-z])edge(js)?([^a-z]|$)/i.test(baseName);
  }

  function createIndependentRunnerStage(stageOptions) {
    return {
      allowsProductionFallback: false,
      color: stageOptions.color,
      key: stageOptions.key,
      label: stageOptions.label,
      runnerCommandParts: stageOptions.runnerCommandParts.slice(),
      runnerDisplay: buildRunnerDisplay(stageOptions.runnerCommandParts),
      selectProjects(projects) {
        return projects.slice();
      },
      skippedProjects() {
        return [];
      },
    };
  }

  function createDependentRunnerStage(stageOptions) {
    const runnerCommandParts = stageOptions.runnerCommandParts.slice();

    return {
      allowsProductionFallback: true,
      color: stageOptions.color,
      key: stageOptions.key,
      label: stageOptions.label,
      runnerCommandParts,
      runnerDisplay: buildRunnerDisplay(runnerCommandParts),
      selectProjects(projects, previousResult) {
        if (!previousResult) {
          return [];
        }

        return previousResult.passed.map((entry) => entry.project);
      },
      skippedProjects(projects, selectedProjects, previousResult) {
        if (!previousResult) {
          return [];
        }

        const selectedNames = new Set(selectedProjects.map((project) => project.name));
        const skippedReasons = new Map();

        for (const failure of previousResult.failed) {
          skippedReasons.set(failure.project.name, `${previousResult.stage.label} failed`);
        }
        for (const skipped of previousResult.skipped) {
          skippedReasons.set(skipped.project.name, skipped.reason || `${previousResult.stage.label} skipped`);
        }

        return projects
          .filter((project) => !selectedNames.has(project.name))
          .map((project) => ({
            project,
            reason: skippedReasons.get(project.name) || `${previousResult.stage.label} failed`,
          }));
      },
    };
  }

  function buildRunnerStages(nodeRunner, comparisonRunner) {
    const stages = [
      createIndependentRunnerStage({
        color: nodeRunner.color,
        key: nodeRunner.key,
        label: nodeRunner.label,
        runnerCommandParts: [nodeRunner.targetPath],
      }),
    ];

    if (path.resolve(nodeRunner.targetPath) === path.resolve(comparisonRunner.targetPath)) {
      logWarn(`comparison runner matches the Node baseline (${comparisonRunner.targetPath}); skipping the EdgeJS and safe stages`);
      return stages;
    }

    const customRunnerLabel = process.env.FRAMEWORK_TEST_RUNNER_LABEL &&
      process.env.FRAMEWORK_TEST_RUNNER_LABEL.trim();
    const comparisonLabel = customRunnerLabel ||
      (path.resolve(comparisonRunner.targetPath) === path.resolve(DEFAULT_RUNNER)
        ? 'EdgeJS Native'
        : 'Comparison Runner');
    const comparisonKey = path.resolve(comparisonRunner.targetPath) === path.resolve(DEFAULT_RUNNER)
      ? 'edgejs'
      : 'comparison';

    stages.push(createDependentRunnerStage({
      color: 'magenta',
      key: comparisonKey,
      label: comparisonLabel,
      runnerCommandParts: [comparisonRunner.targetPath],
    }));

    if (process.env.FRAMEWORK_TEST_SKIP_SAFE === '1') {
      logWarn('FRAMEWORK_TEST_SKIP_SAFE=1; skipping the safe stage');
      return stages;
    }

    if (!supportsSafeRunner(comparisonRunner.targetPath)) {
      logWarn(`comparison runner does not look like an EdgeJS binary (${comparisonRunner.targetPath}); skipping the safe stage`);
      return stages;
    }

    stages.push(createDependentRunnerStage({
      color: 'yellow',
      key: `${comparisonKey}-safe`,
      label: path.resolve(comparisonRunner.targetPath) === path.resolve(DEFAULT_RUNNER)
        ? 'Wasmer + EdgeJS Safe'
        : 'Comparison Runner Safe',
      runnerCommandParts: [comparisonRunner.targetPath, '--safe'],
    }));

    return stages;
  }

  function resolveHostNodeRunner() {
    const result = spawnSync('node', ['-e', 'process.stdout.write(process.execPath)'], {
      cwd: ROOT_DIR,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });

    if (result.error || result.status !== 0 || !result.stdout.trim()) {
      fail(`node is required for the baseline framework run.\n${NODE_HINT}`);
    }

    const targetPath = result.stdout.trim();
    if (!isExecutable(targetPath)) {
      fail(`resolved node runner is not executable: ${targetPath}\n${NODE_HINT}`);
    }

    return {
      color: 'cyan',
      key: 'node',
      label: 'Node.js',
      targetPath,
    };
  }

  function runSyncOrFail(command, args, runOptions, errorMessage) {
    const result = spawnSync(command, args, runOptions);
    if (result.error) {
      fail(`${errorMessage}: ${result.error.message}`);
    }
    if (result.status !== 0) {
      fail(`${errorMessage}: exit code ${result.status}`);
    }
  }

  function resolveRunnerTarget() {
    const rawTarget = process.env.SYMLINK_TARGET && process.env.SYMLINK_TARGET.trim()
      ? process.env.SYMLINK_TARGET.trim()
      : DEFAULT_RUNNER;
    const targetPath = path.isAbsolute(rawTarget) ? rawTarget : path.resolve(ROOT_DIR, rawTarget);
    const defaultRunner = path.resolve(DEFAULT_RUNNER);
    const usingDefaultRunner = path.resolve(targetPath) === defaultRunner;

    if (usingDefaultRunner && !isExecutable(targetPath)) {
      log('default runner missing; building EdgeJS via make build');
      runSyncOrFail('make', ['build'], {
        cwd: ROOT_DIR,
        stdio: 'inherit',
      }, 'failed to build EdgeJS');
    }

    if (!isExecutable(targetPath)) {
      fail(`runner target is not executable: ${targetPath}`);
    }

    return { targetPath };
  }

  function normalizeRunnerCommandParts(runnerTarget) {
    if (Array.isArray(runnerTarget)) {
      return runnerTarget.slice();
    }

    if (typeof runnerTarget === 'string') {
      return [runnerTarget];
    }

    fail(`invalid runner target: ${runnerTarget == null ? 'missing' : String(runnerTarget)}`);
  }

  function removeFileOrSymlink(targetPath) {
    if (!fs.existsSync(targetPath)) {
      return;
    }

    const stat = fs.lstatSync(targetPath);
    if (stat.isDirectory() && !stat.isSymbolicLink()) {
      fail(`refusing to remove directory at ${targetPath}`);
    }

    fs.rmSync(targetPath, { force: true });
  }

  function buildRunnerShimScript(runnerCommandParts) {
    return [
      '#!/bin/sh',
      `exec ${runnerCommandParts.map(shellQuote).join(' ')} "$@"`,
      '',
    ].join('\n');
  }

  function installRunnerShim(nodeShimPath, runnerCommandParts) {
    if (runnerCommandParts.length === 1) {
      fs.symlinkSync(runnerCommandParts[0], nodeShimPath);
      return;
    }

    fs.writeFileSync(nodeShimPath, buildRunnerShimScript(runnerCommandParts), { mode: 0o755 });
    fs.chmodSync(nodeShimPath, 0o755);
  }

  function validateRunnerShim(project, nodeShimPath, runnerCommandParts) {
    if (runnerCommandParts.length === 1) {
      const resolvedShim = fs.realpathSync(nodeShimPath);
      const resolvedTarget = fs.realpathSync(runnerCommandParts[0]);
      if (resolvedShim !== resolvedTarget) {
        fail(`runner shim for ${project.name} does not resolve to ${runnerCommandParts[0]}`);
      }
      return;
    }

    if (!isExecutable(nodeShimPath)) {
      fail(`runner shim for ${project.name} is not executable: ${nodeShimPath}`);
    }

    const expectedScript = buildRunnerShimScript(runnerCommandParts);
    const actualScript = fs.readFileSync(nodeShimPath, 'utf8');
    if (actualScript !== expectedScript) {
      fail(`runner shim for ${project.name} did not match the expected wrapper`);
    }
  }

  function findCompatibleLaunchers(binDir) {
    if (!fs.existsSync(binDir) || !fs.statSync(binDir).isDirectory()) {
      fail(`expected pnpm launcher directory to exist: ${binDir}`);
    }

    const compatible = [];
    const entries = fs.readdirSync(binDir, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isFile() && !entry.isSymbolicLink()) {
        continue;
      }
      if (entry.name === 'node') {
        continue;
      }

      const launcherPath = path.join(binDir, entry.name);
      let content;
      try {
        content = fs.readFileSync(launcherPath, 'utf8');
      } catch (error) {
        continue;
      }

      if (content.includes('$basedir/node')) {
        compatible.push(entry.name);
      }
    }

    return compatible.sort();
  }

  function injectRunner(project, runnerTarget) {
    const binDir = path.join(project.dir, 'node_modules', '.bin');
    const compatibleLaunchers = findCompatibleLaunchers(binDir);
    const runnerCommandParts = normalizeRunnerCommandParts(runnerTarget);

    if (compatibleLaunchers.length === 0) {
      fail(`no compatible pnpm launcher was found for ${project.name} in ${binDir}`);
    }

    const nodeShimPath = path.join(binDir, 'node');
    removeFileOrSymlink(nodeShimPath);
    installRunnerShim(nodeShimPath, runnerCommandParts);
    validateRunnerShim(project, nodeShimPath, runnerCommandParts);

    return {
      compatibleLaunchers,
      project,
    };
  }

  let pnpmMajorVersion;

  function detectPnpmMajorVersion() {
    if (pnpmMajorVersion !== undefined) {
      return pnpmMajorVersion;
    }
    const result = spawnSync('pnpm', ['--version'], {
      cwd: ROOT_DIR,
      encoding: 'utf8',
    });
    const match = !result.error && result.status === 0
      ? String(result.stdout || '').trim().match(/^(\d+)\./)
      : null;
    pnpmMajorVersion = match ? Number(match[1]) : null;
    return pnpmMajorVersion;
  }

  function ensurePnpm() {
    if (detectPnpmMajorVersion() === null) {
      fail(`pnpm is required but was not found on PATH.\n${PNPM_HINT}`);
    }
  }

  // pnpm 11 dropped support for the `pnpm` field in package.json; settings
  // (including onlyBuiltDependencies) now live in pnpm-workspace.yaml. Only
  // pnpm <= 10 still reads onlyBuiltDependencies from package.json, and only
  // there does it conflict with --config.dangerouslyAllowAllBuilds.
  function pnpmReadsPackageJsonPnpmField() {
    const major = detectPnpmMajorVersion();
    return typeof major === 'number' && major <= 10;
  }

  function buildPortCandidates(index) {
    const startPort = PORT_BASE + (index * PORT_BLOCK_SIZE);
    return Array.from({ length: PORT_BLOCK_SIZE }, (_, offset) => startPort + offset);
  }

  function createFailureRecord(project, stage, error) {
    const message = error && error.message ? error.message : String(error);
    return {
      detail: error && error.detail ? error.detail : message,
      logPath: error && error.logPath ? error.logPath : null,
      message,
      project,
      stage,
    };
  }

  function normalizeHeaders(headers) {
    const normalized = {};
    for (const [name, value] of Object.entries(headers || {})) {
      if (Array.isArray(value)) {
        normalized[name.toLowerCase()] = value.join(', ');
        continue;
      }

      normalized[name.toLowerCase()] = value == null ? '' : String(value);
    }
    return normalized;
  }

  function defaultAcceptHeader(contentTypeMode) {
    if (contentTypeMode === 'json') {
      return 'application/json,*/*';
    }
    if (contentTypeMode === 'any') {
      return '*/*';
    }
    return 'text/html,*/*';
  }

  function serializeRequestBody(body) {
    if (body == null) {
      return null;
    }
    if (typeof body === 'string' || Buffer.isBuffer(body)) {
      return body;
    }
    return JSON.stringify(body);
  }

  function normalizeRequestHttpOptions(requestOptions) {
    if (typeof requestOptions === 'number') {
      return {
        accept: 'text/html,*/*',
        body: null,
        contentType: 'text/plain; charset=utf-8',
        headers: {},
        method: 'GET',
        redirectCount: requestOptions,
      };
    }

    const options = requestOptions && typeof requestOptions === 'object' ? requestOptions : {};
    const contentTypeMode = options.contentType || 'html';
    return {
      accept: options.accept || defaultAcceptHeader(contentTypeMode),
      body: serializeRequestBody(options.body),
      contentType: options.requestContentType || 'application/json; charset=utf-8',
      headers: options.headers || {},
      method: (options.method || 'GET').toUpperCase(),
      redirectCount: typeof options.redirectCount === 'number' ? options.redirectCount : 0,
    };
  }

  function requestHttp(url, requestOptions) {
    const options = normalizeRequestHttpOptions(requestOptions);

    return new Promise((resolve) => {
      let requestUrl;
      try {
        requestUrl = new URL(url);
      } catch (error) {
        resolve({ error, ok: false });
        return;
      }

      const client = requestUrl.protocol === 'https:' ? https : http;
      const headers = {
        Accept: options.accept,
        ...options.headers,
      };
      const body = options.body;
      if (body != null && !headers['content-type'] && !headers['Content-Type']) {
        headers['Content-Type'] = options.contentType;
      }

      const request = client.request(requestUrl, {
        headers,
        method: options.method,
      }, (response) => {
        const responseHeaders = normalizeHeaders(response.headers);
        const statusCode = response.statusCode || 0;
        const location = responseHeaders.location;

        if (statusCode >= 300 && statusCode < 400 && location && options.redirectCount < MAX_HTTP_REDIRECTS) {
          response.resume();
          resolve(requestHttp(new URL(location, requestUrl).toString(), {
            ...options,
            redirectCount: options.redirectCount + 1,
          }));
          return;
        }

        const chunks = [];
        let totalBytes = 0;
        response.on('data', (chunk) => {
          const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
          if (totalBytes >= MAX_RESPONSE_BODY_BYTES) {
            return;
          }

          const remaining = MAX_RESPONSE_BODY_BYTES - totalBytes;
          const slice = buffer.subarray(0, remaining);
          chunks.push(slice);
          totalBytes += slice.length;
        });

        response.on('end', () => {
          resolve({
            body: Buffer.concat(chunks).toString('utf8'),
            finalUrl: requestUrl.toString(),
            headers: responseHeaders,
            ok: true,
            statusCode,
          });
        });
      });

      request.setTimeout(HTTP_REQUEST_TIMEOUT_MS, () => {
        request.destroy(new Error(`request timed out after ${HTTP_REQUEST_TIMEOUT_MS}ms`));
      });
      request.on('error', (error) => {
        resolve({ error, ok: false });
      });

      if (body != null) {
        request.write(body);
      }
      request.end();
    });
  }

  function normalizeRoutePath(routePath) {
    if (routePath === '/') {
      return '/';
    }
    return routePath.replace(/\/+$/, '') || '/';
  }

  function buildRouteUrl(port, routePath) {
    return `http://${DEFAULT_HOST}:${port}${normalizeRoutePath(routePath)}`;
  }

  function categorizeStage(stage) {
    if (stage.key === 'node') {
      return 'node';
    }
    if (stage.key.endsWith('-safe')) {
      return 'safe';
    }
    return 'comparison';
  }

  function routeAppliesToStage(route, stage, runtime) {
    if (route.skipOnStatic && runtime && runtime.mode === 'static-export') {
      return false;
    }

    if (!route.stages || route.stages.length === 0) {
      return true;
    }

    const stageCategory = categorizeStage(stage);
    return route.stages.includes(stageCategory) || route.stages.includes(stage.key);
  }

  function readRouteMatrixConfig(configPath) {
    const config = readJsonFile(configPath);
    if (!config || typeof config !== 'object') {
      fail(`invalid route matrix at ${configPath}: expected an object`);
    }
    if (config.version !== 1) {
      fail(`unsupported route matrix version at ${configPath}: expected version 1`);
    }
    if (!Array.isArray(config.routes) || config.routes.length === 0) {
      fail(`invalid route matrix at ${configPath}: routes must be a non-empty array`);
    }
    return config;
  }

  function normalizeRouteDefinition(route, index, configPath) {
    if (!route || typeof route !== 'object') {
      fail(`invalid route at index ${index} in ${configPath}: expected an object`);
    }
    if (typeof route.path !== 'string' || !route.path.startsWith('/')) {
      fail(`invalid route at index ${index} in ${configPath}: path must start with "/"`);
    }

    const expect = route.expect && typeof route.expect === 'object' ? route.expect : {};
    const status = expect.status == null ? [200, 304] : expect.status;
    const normalizedStatus = Array.isArray(status) ? status : [status];

    return {
      body: route.body,
      expect: {
        bodyContains: Array.isArray(expect.bodyContains) ? expect.bodyContains.slice() : [],
        bodyRegex: Array.isArray(expect.bodyRegex) ? expect.bodyRegex.slice() : [],
        contentType: expect.contentType || 'html',
        status: normalizedStatus,
      },
      headers: route.headers && typeof route.headers === 'object' ? route.headers : {},
      method: (route.method || 'GET').toUpperCase(),
      name: route.name || route.path,
      path: normalizeRoutePath(route.path),
      skipOnStatic: Boolean(route.skipOnStatic),
      stages: Array.isArray(route.stages) ? route.stages.slice() : null,
    };
  }

  function routesJsonPath(project, routesFile) {
    return path.join(project.dir, routesFile || ROUTES_JSON_BASENAME);
  }

  function loadRouteMatrix(project, stage, runtime, routesFile) {
    const configPath = routesJsonPath(project, routesFile);
    const config = fs.existsSync(configPath)
      ? readRouteMatrixConfig(configPath)
      : DEFAULT_ROUTE_MATRIX;

    return config.routes
      .map((route, index) => normalizeRouteDefinition(route, index, configPath))
      .filter((route) => routeAppliesToStage(route, stage, runtime));
  }

  function routeReadinessPath(project, stage, runtime, routesFile) {
    const routes = loadRouteMatrix(project, stage, runtime, routesFile);
    if (routes.length === 0) {
      return '/';
    }
    return routes[0].path;
  }

  function summarizeBodySnippet(body) {
    const compact = String(body || '').replace(/\s+/g, ' ').trim();
    if (!compact) {
      return '(empty body)';
    }
    if (compact.length <= 120) {
      return compact;
    }
    return `${compact.slice(0, 117)}...`;
  }

  function bodyLooksLikeJson(body) {
    const trimmed = String(body || '').trim();
    return trimmed.startsWith('{') || trimmed.startsWith('[');
  }

  function bodyLooksLikeHtml(body) {
    const lower = body.toLowerCase();
    return lower.includes('<!doctype html') || lower.includes('<html') || lower.includes('<body') || lower.includes('<head');
  }

  function validateRouteResponse(route, response, project, runtime) {
    const label = `route "${route.name}" (${route.path})`;
    const allowedStatuses = route.expect.status;
    if (!allowedStatuses.includes(response.statusCode)) {
      fail(`${label} unexpected HTTP status for ${project.name} via ${runtime.name}: ${response.statusCode} at ${response.finalUrl} (expected ${allowedStatuses.join(' or ')})`);
    }

    const contentType = response.headers['content-type'] || '';
    const contentTypeMode = route.expect.contentType;
    if (contentTypeMode === 'html' && !/text\/html|application\/xhtml\+xml/i.test(contentType) && !bodyLooksLikeHtml(response.body)) {
      fail(`${label} unexpected response for ${project.name} via ${runtime.name}: content-type ${contentType || 'missing'} at ${response.finalUrl}`);
    } else if (contentTypeMode === 'json' && !/application\/json/i.test(contentType) && !bodyLooksLikeJson(response.body)) {
      fail(`${label} unexpected response for ${project.name} via ${runtime.name}: content-type ${contentType || 'missing'} at ${response.finalUrl}`);
    }

    for (const substring of route.expect.bodyContains) {
      if (!response.body.includes(substring)) {
        const snippet = summarizeBodySnippet(response.body);
        fail(`${label} expected body to contain ${JSON.stringify(substring)} for ${project.name} via ${runtime.name} at ${response.finalUrl}; body snippet: ${snippet}`);
      }
    }

    for (const pattern of route.expect.bodyRegex) {
      let regex;
      try {
        regex = new RegExp(pattern);
      } catch (error) {
        fail(`${label} invalid bodyRegex ${JSON.stringify(pattern)} in routes.json for ${project.name}: ${error.message}`);
      }
      if (!regex.test(response.body)) {
        const snippet = summarizeBodySnippet(response.body);
        fail(`${label} expected body to match /${pattern}/ for ${project.name} via ${runtime.name} at ${response.finalUrl}; body snippet: ${snippet}`);
      }
    }
  }

  async function validateRouteMatrix(project, runtime, port, routes) {
    const routeResults = [];
    for (const route of routes) {
      const url = buildRouteUrl(port, route.path);
      const response = await requestHttp(url, {
        accept: defaultAcceptHeader(route.expect.contentType),
        body: route.body,
        contentType: route.expect.contentType,
        headers: route.headers,
        method: route.method,
      });

      if (!response.ok) {
        fail(`route "${route.name}" (${route.path}) request failed for ${project.name} via ${runtime.name}: ${response.error.message}`, {
          detail: `url=${url}`,
        });
      }

      validateRouteResponse(route, response, project, runtime);
      routeResults.push({
        name: route.name,
        path: route.path,
        response,
        statusCode: response.statusCode,
      });
      log(`route "${route.name}" (${route.path}) passed for ${project.name} via ${runtime.name}: HTTP ${response.statusCode}`);
    }

    if (routeResults.length === 0) {
      fail(`no routes configured for ${project.name} on ${runtime.name}`);
    }

    return routeResults;
  }

  function isMissingProcessError(error) {
    return Boolean(error && error.code === 'ESRCH');
  }

  function formatProcessFailure(handle) {
    if (handle.errorMessage) {
      return `process failed to spawn: ${handle.errorMessage}`;
    }
    if (handle.signal) {
      return `process exited from signal ${handle.signal}`;
    }
    if (handle.exitCode !== null) {
      return `process exited with code ${handle.exitCode}`;
    }
    return 'process exited unexpectedly';
  }

  function summarizeLogFailure(logPath, fallbackError) {
    if (!logPath || !fs.existsSync(logPath)) {
      return fallbackError && fallbackError.message ? fallbackError.message : 'see log for details';
    }

    const lines = fs.readFileSync(logPath, 'utf8')
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean)
      .filter((line) => !line.startsWith('[20') || !line.includes(`[${TOOL_NAME}] [INFO]`))
      .filter((line) => !line.startsWith('[20') || !line.includes(`[${TOOL_NAME}] [WARN]`))
      .filter((line) => !line.startsWith('[20') || !line.includes(`[${TOOL_NAME}] [ERROR] exit:`));

    const patternGroups = [
      /Minimum Node\.js version not met/i,
      /Requirement:\s*Node\.js/i,
      /undefined symbol/i,
      /symbol lookup error/i,
      /Cannot find module/i,
      /does not work with/i,
      /Failed to fetch/i,
      /error when starting/i,
      /listen E[A-Z]+/i,
      /^\[ERROR\]/i,
      /^Error:/i,
      /^error\b/i,
    ];

    const matches = [];
    for (const pattern of patternGroups) {
      for (const line of lines) {
        if (pattern.test(line) && !matches.includes(line)) {
          matches.push(line);
        }
      }
    }

    if (matches.length > 0) {
      return matches.slice(0, 2).join(' | ');
    }

    const fallbackLines = lines.filter((line) => !/ELIFECYCLE|^\[INFO\]/.test(line));
    if (fallbackLines.length > 0) {
      return fallbackLines.slice(-2).join(' | ');
    }

    return fallbackError && fallbackError.message ? fallbackError.message : 'see log for details';
  }

  async function stopProcess(handle) {
    if (!handle) {
      return;
    }

    if (handle.exited) {
      await handle.exitPromise;
      return;
    }

    const pid = handle.child && handle.child.pid ? handle.child.pid : null;
    if (pid) {
      try {
        process.kill(-pid, 'SIGTERM');
      } catch (error) {
        if (!isMissingProcessError(error)) {
          try {
            handle.child.kill('SIGTERM');
          } catch (killError) {
            if (!isMissingProcessError(killError)) {
              throw killError;
            }
          }
        }
      }
    } else {
      try {
        handle.child.kill('SIGTERM');
      } catch (error) {
        if (!isMissingProcessError(error)) {
          throw error;
        }
      }
    }

    const exitedAfterTerm = await Promise.race([
      handle.exitPromise.then(() => true),
      delay(PROCESS_SHUTDOWN_TIMEOUT_MS).then(() => false),
    ]);

    if (exitedAfterTerm) {
      return;
    }

    if (pid) {
      try {
        process.kill(-pid, 'SIGKILL');
      } catch (error) {
        if (!isMissingProcessError(error)) {
          try {
            handle.child.kill('SIGKILL');
          } catch (killError) {
            if (!isMissingProcessError(killError)) {
              throw killError;
            }
          }
        }
      }
    } else {
      try {
        handle.child.kill('SIGKILL');
      } catch (error) {
        if (!isMissingProcessError(error)) {
          throw error;
        }
      }
    }

    await handle.exitPromise;
  }

  function spawnLoggedProcess(options) {
    const logStream = fs.createWriteStream(options.logPath, { flags: options.append ? 'a' : 'w' });
    logStream.write(`${formatPrefix('INFO')} ${options.description}${os.EOL}`);
    if (options.commandDisplay) {
      logStream.write(`${formatPrefix('INFO')} command: ${options.commandDisplay}${os.EOL}`);
    }

    const child = spawn(options.shellCommand, {
      cwd: options.cwd,
      detached: true,
      env: options.env,
      shell: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    const handle = {
      child,
      description: options.description,
      errorMessage: null,
      exitCode: null,
      exited: false,
      exitPromise: null,
      logPath: options.logPath,
      signal: null,
    };

    handle.exitPromise = new Promise((resolve) => {
      const finish = (result) => {
        if (handle.exited) {
          return;
        }
        handle.exited = true;
        handle.exitCode = result.exitCode;
        handle.signal = result.signal;
        handle.errorMessage = result.errorMessage;
        resolve(result);
      };

      child.on('error', (error) => {
        logStream.write(`${formatPrefix('ERROR')} spawn error: ${error.message}${os.EOL}`);
        finish({ errorMessage: error.message, exitCode: 1, signal: null });
      });

      child.stdout.on('data', (chunk) => {
        logStream.write(chunk);
      });
      child.stderr.on('data', (chunk) => {
        logStream.write(chunk);
      });

      child.on('close', (code, signal) => {
        if (signal) {
          logStream.write(`${formatPrefix('WARN')} signal: ${signal}${os.EOL}`);
        }
        logStream.write(`${formatPrefix(code === 0 ? 'INFO' : 'ERROR')} exit: ${code}${os.EOL}`);
        finish({ errorMessage: null, exitCode: code, signal: signal || null });
      });
    });

    return handle;
  }

  async function waitForHttpResponse(handle, url) {
    const deadline = Date.now() + SERVER_READY_TIMEOUT_MS;
    let lastError = null;

    while (Date.now() < deadline) {
      if (handle.exited) {
        fail(formatProcessFailure(handle), {
          detail: summarizeLogFailure(handle.logPath),
          logPath: handle.logPath,
        });
      }

      const response = await requestHttp(url);
      if (response.ok) {
        return response;
      }

      lastError = response.error;
      await delay(HTTP_POLL_INTERVAL_MS);
    }

    if (handle.exited) {
      fail(formatProcessFailure(handle), {
        detail: summarizeLogFailure(handle.logPath),
        logPath: handle.logPath,
      });
    }

    fail(`timed out waiting for ${url}${lastError ? `: ${lastError.message}` : ''}`, {
      detail: summarizeLogFailure(handle.logPath, lastError),
      logPath: handle.logPath,
    });
  }

  async function readLogTail(logPath, maxLines = 40) {
    try {
      const content = await fs.promises.readFile(logPath, 'utf8');
      const lines = content.split(/\r?\n/);
      if (lines.length <= maxLines) {
        return content.trimEnd();
      }
      return lines.slice(-maxLines).join('\n');
    } catch (error) {
      return `(unable to read ${logPath}: ${error.message})`;
    }
  }

  function readProjectPackageJson(project) {
    try {
      return JSON.parse(fs.readFileSync(path.join(project.dir, 'package.json'), 'utf8'));
    } catch {
      return null;
    }
  }

  function projectHasOnlyBuiltDependencies(project) {
    const pkg = readProjectPackageJson(project);
    return Array.isArray(pkg?.pnpm?.onlyBuiltDependencies)
      && pkg.pnpm.onlyBuiltDependencies.length > 0;
  }

  function pnpmInstallArgs(project) {
    const args = [
      'install',
      '--no-lockfile',
      '--store-dir', PNPM_STORE_DIR,
    ];
    // Approve all dependency build scripts so installs do not abort on
    // ERR_PNPM_IGNORED_BUILDS. pnpm <= 10 rejects combining
    // dangerouslyAllowAllBuilds with a package.json pnpm.onlyBuiltDependencies
    // field (js-gatsby-staticsite2); pnpm 11 ignores that field entirely, so
    // the flag is always safe and required there.
    if (!(pnpmReadsPackageJsonPnpmField() && projectHasOnlyBuiltDependencies(project))) {
      args.push('--config.dangerouslyAllowAllBuilds=true');
    }
    return args;
  }

  async function formatPnpmInstallFailures(failures) {
    const lines = ['one or more pnpm install commands failed:'];
    for (const failure of failures) {
      lines.push(`- ${failure.project.name}: ${failure.logPath}`);
      lines.push('--- log tail ---');
      lines.push(await readLogTail(failure.logPath));
      lines.push('--- end log tail ---');
    }
    return lines.join('\n');
  }

  async function installProject(project) {
    const logPath = path.join(LOG_DIR, `${project.name}.pnpm-install.log`);
    const startedAt = Date.now();

    return new Promise((resolve) => {
      const logStream = fs.createWriteStream(logPath, { flags: 'w' });
      let settled = false;
      const finish = (result) => {
        if (settled) {
          return;
        }
        settled = true;
        logStream.end(() => resolve({
          durationMs: Date.now() - startedAt,
          ...result,
        }));
      };

      logStream.write(`${formatPrefix('INFO')} pnpm install in ${project.name}${os.EOL}`);

      const child = spawn('pnpm', pnpmInstallArgs(project), {
        cwd: project.dir,
        env: {
          ...process.env,
          CI: process.env.CI || 'true',
        },
        stdio: ['ignore', 'pipe', 'pipe'],
      });

      child.stdout.on('data', (chunk) => {
        logStream.write(chunk);
      });
      child.stderr.on('data', (chunk) => {
        logStream.write(chunk);
      });

      child.on('error', (error) => {
        logStream.write(`${formatPrefix('ERROR')} spawn error: ${error.message}${os.EOL}`);
        finish({ ok: false, project, logPath });
      });

      child.on('close', (code, signal) => {
        if (signal) {
          logStream.write(`${formatPrefix('WARN')} signal: ${signal}${os.EOL}`);
        }
        logStream.write(`${formatPrefix(code === 0 ? 'INFO' : 'ERROR')} exit: ${code}${os.EOL}`);
        finish({ ok: code === 0, project, logPath });
      });
    });
  }

  async function installProjects(projects) {
    const parallel = process.env.FRAMEWORK_TEST_PARALLEL_PNPM === '1';
    log(`running pnpm install ${parallel ? 'in parallel' : 'serially'} across ${projects.length} framework${projects.length === 1 ? '' : 's'}`);

    let completed = 0;
    const runInstall = async (project, index) => {
      log(`pnpm install started for ${project.name} (${index + 1}/${projects.length})`);
      const result = await installProject(project);
      completed += 1;
      const status = result.ok ? 'completed' : 'failed';
      log(`pnpm install ${status} for ${project.name} (${completed}/${projects.length}, ${formatDuration(result.durationMs)})`);
      return result;
    };

    let results;
    if (parallel) {
      results = await Promise.all(projects.map((project, index) => runInstall(project, index)));
    } else {
      results = [];
      for (let index = 0; index < projects.length; index += 1) {
        results.push(await runInstall(projects[index], index));
      }
    }

    const failures = results.filter((result) => !result.ok);

    if (failures.length > 0) {
      fail(await formatPnpmInstallFailures(failures));
    }
  }

  async function runRunnerStage(stage, projects, skippedProjects, handlers) {
    printSection(stage.label, stage.color, stage.runnerDisplay);
    const skipped = (skippedProjects || []).slice();

    if (projects.length === 0) {
      logSkip(`no frameworks selected for ${stage.label.toLowerCase()}`);
      const emptyResult = {
        failed: [],
        passed: [],
        skipped,
        stage,
        testedProjects: [],
      };
      printStageSummary(emptyResult);
      return emptyResult;
    }

    const passed = [];
    const failed = [];
    for (let index = 0; index < projects.length; index += 1) {
      const project = projects[index];

      try {
        const preparation = await handlers.prepareProject(project, stage);
        const result = await handlers.testProject(project, stage, index, projects.length, preparation);
        passed.push({
          ...result,
          compatibleLaunchers: preparation.compatibleLaunchers,
          project,
        });
        const routeSummary = result.routeResults
          ? `${result.routeResults.length}/${result.routeResults.length} routes`
          : `HTTP ${result.response.statusCode}`;
        logSuccess(`validated ${project.name}: ${routeSummary} via ${result.runtime.name} on ${DEFAULT_HOST}:${result.port}`);
      } catch (error) {
        if (error && error.skip) {
          skipped.push({
            project,
            reason: error.detail || error.message || 'skipped',
          });
          logSkip(`${project.name} skipped on ${stage.label}: ${error.detail || error.message}`);
          continue;
        }
        const failure = createFailureRecord(project, stage, error);
        failed.push(failure);
        logError(`${project.name} failed on ${stage.label}: ${failure.detail}`);
      }
    }

    const result = {
      failed,
      passed,
      skipped,
      stage,
      testedProjects: projects,
    };
    printStageSummary(result);
    return result;
  }

  return {
    ANSI,
    DEFAULT_HOST,
    DEFAULT_ROUTE_MATRIX,
    DEFAULT_RUNNER,
    EXAMPLES_DIR,
    LOG_DIR,
    NODE_HINT,
    PNPM_HINT,
    PNPM_STORE_DIR,
    PORT_BASE,
    PORT_BLOCK_SIZE,
    ROOT_DIR,
    ROUTES_JSON_BASENAME,
    STATE_DIR,
    STATUS_COLOR,
    SUBMODULE_HINT,
    buildPortCandidates,
    buildRouteUrl,
    buildRunnerDisplay,
    buildRunnerStages,
    colorize,
    createDependentRunnerStage,
    createFailureRecord,
    createIndependentRunnerStage,
    delay,
    ensureDir,
    ensurePnpm,
    fail,
    findCompatibleLaunchers,
    formatDuration,
    formatPrefix,
    injectRunner,
    installProjects,
    isExecutable,
    loadRouteMatrix,
    log,
    logError,
    logProgress,
    logSkip,
    logSuccess,
    logWarn,
    normalizeRoutePath,
    normalizeRunnerCommandParts,
    parseSelector,
    printMatrixSummary,
    printSection,
    printStageSummary,
    readJsonFile,
    removeFileOrSymlink,
    requestHttp,
    resolveHostNodeRunner,
    resolveRunnerTarget,
    routeReadinessPath,
    runRunnerStage,
    runSyncOrFail,
    shellQuote,
    spawnLoggedProcess,
    stopProcess,
    supportsSafeRunner,
    validateRouteMatrix,
    waitForHttpResponse,
  };
}

module.exports = {
  create,
};
