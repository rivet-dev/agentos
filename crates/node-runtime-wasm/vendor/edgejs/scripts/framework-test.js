// Intentionally plain JavaScript because the current local EdgeJS build does not
// have native TypeScript stripping enabled.

'use strict';

const fs = require('node:fs');
const http = require('node:http');
const https = require('node:https');
const os = require('node:os');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');

const harness = require('./lib/framework-test-shared').create({
  rootDir: path.resolve(__dirname, '..'),
  toolName: 'framework-test',
  stateDirName: '.framework-test',
});

const ROOT_DIR = harness.ROOT_DIR;
const EXAMPLES_DIR = harness.EXAMPLES_DIR;
const STATE_DIR = harness.STATE_DIR;
const LOG_DIR = harness.LOG_DIR;
const PNPM_STORE_DIR = harness.PNPM_STORE_DIR;
const installProjects = harness.installProjects;
const DEFAULT_RUNNER = harness.DEFAULT_RUNNER;
const DEFAULT_HOST = harness.DEFAULT_HOST;
const STATIC_SERVER_SCRIPT_BASENAME = '.framework-test-static-server.cjs';
const LEGACY_STATIC_SERVER_SCRIPT_BASENAME = '.framework-test-static-server.js';
const STATIC_SERVER_RESOLVER_MARKER = 'framework-test-static-server-v2';
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
const RUNTIME_SCRIPT_PRIORITY = {
  preview: 100,
  serve: 90,
  start: 70,
  dev: 60,
  develop: 50,
};
const GENERATED_FRAMEWORK_PATHS = [
  'dist',
  'build',
  'out',
  '.next',
  '.svelte-kit',
  '.astro',
  '.docusaurus',
  '.cache',
  'public/build',
  'public/_gatsby',
  'public/page-data',
];
const NEXT_JS_CONFIG_FILES = [
  'next.config.js',
  'next.config.mjs',
  'next.config.cjs',
];
const NEXT_TS_CONFIG_FILE = 'next.config.ts';
const SERVER_READY_TIMEOUT_MS = 45 * 1000;
const HTTP_REQUEST_TIMEOUT_MS = 5 * 1000;
const PROCESS_SHUTDOWN_TIMEOUT_MS = 5 * 1000;
const HTTP_POLL_INTERVAL_MS = 500;
const MAX_HTTP_REDIRECTS = 5;
const MAX_RESPONSE_BODY_BYTES = 64 * 1024;
const PORT_BASE = Number(process.env.FRAMEWORK_TEST_PORT_BASE || '4300');
const PORT_BLOCK_SIZE = Number(process.env.FRAMEWORK_TEST_PORT_BLOCK_SIZE || '10');
const EDGE_STAGE_SKIP_PROJECTS = parseEdgeStageSkipProjects();
const NODE_STAGE_SKIP_PROJECTS = parseNodeStageSkipProjects();
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

main().catch((error) => {
  const message = error && error.message ? error.message : String(error);
  process.stderr.write(`${formatPrefix('ERROR')} ${message}${os.EOL}`);
  process.exit(1);
});

async function main() {
  const args = process.argv.slice(2);
  const command = args[0];
  const selector = parseSelector(args.slice(1));

  if (command !== 'setup' && command !== 'test' && command !== 'reset') {
    fail([
      'usage: framework-test.js <setup|test|reset> [js-framework-name]',
      'example: framework-test.js test js-next-staticsite',
    ].join('\n'));
  }

  if (command === 'reset') {
    reset(selector);
    return;
  }

  if (command === 'setup') {
    await setup(selector);
    return;
  }

  await test(selector);
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

async function test(selector) {
  const prepared = await setup(selector);
  const nodeRunner = HOST_NODE_RUNNER;
  const stages = harness.buildRunnerStages(nodeRunner, prepared.runner);

  printSection('Framework Matrix', 'blue', `${prepared.projects.length} framework${prepared.projects.length === 1 ? '' : 's'}`);

  const stageResults = [];
  let previousResult = null;
  for (const stage of stages) {
    const selectedProjects = stage.selectProjects(prepared.projects, previousResult);
    const skippedProjects = stage.skippedProjects
      ? stage.skippedProjects(prepared.projects, selectedProjects, previousResult)
      : [];
    const result = await harness.runRunnerStage(stage, selectedProjects, skippedProjects, {
      prepareProject: prepareProjectForStage,
      testProject,
    });
    stageResults.push(result);
    previousResult = result;
  }

  harness.printMatrixSummary(stageResults, prepared.projects);

  if (stageResults.some((result) => result.failed.length > 0)) {
    fail('framework runtime validation failed');
  }

  logSuccess('framework runtime validation passed across all configured runner stages');
}

function resolveHostNodeRunner() {
  return harness.resolveHostNodeRunner();
}

const HOST_NODE_RUNNER = resolveHostNodeRunner();

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

  const comparisonStage = createDependentRunnerStage({
    color: 'magenta',
    key: comparisonKey,
    label: comparisonLabel,
    runnerCommandParts: [comparisonRunner.targetPath],
  });
  stages.push(comparisonStage);

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

function createIndependentRunnerStage(options) {
  return {
    allowsProductionFallback: false,
    color: options.color,
    key: options.key,
    label: options.label,
    runnerCommandParts: options.runnerCommandParts.slice(),
    runnerDisplay: buildRunnerDisplay(options.runnerCommandParts),
    selectProjects(projects) {
      return projects.slice();
    },
    skippedProjects() {
      return [];
    },
  };
}

function createDependentRunnerStage(options) {
  const runnerCommandParts = options.runnerCommandParts.slice();

  return {
    allowsProductionFallback: true,
    color: options.color,
    key: options.key,
    label: options.label,
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

async function runRunnerStage(stage, projects, skippedProjects) {
  return harness.runRunnerStage(stage, projects, skippedProjects, {
    prepareProject: prepareProjectForStage,
    testProject,
  });
}

function prepareProjectForStage(project, stage) {
  log(`[${stage.label}] preparing ${project.name}`);
  const reuseExistingBuild = stage.key !== 'node' && hasGeneratedFrameworkArtifacts(project);
  if (reuseExistingBuild) {
    log(`reusing generated build artifacts for ${project.name} on ${stage.label}`);
  } else {
    removeGeneratedFrameworkArtifacts(project);
  }
  const injection = injectRunner(project, stage.runnerCommandParts);
  return {
    compatibleLaunchers: injection.compatibleLaunchers,
    reuseExistingBuild,
  };
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

async function setup(selector) {
  const totalSteps = 6;
  logProgress(1, totalSteps, 'starting framework Phase 1 setup');
  ensureDir(LOG_DIR);
  ensureDir(PNPM_STORE_DIR);
  log('state directories ready');

  logProgress(2, totalSteps, 'checking prerequisites');
  ensurePnpm();
  log('pnpm is available on PATH');

  const projects = discoverProjects(selector);
  log(`discovered ${projects.length} framework${projects.length === 1 ? '' : 's'}`);

  logProgress(3, totalSteps, 'resolving runner target');
  const runner = resolveRunnerTarget();

  log(`using runner target: ${runner.targetPath}`);
  if (selector) {
    log(`selected framework: ${projects[0].name}`);
  } else {
    log(`selected frameworks (${projects.length}): ${projects.map((project) => project.name).join(', ')}`);
  }

  logProgress(4, totalSteps, `installing dependencies for ${projects.length} framework${projects.length === 1 ? '' : 's'}`);
  await installProjects(projects);

  logProgress(5, totalSteps, 'injecting runner symlinks');
  const injected = [];
  for (let index = 0; index < projects.length; index += 1) {
    const project = projects[index];
    log(`injecting runner for ${project.name} (${index + 1}/${projects.length})`);
    injected.push(injectRunner(project, runner.targetPath));
  }

  logProgress(6, totalSteps, 'phase 1 setup complete');
  log(`phase 1 setup complete for ${projects.length} framework${projects.length === 1 ? '' : 's'}`);
  for (const result of injected) {
    log(`prepared ${result.project.name} via ${result.compatibleLaunchers.join(', ')}`);
  }

  return {
    injected,
    projects,
    runner,
  };
}

function ensurePnpm() {
  const check = spawnSync('pnpm', ['--version'], {
    cwd: ROOT_DIR,
    stdio: 'ignore',
  });

  if (check.error || check.status !== 0) {
    fail(`pnpm is required but was not found on PATH.\n${PNPM_HINT}`);
  }
}

function discoverProjects(selector) {
  if (!fs.existsSync(EXAMPLES_DIR)) {
    fail(`wasmer-examples is missing.\nRun: ${SUBMODULE_HINT}`);
  }

  const entries = fs.readdirSync(EXAMPLES_DIR, { withFileTypes: true });
  const projects = entries
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('js-'))
    .map((entry) => {
      const packageJson = path.join(EXAMPLES_DIR, entry.name, 'package.json');
      if (!fs.existsSync(packageJson)) {
        return null;
      }

      const manifest = readJsonFile(packageJson);
      return {
        dir: path.join(EXAMPLES_DIR, entry.name),
        manifest,
        name: entry.name,
        packageJson,
        scripts: manifest.scripts || {},
      };
    })
    .filter(Boolean)
    .sort((left, right) => left.name.localeCompare(right.name));

  // FRAMEWORK_TEST_EXCLUDE fully drops projects from discovery: they are never
  // built, and no runtime stage is attempted or reported. This is deliberately
  // stronger than FRAMEWORK_TEST_NODE_SKIP / FRAMEWORK_TEST_EDGE_SKIP (which
  // only skip a runtime stage but still build the app). We need the stronger
  // form for js-gatsby-* on macOS: the failure there is `gatsby build` itself
  // hanging on GitHub macOS runners (a gatsby-worker/sharp issue) — not EdgeJS
  // running the app — and the harness has no build-step timeout, so a stage
  // skip alone would still hang the job during the build. See the macOS job in
  // .github/workflows/test-and-build-quickjs.yml.
  const excluded = parseExcludedProjects();
  const discovered = excluded.size > 0
    ? projects.filter((project) => !excluded.has(project.name))
    : projects;
  for (const name of excluded) {
    if (projects.some((project) => project.name === name)) {
      log(`excluding ${name} from discovery (FRAMEWORK_TEST_EXCLUDE)`);
    }
  }

  if (discovered.length === 0) {
    fail(`wasmer-examples is not initialized or has no js-* packages.\nRun: ${SUBMODULE_HINT}`);
  }

  if (!selector) {
    return discovered;
  }

  const match = discovered.find((project) => project.name === selector);
  if (!match) {
    fail([
      `unknown framework selector: ${selector}`,
      `available frameworks: ${discovered.map((project) => project.name).join(', ')}`,
    ].join('\n'));
  }

  return [match];
}

function readJsonFile(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (error) {
    fail(`failed to parse JSON at ${filePath}: ${error.message}`);
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

function parseExcludedProjects() {
  const raw = process.env.FRAMEWORK_TEST_EXCLUDE;
  if (!raw || !raw.trim()) {
    return new Set();
  }

  return new Set(raw.split(',').map((entry) => entry.trim()).filter(Boolean));
}

function parseEdgeStageSkipProjects() {
  const raw = process.env.FRAMEWORK_TEST_EDGE_SKIP;
  if (!raw || !raw.trim()) {
    return new Set();
  }

  return new Set(raw.split(',').map((entry) => entry.trim()).filter(Boolean));
}

function parseNodeStageSkipProjects() {
  const raw = process.env.FRAMEWORK_TEST_NODE_SKIP;
  if (!raw || !raw.trim()) {
    return new Set();
  }

  return new Set(raw.split(',').map((entry) => entry.trim()).filter(Boolean));
}

function maybeSkipStageProject(project, stage) {
  if (stage.key === 'node' && NODE_STAGE_SKIP_PROJECTS.has(project.name)) {
    const error = new Error(`${project.name} skipped on ${stage.label}`);
    error.skip = true;
    error.detail = 'listed in FRAMEWORK_TEST_NODE_SKIP';
    return error;
  }

  if (isEdgeRuntimeStage(stage) && EDGE_STAGE_SKIP_PROJECTS.has(project.name)) {
    const error = new Error(`${project.name} skipped on ${stage.label}`);
    error.skip = true;
    error.detail = 'listed in FRAMEWORK_TEST_EDGE_SKIP';
    return error;
  }

  return null;
}

async function testProject(project, stage, index, total, preparation) {
  logProgress(index + 1, total, `[${stage.label}] testing ${project.name}`);

  const skipError = maybeSkipStageProject(project, stage);
  if (skipError) {
    throw skipError;
  }

  assertEdgeRuntimeCompatibleProject(project, stage);

  let runtime = detectRuntimeScript(project);
  runtime = resolveRuntimeStrategy(project, runtime);

  const portCandidates = buildPortCandidates(index);
  log(`port candidates for ${project.name}: ${portCandidates.join(', ')}`);

  const shouldBuild = shouldBuildProject(project, runtime, stage);
  const reuseExistingBuild = Boolean(preparation && preparation.reuseExistingBuild);

  if (shouldBuild && !reuseExistingBuild) {
    log(`building ${project.name} before runtime validation`);
    const buildResult = await runProjectBuild(project, stage);
    log(`build completed for ${project.name} (${formatDuration(buildResult.durationMs)})`);
  } else if (reuseExistingBuild) {
    log(`reusing existing build output for ${project.name} on ${stage.label}`);
  } else {
    log(`skipping build for ${project.name}; runtime script ${runtime.name} is development-oriented`);
  }

  if (isEdgeRuntimeStage(stage)) {
    runtime = resolveEdgeProductionRuntime(project, runtime, stage);
  } else {
    runtime = preferStaticRuntimeOnEdge(project, runtime, stage);
  }
  log(`selected runtime for ${project.name}: ${runtime.name} -> ${runtime.command}`);

  const routes = loadRouteMatrix(project, stage, runtime);
  if (routes.length === 0) {
    if (isEdgeRuntimeStage(stage)) {
      const error = new Error(`${project.name} has no routes for ${stage.label}`);
      error.skip = true;
      error.detail = 'routes.json limits this project to the Node.js baseline stage';
      throw error;
    }
    fail(`no routes configured for ${project.name} on ${stage.label}`);
  }

  let server = null;
  let activeRuntime = runtime;
  let usedProductionFallback = false;
  let readinessPath = routeReadinessPath(project, stage, activeRuntime);
  try {
    server = await startProjectServer(project, runtime, portCandidates, stage, readinessPath);
  } catch (error) {
    const fallbackRuntime = await maybePrepareProductionFallback(project, stage, runtime, shouldBuild, reuseExistingBuild, error);
    if (!fallbackRuntime) {
      throw error;
    }
    activeRuntime = fallbackRuntime;
    usedProductionFallback = true;
    readinessPath = routeReadinessPath(project, stage, activeRuntime);
    server = await startProjectServer(project, fallbackRuntime, portCandidates, stage, readinessPath);
  }
  try {
    const routeResults = await validateRouteMatrix(project, activeRuntime, server.port, routes);
    return {
      buildLogPath: shouldBuild && !reuseExistingBuild ? buildLogPath(project, stage) : null,
      candidate: server.candidate,
      port: server.port,
      project,
      response: server.response,
      routeResults,
      runtime: activeRuntime,
      serverLogPath: server.logPath,
      usedProductionFallback,
    };
  } finally {
    await stopProcess(server.handle);
  }
}

async function maybePrepareProductionFallback(project, stage, runtime, shouldBuild, reuseExistingBuild, startupError) {
  if (!stage.allowsProductionFallback) {
    return null;
  }
  if (typeof project.scripts.build !== 'string') {
    return null;
  }

  logWarn(`runtime startup failed for ${project.name} on ${stage.label}; falling back to production artifact serving`);
  if (startupError && startupError.message) {
    logWarn(`startup failure: ${startupError.message}`);
  }

  if (!reuseExistingBuild) {
    log(`building ${project.name} with Node.js for production fallback`);
    const buildResult = await runProjectBuild(project, {
      key: HOST_NODE_RUNNER.key,
      label: HOST_NODE_RUNNER.label,
      runnerCommandParts: [HOST_NODE_RUNNER.targetPath],
    });
    log(`Node.js fallback build completed for ${project.name} (${formatDuration(buildResult.durationMs)})`);
  } else if (shouldBuild) {
    log(`reusing existing production build for fallback on ${project.name}`);
  }

  const fallbackRuntime = resolveProductionFallbackRuntime(project, runtime);
  if (!fallbackRuntime) {
    return null;
  }

  return fallbackRuntime;
}

function isDevelopmentLikeRuntime(runtime) {
  if (!runtime || typeof runtime.command !== 'string') {
    return false;
  }

  const lower = runtime.command.toLowerCase();
  if (runtime.name === 'dev' || runtime.name === 'develop') {
    return true;
  }
  if (/\bdev\b/.test(lower) || /\bdevelop\b/.test(lower)) {
    return true;
  }
  if (/\bdocusaurus-start\b/.test(lower)) {
    return true;
  }
  if (/\bdocusaurus start\b/.test(lower) && !/\bdocusaurus serve\b/.test(lower)) {
    return true;
  }
  if (/\bgatsby develop\b/.test(lower)) {
    return true;
  }
  if (/\bnext dev\b/.test(lower)) {
    return true;
  }
  if (/\bvite dev\b/.test(lower)) {
    return true;
  }
  if (/\bastro dev\b/.test(lower)) {
    return true;
  }

  return false;
}

function staticRuntimeFor(project, outputDir) {
  return {
    command: `internal static server for ${path.basename(outputDir)}/`,
    mode: 'static-export',
    name: 'static',
    outputDir,
  };
}

function resolveEdgeProductionRuntime(project, runtime, stage) {
  if (runtime.mode === 'static-export') {
    return runtime;
  }

  if (isDevelopmentLikeRuntime(runtime) || shouldPreferStaticRuntimeOverScript(project, runtime)) {
    const outputDir = detectStaticProductionOutputDir(project);
    if (!outputDir) {
      fail([
        `${project.name} cannot run on ${stage.label}: EdgeJS runtime stages require prebuilt production artifacts`,
        `The selected script (${runtime.name}: ${runtime.command}) is development-oriented.`,
        'Ensure the project has a build script and that `pnpm run build` succeeds on host Node.js first.',
      ].join('\n'));
    }

    log(`using static production output for ${project.name} on ${stage.label} instead of ${runtime.name}`);
    return staticRuntimeFor(project, outputDir);
  }

  return runtime;
}

function resolveProductionFallbackRuntime(project, currentRuntime) {
  const nextPreRenderedDir = detectNextPrerenderedOutputDir(project);
  if (nextPreRenderedDir) {
    return {
      command: `internal static server for ${path.relative(project.dir, nextPreRenderedDir)}/`,
      mode: 'static-export',
      name: 'static',
      outputDir: nextPreRenderedDir,
    };
  }

  const outputDir = detectStaticProductionOutputDir(project);
  if (outputDir) {
    return {
      command: `internal static server for ${path.basename(outputDir)}/`,
      mode: 'static-export',
      name: 'static',
      outputDir,
    };
  }

  const productionScript = detectProductionRuntimeScript(project);
  if (productionScript && (!currentRuntime || productionScript.command.trim() !== currentRuntime.command.trim())) {
    const fallbackRuntime = resolveRuntimeStrategy(project, productionScript);
    if (fallbackRuntime.mode === 'package-script' && shouldPreferStaticRuntimeOverScript(project, fallbackRuntime)) {
      const staticDir = detectStaticProductionOutputDir(project);
      if (staticDir) {
        return {
          command: `internal static server for ${path.basename(staticDir)}/`,
          mode: 'static-export',
          name: 'static',
          outputDir: staticDir,
        };
      }
    }
    return fallbackRuntime;
  }

  return null;
}

function detectProductionRuntimeScript(project) {
  const entries = Object.entries(project.scripts)
    .filter(([name, command]) => typeof command === 'string' && name !== 'dev' && name !== 'develop')
    .map(([name, command]) => ({
      command,
      name,
      score: scoreProductionRuntimeScript(name, command),
    }))
    .filter((entry) => entry.score > 0)
    .sort((left, right) => right.score - left.score || left.name.localeCompare(right.name));

  return entries.length > 0 ? entries[0] : null;
}

function scoreProductionRuntimeScript(name, command) {
  const lower = command.toLowerCase();
  let score = 0;

  if (name === 'preview') {
    score += 100;
  } else if (name === 'serve') {
    score += 90;
  } else if (name === 'start') {
    score += 80;
  }

  if (/\bpreview\b/.test(lower)) {
    score += 40;
  }
  if (/\bserve\b/.test(lower)) {
    score += 30;
  }
  if (/\bstart\b/.test(lower)) {
    score += 20;
  }
  if (/\bdev\b|\bdevelop\b/.test(lower)) {
    score -= 200;
  }
  if (/\bdocusaurus-start\b/.test(lower)) {
    score -= 200;
  }
  if (/\bgatsby develop\b/.test(lower)) {
    score -= 200;
  }
  if (/\bnext dev\b/.test(lower)) {
    score -= 200;
  }
  if (/\bvite dev\b/.test(lower)) {
    score -= 200;
  }
  if (/\bastro dev\b/.test(lower)) {
    score -= 200;
  }
  if (/\$port\b|--port\b|(?:^|\s)-p(?:\s|$)|(?:^|\s)-l(?:\s|$)/.test(lower)) {
    score += 10;
  }
  if (/--host\b|--hostname\b|\$host\b/.test(lower)) {
    score += 5;
  }

  return score;
}

function detectNextPrerenderedOutputDir(project) {
  const candidate = path.join(project.dir, '.next', 'server', 'app');
  if (!fs.existsSync(candidate) || !fs.statSync(candidate).isDirectory()) {
    return null;
  }
  if (!fs.existsSync(path.join(candidate, 'index.html'))) {
    return null;
  }
  return candidate;
}

function detectStaticOutputDir(project) {
  for (const candidate of ['out', 'dist']) {
    const target = path.join(project.dir, candidate);
    if (fs.existsSync(target) && fs.statSync(target).isDirectory()) {
      return target;
    }
  }

  const buildDir = path.join(project.dir, 'build');
  if (fs.existsSync(buildDir)
    && fs.statSync(buildDir).isDirectory()
    && fs.existsSync(path.join(buildDir, 'index.html'))) {
    return buildDir;
  }

  return null;
}

function detectStaticProductionOutputDir(project) {
  const gatsbyDir = detectGatsbyBuildOutputDir(project);
  if (gatsbyDir) {
    return gatsbyDir;
  }

  const docusaurusV1Dir = detectDocusaurusV1BuildOutputDir(project);
  if (docusaurusV1Dir) {
    return docusaurusV1Dir;
  }

  return detectStaticOutputDir(project);
}

function projectUsesDocusaurusV1(project) {
  const deps = {
    ...(project.manifest.dependencies || {}),
    ...(project.manifest.devDependencies || {}),
    ...(project.manifest.peerDependencies || {}),
  };

  return Object.prototype.hasOwnProperty.call(deps, 'docusaurus')
    && !Object.prototype.hasOwnProperty.call(deps, '@docusaurus/core');
}

function detectDocusaurusV1BuildOutputDir(project) {
  if (!projectUsesDocusaurusV1(project)) {
    return null;
  }

  const buildRoot = path.join(project.dir, 'build');
  if (!fs.existsSync(buildRoot) || !fs.statSync(buildRoot).isDirectory()) {
    return null;
  }

  const siteConfigPath = path.join(project.dir, 'siteConfig.js');
  if (fs.existsSync(siteConfigPath)) {
    try {
      const siteConfig = require(siteConfigPath);
      if (siteConfig && typeof siteConfig.projectName === 'string') {
        const candidate = path.join(buildRoot, siteConfig.projectName);
        if (fs.existsSync(path.join(candidate, 'index.html'))) {
          return candidate;
        }
      }
    } catch (error) {
      logWarn(`unable to read ${siteConfigPath} while locating Docusaurus v1 output: ${error.message}`);
    }
  }

  for (const entry of fs.readdirSync(buildRoot)) {
    const candidate = path.join(buildRoot, entry);
    if (!fs.statSync(candidate).isDirectory()) {
      continue;
    }
    if (fs.existsSync(path.join(candidate, 'index.html'))) {
      return candidate;
    }
  }

  return null;
}

function projectUsesGatsby(project) {
  const deps = {
    ...(project.manifest.dependencies || {}),
    ...(project.manifest.devDependencies || {}),
    ...(project.manifest.peerDependencies || {}),
  };

  return Object.prototype.hasOwnProperty.call(deps, 'gatsby');
}

function detectGatsbyBuildOutputDir(project) {
  if (!projectUsesGatsby(project)) {
    return null;
  }

  const publicDir = path.join(project.dir, 'public');
  if (!fs.existsSync(publicDir) || !fs.statSync(publicDir).isDirectory()) {
    return null;
  }

  if (!fs.existsSync(path.join(publicDir, 'index.html'))) {
    return null;
  }

  const hasGatsbyBuildMarkers = fs.existsSync(path.join(publicDir, 'page-data'))
    || fs.existsSync(path.join(publicDir, 'chunk-map.json'))
    || fs.readdirSync(publicDir).some((entry) => /^webpack-runtime-/.test(entry));

  return hasGatsbyBuildMarkers ? publicDir : null;
}

function shouldPreferStaticRuntimeOverScript(project, runtime) {
  if (!runtime || typeof runtime.command !== 'string') {
    return false;
  }

  const lower = runtime.command.toLowerCase();
  if (/\bpreview\b/.test(lower) || /\bvite\b/.test(lower)) {
    return true;
  }

  if (/\bgatsby serve\b/.test(lower)) {
    return true;
  }

  if (/\bdocusaurus serve\b/.test(lower)) {
    return true;
  }

  return false;
}

function preferStaticRuntimeOnEdge(project, runtime, stage) {
  if (!isEdgeRuntimeStage(stage) || runtime.mode === 'static-export') {
    return runtime;
  }

  if (!shouldPreferStaticRuntimeOverScript(project, runtime)) {
    return runtime;
  }

  const outputDir = detectStaticProductionOutputDir(project);
  if (!outputDir) {
    return runtime;
  }

  log(`using static production output for ${project.name} on ${stage.label} instead of ${runtime.name}`);
  return staticRuntimeFor(project, outputDir);
}

function resolveRuntimeStrategy(project, runtime) {
  if (runtime.command.trim() === 'next start' && usesNextStaticExport(project)) {
    return {
      command: 'internal static server for out/',
      mode: 'static-export',
      name: 'export',
      outputDir: path.join(project.dir, 'out'),
    };
  }

  return {
    ...runtime,
    mode: 'package-script',
  };
}

function isEdgeRuntimeStage(stage) {
  return stage.key !== HOST_NODE_RUNNER.key;
}

function nodeBuildStage() {
  return {
    key: 'node-build',
    label: 'Node.js build',
    runnerCommandParts: [HOST_NODE_RUNNER.targetPath],
  };
}

function projectUsesNext(project) {
  const deps = {
    ...(project.manifest.dependencies || {}),
    ...(project.manifest.devDependencies || {}),
    ...(project.manifest.peerDependencies || {}),
  };

  return Object.prototype.hasOwnProperty.call(deps, 'next');
}

function hasShippableNextConfig(projectDir) {
  return NEXT_JS_CONFIG_FILES.some((name) => fs.existsSync(path.join(projectDir, name)));
}

function hasTypescriptOnlyNextConfig(projectDir) {
  return fs.existsSync(path.join(projectDir, NEXT_TS_CONFIG_FILE))
    && !hasShippableNextConfig(projectDir);
}

function assertEdgeRuntimeCompatibleProject(project, stage) {
  if (!isEdgeRuntimeStage(stage) || !projectUsesNext(project)) {
    return;
  }

  if (!hasTypescriptOnlyNextConfig(project.dir)) {
    return;
  }

  fail([
    `${project.name} is not ready for ${stage.label}: next.config.ts requires SWC at runtime`,
    'Build with host Node.js (SWC runs during next build), but EdgeJS runtime stages need a JavaScript config.',
    'Add next.config.js/.mjs/.cjs or remove next.config.ts before testing EdgeJS runtime stages.',
  ].join('\n'));
}

function usesNextStaticExport(project) {
  const nextConfigCandidates = NEXT_JS_CONFIG_FILES.map((name) => path.join(project.dir, name));

  for (const configPath of nextConfigCandidates) {
    if (!fs.existsSync(configPath)) {
      continue;
    }

    const config = fs.readFileSync(configPath, 'utf8');
    if (/output\s*:\s*['"]export['"]/.test(config)) {
      return true;
    }
  }

  return false;
}

function detectRuntimeScript(project) {
  const entries = Object.entries(project.scripts)
    .filter(([name, command]) => typeof command === 'string' && Object.prototype.hasOwnProperty.call(RUNTIME_SCRIPT_PRIORITY, name))
    .map(([name, command]) => ({
      command,
      name,
      score: scoreRuntimeScript(name, command),
    }))
    .sort((left, right) => right.score - left.score || left.name.localeCompare(right.name));

  if (entries.length === 0) {
    fail(`no runtime script was found for ${project.name}; expected one of ${Object.keys(RUNTIME_SCRIPT_PRIORITY).join(', ')}`);
  }

  return entries[0];
}

function scoreRuntimeScript(name, command) {
  const lower = command.toLowerCase();
  let score = RUNTIME_SCRIPT_PRIORITY[name] || 0;

  if (/\bpreview\b/.test(lower)) {
    score += 40;
  }
  if (/\bserve\b/.test(lower)) {
    score += 30;
  }
  if (/\bdev\b/.test(lower) || /\bdevelop\b/.test(lower)) {
    score -= 40;
  }
  if (/\$port\b|--port\b|(?:^|\s)-p(?:\s|$)|(?:^|\s)-l(?:\s|$)/.test(lower)) {
    score += 20;
  }
  if (/--host\b|--hostname\b|\$host\b/.test(lower)) {
    score += 10;
  }

  return score;
}

function shouldBuildProject(project, runtime, stage) {
  if (typeof project.scripts.build !== 'string') {
    return false;
  }

  if (isEdgeRuntimeStage(stage)) {
    return !detectStaticProductionOutputDir(project);
  }

  if (runtime.name === 'dev' || runtime.name === 'develop') {
    return false;
  }

  const lower = runtime.command.toLowerCase();
  if (/\bdev\b/.test(lower) || /\bdevelop\b/.test(lower)) {
    return false;
  }

  return true;
}

async function runProjectBuild(project, stage) {
  const buildStage = isEdgeRuntimeStage(stage) ? nodeBuildStage() : stage;
  if (isEdgeRuntimeStage(stage)) {
    log(`building ${project.name} with host Node.js; ${stage.label} validates prebuilt artifacts only`);
  }

  const logPath = buildLogPath(project, buildStage);
  removeFileOrSymlink(logPath);

  return runProjectCommand({
    description: `build for ${project.name}`,
    detached: false,
    env: makeProjectEnv(),
    errorMessage: `build failed for ${project.name} on ${buildStage.label}`,
    extraArgs: [],
    logPath,
    project,
    scriptName: 'build',
  });
}

async function startProjectServer(project, runtime, portCandidates, stage, readinessPath) {
  const readyPath = normalizeRoutePath(readinessPath);
  if (runtime.mode === 'static-export') {
    return startStaticExportServer(project, runtime, portCandidates, stage, readyPath);
  }

  const logPath = serverLogPath(project, stage);
  removeFileOrSymlink(logPath);

  let lastError = null;
  let attempt = 0;
  for (const port of portCandidates) {
    const candidates = buildRuntimeCandidates(runtime, port);
    let shouldTryAnotherPort = false;
    for (let index = 0; index < candidates.length; index += 1) {
      attempt += 1;
      const candidate = candidates[index];
      log(`starting ${project.name} on ${DEFAULT_HOST}:${port} with ${candidate.description} (attempt ${attempt})`);

      const handle = spawnLoggedProcess({
        append: attempt > 1,
        description: `runtime ${runtime.name} for ${project.name} on ${DEFAULT_HOST}:${port} using ${candidate.description}`,
        commandDisplay: buildRuntimeShellCommand(project, runtime, candidate.extraArgs),
        detached: true,
        env: makeProjectEnv(port),
        logPath,
        project,
        shellCommand: buildRuntimeShellCommand(project, runtime, candidate.extraArgs),
      });

      try {
        const response = await waitForHttpResponse(handle, buildRouteUrl(port, readyPath));
        return {
          candidate,
          handle,
          logPath,
          port,
          response,
        };
      } catch (error) {
        lastError = error;
        await stopProcess(handle);
        shouldTryAnotherPort = shouldTryAnotherPort || shouldRetryWithAnotherPort(error, logPath);
        logWarn(`start attempt failed for ${project.name} on ${DEFAULT_HOST}:${port}: ${error.message}`);
      }
    }

    if (!shouldTryAnotherPort && lastError) {
      logWarn(`stopping additional port retries for ${project.name} on ${stage.label}; the startup failure is not port-related`);
      break;
    }
  }

  fail(`unable to start ${project.name} via pnpm run ${runtime.name} on ${stage.label}; tried ports ${portCandidates.join(', ')}`, {
    detail: summarizeLogFailure(logPath, lastError),
    logPath,
  });
}

async function startStaticExportServer(project, runtime, portCandidates, stage, readinessPath) {
  if (!fs.existsSync(runtime.outputDir)) {
    fail(`expected static output directory for ${project.name}: ${runtime.outputDir}`);
  }

  ensureStaticServerScript(project);
  const logPath = serverLogPath(project, stage);
  removeFileOrSymlink(logPath);

  let lastError = null;
  for (let index = 0; index < portCandidates.length; index += 1) {
    const port = portCandidates[index];
    const commandDisplay = buildStaticServerCommand(project, stage.runnerCommandParts, runtime.outputDir, port);
    log(`starting ${project.name} on ${DEFAULT_HOST}:${port} with static export fallback (attempt ${index + 1})`);

    const handle = spawnLoggedProcess({
      append: index > 0,
      commandDisplay,
      description: `static export server for ${project.name} on ${DEFAULT_HOST}:${port}`,
      detached: true,
      env: makeProjectEnv(port),
      logPath,
      project,
      shellCommand: commandDisplay,
    });

    try {
      const response = await waitForHttpResponse(handle, buildRouteUrl(port, readinessPath || '/'));
      return {
        candidate: {
          description: 'static export fallback',
          extraArgs: [],
          signature: 'static-export',
        },
        handle,
        logPath,
        port,
        response,
      };
    } catch (error) {
      lastError = error;
      await stopProcess(handle);
      logWarn(`static export fallback failed for ${project.name} on ${DEFAULT_HOST}:${port}: ${error.message}`);
      if (!shouldRetryWithAnotherPort(error, logPath)) {
        logWarn(`stopping additional port retries for ${project.name} on ${stage.label}; the startup failure is not port-related`);
        break;
      }
    }
  }

  fail(`unable to start static export fallback for ${project.name} on ${stage.label}; tried ports ${portCandidates.join(', ')}`, {
    detail: summarizeLogFailure(logPath, lastError),
    logPath,
  });
}

function buildRuntimeCandidates(runtime, port) {
  const lower = runtime.command.toLowerCase();
  const candidates = [];

  if (/\bdocusaurus-start\b/.test(lower) && /\$port\b/.test(lower)) {
    pushRuntimeCandidate(candidates, [], 'HOST/PORT environment only');
    pushRuntimeCandidate(candidates, ['--host', DEFAULT_HOST], 'host override');
    return candidates;
  }

  if (/(?:^|\s)serve(?:\s|$)/.test(lower) && /(?:^|\s)-l(?:\s|$)/.test(lower)) {
    pushRuntimeCandidate(candidates, ['-l', String(port)], 'serve listen port override');
    pushRuntimeCandidate(candidates, ['-l', `tcp://${DEFAULT_HOST}:${port}`], 'serve tcp listen override');
    return candidates;
  }

  if (/\bvite\b/.test(lower)) {
    pushRuntimeCandidate(candidates, ['--host', DEFAULT_HOST, '--port', String(port), '--strictPort'], 'Vite host/port override');
    pushRuntimeCandidate(candidates, ['--port', String(port), '--strictPort'], 'Vite port override');
    return candidates;
  }

  if (/\bnext\b/.test(lower)) {
    pushRuntimeCandidate(candidates, ['--hostname', DEFAULT_HOST, '--port', String(port)], 'Next hostname/port override');
    pushRuntimeCandidate(candidates, ['--port', String(port)], 'Next port override');
    return candidates;
  }

  pushRuntimeCandidate(candidates, ['--host', DEFAULT_HOST, '--port', String(port)], 'host/port override');
  pushRuntimeCandidate(candidates, ['--port', String(port), '--host', DEFAULT_HOST], 'port/host override');
  pushRuntimeCandidate(candidates, ['--hostname', DEFAULT_HOST, '--port', String(port)], 'hostname/port override');
  pushRuntimeCandidate(candidates, ['--port', String(port)], 'port override');
  pushRuntimeCandidate(candidates, [], 'HOST/PORT environment only');

  return candidates;
}

function pushRuntimeCandidate(candidates, extraArgs, description) {
  const signature = extraArgs.join('\0');
  if (candidates.some((candidate) => candidate.signature === signature)) {
    return;
  }

  candidates.push({
    description,
    extraArgs,
    signature,
  });
}

function buildLogPath(project, stage) {
  return path.join(LOG_DIR, `${project.name}.${stage.key}.build.log`);
}

function serverLogPath(project, stage) {
  return path.join(LOG_DIR, `${project.name}.${stage.key}.server.log`);
}

function ensureStaticServerScript(project) {
  const scriptPath = staticServerScriptPath(project);
  if (fs.existsSync(scriptPath)) {
    const existing = fs.readFileSync(scriptPath, 'utf8');
    if (existing.includes(STATIC_SERVER_RESOLVER_MARKER)) {
      return scriptPath;
    }
  }

  fs.writeFileSync(scriptPath, [
    "'use strict';",
    '',
    `// ${STATIC_SERVER_RESOLVER_MARKER}`,
    '',
    "const fs = require('node:fs');",
    "const http = require('node:http');",
    "const path = require('node:path');",
    '',
    "const root = path.resolve(process.argv[2]);",
    "const port = Number(process.argv[3] || process.env.PORT || '3000');",
    "const host = process.argv[4] || process.env.HOST || '127.0.0.1';",
    '',
    'const MIME_TYPES = {',
    "  '.css': 'text/css; charset=utf-8',",
    "  '.html': 'text/html; charset=utf-8',",
    "  '.js': 'application/javascript; charset=utf-8',",
    "  '.json': 'application/json; charset=utf-8',",
    "  '.svg': 'image/svg+xml',",
    "  '.txt': 'text/plain; charset=utf-8',",
    "  '.wasm': 'application/wasm',",
    "  '.xml': 'application/xml; charset=utf-8',",
    '};',
    '',
    'function isSafePath(candidate) {',
    '  const relative = path.relative(root, candidate);',
    "  return !relative.startsWith('..') && !path.isAbsolute(relative);",
    '}',
    '',
    'function resolveFilePath(urlPath) {',
    "  let pathname = decodeURIComponent(urlPath.split('?')[0] || '/');",
    '  const candidates = [];',
    "  if (pathname.endsWith('/')) {",
    "    candidates.push(path.resolve(root, `.${pathname}index.html`));",
    '  } else {',
    "    candidates.push(path.resolve(root, `.${pathname}`));",
    "    candidates.push(path.resolve(root, `.${pathname}.html`));",
    "    candidates.push(path.resolve(root, `.${pathname}/index.html`));",
    '  }',
    '',
    '  for (const candidate of candidates) {',
    '    if (!isSafePath(candidate)) {',
    '      continue;',
    '    }',
    '    if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {',
    '      return candidate;',
    '    }',
    '  }',
    '',
    '  return null;',
    '}',
    '',
    'const server = http.createServer((request, response) => {',
    "  if (request.method !== 'GET' && request.method !== 'HEAD') {",
    "    response.statusCode = 405;",
    "    response.end('Method Not Allowed');",
    '    return;',
    '  }',
    '',
    '  const filePath = resolveFilePath(request.url || "/");',
    '  if (!filePath) {',
    "    response.statusCode = 404;",
    "    response.end('Not Found');",
    '    return;',
    '  }',
    '',
    '  const extension = path.extname(filePath).toLowerCase();',
    "  response.setHeader('Content-Type', MIME_TYPES[extension] || 'application/octet-stream');",
    '  response.statusCode = 200;',
    "  if (request.method === 'HEAD') {",
    '    response.end();',
    '    return;',
    '  }',
    '',
    '  fs.createReadStream(filePath).pipe(response);',
    '});',
    '',
    'server.listen(port, host, () => {',
    "  process.stdout.write(`static export server listening on http://${host}:${port}\\n`);",
    '});',
  ].join('\n'));
  return scriptPath;
}

function buildStaticServerCommand(project, runnerCommandParts, outputDir, port) {
  const scriptPath = toProjectRelativePath(project.dir, staticServerScriptPath(project));
  const relativeOutputDir = toProjectRelativePath(project.dir, outputDir);
  const commandParts = normalizeRunnerCommandParts(runnerCommandParts)
    .concat([scriptPath, relativeOutputDir, String(port), DEFAULT_HOST]);
  return `exec ${commandParts.map(shellQuote).join(' ')}`;
}

function staticServerScriptPath(project) {
  return path.join(project.dir, STATIC_SERVER_SCRIPT_BASENAME);
}

function toProjectRelativePath(projectDir, targetPath) {
  const relativePath = path.relative(projectDir, targetPath);
  if (!relativePath || relativePath === '') {
    return '.';
  }

  return relativePath.startsWith('.') ? relativePath : `./${relativePath}`;
}

function makeProjectEnv(port) {
  const env = {
    ...process.env,
    BROWSER: 'none',
    CI: '1',
    HOST: DEFAULT_HOST,
  };

  if (typeof port === 'number') {
    env.PORT = String(port);
  }

  return env;
}

async function runProjectCommand(options) {
  const handle = spawnLoggedProcess(options);
  const result = await handle.exitPromise;
  if (!result.ok) {
    fail(options.errorMessage, {
      detail: summarizeLogFailure(result.logPath),
      logPath: result.logPath,
    });
  }
  return result;
}

function spawnLoggedProcess(options) {
  const {
    append = false,
    commandDisplay,
    description,
    detached = false,
    env,
    extraArgs,
    logPath,
    project,
    shellCommand,
    scriptName,
  } = options;
  ensureDir(path.dirname(logPath));

  const logStream = fs.createWriteStream(logPath, { flags: append ? 'a' : 'w' });
  const commandArgs = ['run', scriptName].concat(extraArgs);
  const handle = {
    child: null,
    errorMessage: null,
    exitCode: null,
    exitPromise: null,
    exited: false,
    logPath,
    ok: false,
    signal: null,
    startedAt: Date.now(),
  };

  logStream.write(`${formatPrefix('INFO')} ${description}${os.EOL}`);
  logStream.write(`${formatPrefix('INFO')} command: ${commandDisplay || `pnpm ${commandArgs.join(' ')}`}${os.EOL}`);
  logStream.write(`${formatPrefix('INFO')} cwd: ${project.dir}${os.EOL}`);
  if (env.HOST) {
    logStream.write(`${formatPrefix('INFO')} host: ${env.HOST}${os.EOL}`);
  }
  if (env.PORT) {
    logStream.write(`${formatPrefix('INFO')} port: ${env.PORT}${os.EOL}`);
  }

  const child = shellCommand
    ? spawn('/bin/sh', ['-lc', shellCommand], {
      cwd: project.dir,
      detached,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    : spawn('pnpm', commandArgs, {
      cwd: project.dir,
      detached,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  handle.child = child;

  let settled = false;
  handle.exitPromise = new Promise((resolve) => {
    const finish = (fields) => {
      if (settled) {
        return;
      }
      settled = true;
      Object.assign(handle, {
        durationMs: Date.now() - handle.startedAt,
        ...fields,
      });
      logStream.end(() => resolve(handle));
    };

    child.stdout.on('data', (chunk) => {
      logStream.write(chunk);
    });
    child.stderr.on('data', (chunk) => {
      logStream.write(chunk);
    });

    child.on('error', (error) => {
      logStream.write(`${formatPrefix('ERROR')} spawn error: ${error.message}${os.EOL}`);
      finish({
        errorMessage: error.message,
        exited: true,
        ok: false,
      });
    });

    child.on('close', (code, signal) => {
      if (signal) {
        logStream.write(`${formatPrefix('WARN')} signal: ${signal}${os.EOL}`);
      }
      logStream.write(`${formatPrefix(code === 0 ? 'INFO' : 'ERROR')} exit: ${code}${os.EOL}`);
      finish({
        exitCode: code,
        exited: true,
        ok: code === 0,
        signal: signal || null,
      });
    });
  });

  return handle;
}

function buildRuntimeShellCommand(project, runtime, extraArgs) {
  const binDir = path.join(project.dir, 'node_modules', '.bin');
  const suffix = extraArgs.length > 0 ? ` ${extraArgs.map(shellQuote).join(' ')}` : '';
  return `PATH=${shellQuote(binDir)}:$PATH; export PATH; exec ${runtime.command}${suffix}`;
}

function shellQuote(value) {
  return `'${String(value).replace(/'/g, `'\"'\"'`)}'`;
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

function requestHttp(url, options) {
  const requestOptions = normalizeRequestHttpOptions(options);

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
      Accept: requestOptions.accept,
      ...requestOptions.headers,
    };
    const body = requestOptions.body;
    if (body != null && !headers['content-type'] && !headers['Content-Type']) {
      headers['Content-Type'] = requestOptions.contentType;
    }

    const request = client.request(requestUrl, {
      headers,
      method: requestOptions.method,
    }, (response) => {
      const responseHeaders = normalizeHeaders(response.headers);
      const statusCode = response.statusCode || 0;
      const location = responseHeaders.location;

      if (statusCode >= 300 && statusCode < 400 && location && requestOptions.redirectCount < MAX_HTTP_REDIRECTS) {
        response.resume();
        resolve(requestHttp(new URL(location, requestUrl).toString(), {
          ...requestOptions,
          redirectCount: requestOptions.redirectCount + 1,
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

function normalizeRequestHttpOptions(options) {
  if (typeof options === 'number') {
    return {
      accept: 'text/html,*/*',
      body: null,
      contentType: 'text/plain; charset=utf-8',
      headers: {},
      method: 'GET',
      redirectCount: options,
    };
  }

  const requestOptions = options && typeof options === 'object' ? options : {};
  const contentTypeMode = requestOptions.contentType || 'html';
  return {
    accept: requestOptions.accept || defaultAcceptHeader(contentTypeMode),
    body: serializeRequestBody(requestOptions.body),
    contentType: requestOptions.requestContentType || 'application/json; charset=utf-8',
    headers: requestOptions.headers || {},
    method: (requestOptions.method || 'GET').toUpperCase(),
    redirectCount: typeof requestOptions.redirectCount === 'number' ? requestOptions.redirectCount : 0,
  };
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

function validateHttpResponse(project, runtime, response) {
  validateRouteResponse({
    name: 'home',
    path: '/',
    expect: {
      contentType: 'html',
    },
  }, response, project, runtime);
}

function routesJsonPath(project) {
  return path.join(project.dir, ROUTES_JSON_BASENAME);
}

function routeReadinessPath(project, stage, runtime) {
  const routes = loadRouteMatrix(project, stage, runtime);
  if (routes.length === 0) {
    return '/';
  }
  return routes[0].path;
}

function loadRouteMatrix(project, stage, runtime) {
  const configPath = routesJsonPath(project);
  const config = fs.existsSync(configPath)
    ? readRouteMatrixConfig(configPath)
    : DEFAULT_ROUTE_MATRIX;

  return config.routes
    .map((route, index) => normalizeRouteDefinition(route, index, configPath))
    .filter((route) => routeAppliesToStage(route, stage, runtime));
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

function normalizeRoutePath(routePath) {
  if (routePath === '/') {
    return '/';
  }
  return routePath.replace(/\/+$/, '') || '/';
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

function buildRouteUrl(port, routePath) {
  return `http://${DEFAULT_HOST}:${port}${normalizeRoutePath(routePath)}`;
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
    .filter((line) => !line.startsWith('[20') || !line.includes('[framework-test] [INFO]'))
    .filter((line) => !line.startsWith('[20') || !line.includes('[framework-test] [WARN]'))
    .filter((line) => !line.startsWith('[20') || !line.includes('[framework-test] [ERROR] exit:'));

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

function shouldRetryWithAnotherPort(error, logPath) {
  const detail = describeStartupFailure(error, logPath).toLowerCase();
  const retryablePatterns = [
    /\btimed out waiting\b/,
    /\brequest timed out\b/,
    /\beaddrinuse\b/,
    /\beaddrnotavail\b/,
    /\beacces\b/,
    /\balready in use\b/,
    /\blisten e[a-z]+\b/,
    /\beconnrefused\b/,
    /\beconnreset\b/,
    /\bsocket hang up\b/,
  ];

  return retryablePatterns.some((pattern) => pattern.test(detail));
}

function describeStartupFailure(error, logPath) {
  if (error && typeof error.detail === 'string' && error.detail) {
    return error.detail;
  }

  return summarizeLogFailure(logPath, error);
}

function buildPortCandidates(index) {
  const startPort = PORT_BASE + (index * PORT_BLOCK_SIZE);
  return Array.from({ length: PORT_BLOCK_SIZE }, (_, offset) => startPort + offset);
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

function normalizeRunnerCommandParts(runnerTarget) {
  if (Array.isArray(runnerTarget)) {
    return runnerTarget.slice();
  }

  if (typeof runnerTarget === 'string' && runnerTarget) {
    return [runnerTarget];
  }

  fail(`invalid runner target: ${runnerTarget == null ? 'missing' : String(runnerTarget)}`);
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

function buildRunnerShimScript(runnerCommandParts) {
  return [
    '#!/bin/sh',
    `exec ${runnerCommandParts.map(shellQuote).join(' ')} "$@"`,
    '',
  ].join('\n');
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

function hasGeneratedFrameworkArtifacts(project) {
  return GENERATED_FRAMEWORK_PATHS
    .map((relativePath) => path.join(project.dir, relativePath))
    .some((targetPath) => fs.existsSync(targetPath));
}

function reset(selector) {
  const totalSteps = 3;
  logProgress(1, totalSteps, 'resetting framework-test generated state');

  const projects = safeDiscoverProjects(selector);
  log(`selected ${projects.length} framework${projects.length === 1 ? '' : 's'} for reset`);

  logProgress(2, totalSteps, 'removing generated framework state');
  for (const project of projects) {
    const nodeModulesPath = path.join(project.dir, 'node_modules');
    if (fs.existsSync(nodeModulesPath)) {
      log(`removing ${nodeModulesPath}`);
      fs.rmSync(nodeModulesPath, { recursive: true, force: true });
    }

    removeGeneratedFrameworkArtifacts(project);
  }

  if (fs.existsSync(STATE_DIR)) {
    log(`removing ${STATE_DIR}`);
    fs.rmSync(STATE_DIR, { recursive: true, force: true });
  }

  const buildDir = path.join(ROOT_DIR, 'build-edge');
  if (fs.existsSync(buildDir)) {
    log(`removing ${buildDir}`);
    fs.rmSync(buildDir, { recursive: true, force: true });
  }

  const rootStoreDir = path.join(ROOT_DIR, '.pnpm-store');
  if (fs.existsSync(rootStoreDir)) {
    log(`removing ${rootStoreDir}`);
    fs.rmSync(rootStoreDir, { recursive: true, force: true });
  }

  logProgress(3, totalSteps, 'framework-test reset complete');
  log('framework-test reset complete');
}

function removeGeneratedFrameworkArtifacts(project) {
  for (const basename of [STATIC_SERVER_SCRIPT_BASENAME, LEGACY_STATIC_SERVER_SCRIPT_BASENAME]) {
    const staticServerPath = path.join(project.dir, basename);
    if (fs.existsSync(staticServerPath)) {
      log(`removing ${staticServerPath}`);
      fs.rmSync(staticServerPath, { force: true });
    }
  }

  for (const relativePath of GENERATED_FRAMEWORK_PATHS) {
    const targetPath = path.join(project.dir, relativePath);
    if (!fs.existsSync(targetPath)) {
      continue;
    }

    log(`removing ${targetPath}`);
    fs.rmSync(targetPath, { recursive: true, force: true });
  }

  maybeRemoveUntrackedPublicDir(project);
}

function maybeRemoveUntrackedPublicDir(project) {
  const publicDir = path.join(project.dir, 'public');
  if (!fs.existsSync(publicDir)) {
    return;
  }

  const tracked = spawnSync('git', ['-C', EXAMPLES_DIR, 'ls-files', '--', `${project.name}/public`], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  });

  if (tracked.error || tracked.status !== 0) {
    return;
  }

  if (tracked.stdout.trim() !== '') {
    return;
  }

  log(`removing ${publicDir}`);
  fs.rmSync(publicDir, { recursive: true, force: true });
}

function safeDiscoverProjects(selector) {
  if (!fs.existsSync(EXAMPLES_DIR)) {
    return [];
  }

  try {
    return discoverProjects(selector);
  } catch (error) {
    if (selector) {
      throw error;
    }
    return [];
  }
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

function runSyncOrFail(command, args, options, errorMessage) {
  const result = spawnSync(command, args, options);
  if (result.error) {
    fail(`${errorMessage}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(`${errorMessage}: exit code ${result.status}`);
  }
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

function writeLog(level, message, useStderr) {
  const stream = useStderr ? process.stderr : process.stdout;
  const prefix = colorize(formatPrefix(level), STATUS_COLOR[level] || null, level === 'PASS' ? ['bold'] : []);
  stream.write(`${prefix} ${message}${os.EOL}`);
}

function logProgress(current, total, message) {
  log(`[${current}/${total}] ${message}`);
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
      process.stdout.write(`${colorize('  SKIP', 'yellow', ['bold'])} ${skipped.project.name}: ${skipped.reason || 'skipped'}${os.EOL}`);
    }
  }
}

function printMatrixSummary(stageResults, allProjects) {
  printSection('Framework Summary', 'blue', `${allProjects.length} discovered`);

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

function formatPrefix(level) {
  return `[${new Date().toISOString()}] [framework-test] [${level}]`;
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

function delay(durationMs) {
  return new Promise((resolve) => {
    setTimeout(resolve, durationMs);
  });
}

function fail(message, options) {
  const error = new Error(message);
  if (options && typeof options === 'object') {
    Object.assign(error, options);
  }
  throw error;
}
