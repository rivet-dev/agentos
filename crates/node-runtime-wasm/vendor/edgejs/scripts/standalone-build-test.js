'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const harness = require('./lib/framework-test-shared').create({
  rootDir: path.resolve(__dirname, '..'),
  toolName: 'standalone-build-test',
  stateDirName: '.standalone-build-test',
  defaultRunner: path.join('build-edge-quickjs-cli', 'edge'),
});

const {
  DEFAULT_HOST,
  EXAMPLES_DIR,
  LOG_DIR,
  PNPM_STORE_DIR,
  ROOT_DIR,
  SUBMODULE_HINT,
  buildPortCandidates,
  buildRouteUrl,
  buildRunnerStages,
  ensureDir,
  ensurePnpm,
  fail,
  formatDuration,
  injectRunner,
  installProjects,
  loadRouteMatrix,
  log,
  logProgress,
  logSuccess,
  logWarn,
  parseSelector,
  printMatrixSummary,
  printSection,
  readJsonFile,
  resolveHostNodeRunner,
  resolveRunnerTarget,
  routeReadinessPath,
  runRunnerStage,
  shellQuote,
  spawnLoggedProcess,
  stopProcess,
  validateRouteMatrix,
  waitForHttpResponse,
} = harness;

const STANDALONE_JSON_BASENAME = 'standalone.json';
const HOST_NODE_RUNNER = resolveHostNodeRunner();

main().catch((error) => {
  const message = error && error.message ? error.message : String(error);
  process.stderr.write(`[standalone-build-test] [ERROR] ${message}${os.EOL}`);
  process.exit(1);
});

async function main() {
  const args = process.argv.slice(2);
  const command = args[0];
  const selector = parseSelector(args.slice(1));

  if (command !== 'test') {
    fail([
      'usage: standalone-build-test.js test [js-framework-name]',
      'example: standalone-build-test.js test js-next-standalone',
    ].join('\n'));
  }

  await test(selector);
}

async function test(selector) {
  const prepared = await setup(selector);
  const stages = buildRunnerStages(HOST_NODE_RUNNER, prepared.runner);

  printSection('Standalone Build Matrix', 'blue', `${prepared.projects.length} app${prepared.projects.length === 1 ? '' : 's'}`);

  const stageResults = [];
  let previousResult = null;
  for (const stage of stages) {
    const selectedProjects = stage.selectProjects(prepared.projects, previousResult);
    const skippedProjects = stage.skippedProjects
      ? stage.skippedProjects(prepared.projects, selectedProjects, previousResult)
      : [];
    const result = await runRunnerStage(stage, selectedProjects, skippedProjects, {
      prepareProject,
      testProject,
    });
    stageResults.push(result);
    previousResult = result;
  }

  printMatrixSummary(stageResults, prepared.projects, 'Standalone Summary');

  if (stageResults.some((result) => result.failed.length > 0)) {
    fail('standalone build validation failed');
  }

  logSuccess('standalone build validation passed across all configured runner stages');
}

async function setup(selector) {
  logProgress(1, 4, 'starting standalone build test setup');
  ensureDir(LOG_DIR);
  ensureDir(PNPM_STORE_DIR);

  logProgress(2, 4, 'checking prerequisites');
  ensurePnpm();

  const projects = discoverStandaloneProjects(selector);
  log(`discovered ${projects.length} standalone app${projects.length === 1 ? '' : 's'}`);

  logProgress(3, 4, 'resolving runner target');
  const runner = resolveRunnerTarget();
  log(`using runner target: ${runner.targetPath}`);

  logProgress(4, 4, 'installing dependencies');
  await installProjects(projects);

  for (const project of projects) {
    injectRunner(project, runner.targetPath);
  }

  return {
    projects,
    runner,
  };
}

function discoverStandaloneProjects(selector) {
  if (!fs.existsSync(EXAMPLES_DIR)) {
    fail(`wasmer-examples is missing.\nRun: ${SUBMODULE_HINT}`);
  }

  const entries = fs.readdirSync(EXAMPLES_DIR, { withFileTypes: true });
  const projects = entries
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('js-'))
    .map((entry) => {
      const dir = path.join(EXAMPLES_DIR, entry.name);
      const packageJson = path.join(dir, 'package.json');
      const standaloneJson = path.join(dir, STANDALONE_JSON_BASENAME);
      if (!fs.existsSync(packageJson) || !fs.existsSync(standaloneJson)) {
        return null;
      }

      const manifest = readJsonFile(packageJson);
      const standalone = readStandaloneConfig(standaloneJson);
      if (standalone.skip) {
        return null;
      }

      return {
        dir,
        manifest,
        name: entry.name,
        packageJson,
        scripts: manifest.scripts || {},
        standalone,
      };
    })
    .filter(Boolean)
    .sort((left, right) => left.name.localeCompare(right.name));

  if (projects.length === 0) {
    fail(`no standalone apps found in wasmer-examples (expected js-* with ${STANDALONE_JSON_BASENAME}).\nRun: ${SUBMODULE_HINT}`);
  }

  if (!selector) {
    return projects;
  }

  const match = projects.find((project) => project.name === selector);
  if (!match) {
    fail([
      `unknown standalone selector: ${selector}`,
      `available standalone apps: ${projects.map((project) => project.name).join(', ')}`,
    ].join('\n'));
  }

  return [match];
}

function readStandaloneConfig(configPath) {
  const config = readJsonFile(configPath);
  if (!config || typeof config !== 'object') {
    fail(`invalid standalone config at ${configPath}: expected an object`);
  }
  if (config.version !== 1) {
    fail(`unsupported standalone config version at ${configPath}: expected version 1`);
  }
  if (!config.entry || typeof config.entry.path !== 'string') {
    fail(`invalid standalone config at ${configPath}: entry.path is required`);
  }

  const entry = config.entry;
  return {
    build: config.build && typeof config.build === 'object' ? config.build : {},
    entry: {
      args: Array.isArray(entry.args) ? entry.args.slice() : [],
      cwd: typeof entry.cwd === 'string' ? entry.cwd : '.',
      env: entry.env && typeof entry.env === 'object' ? entry.env : {},
      path: entry.path,
      prelaunch: Array.isArray(entry.prelaunch) ? entry.prelaunch.slice() : [],
    },
    routes: typeof config.routes === 'string' ? config.routes : 'routes.json',
    skip: Boolean(config.skip),
    skipReason: typeof config.skipReason === 'string' ? config.skipReason : 'skipped by standalone.json',
    skipStages: normalizeStandaloneSkipStages(config.skipStages),
  };
}

function normalizeStandaloneSkipStages(skipStages) {
  if (!skipStages || typeof skipStages !== 'object') {
    return {};
  }

  const normalized = {};
  for (const [stageKey, value] of Object.entries(skipStages)) {
    if (value === true) {
      normalized[stageKey] = `skipped on ${stageKey} by standalone.json`;
      continue;
    }
    if (typeof value === 'string' && value.trim()) {
      normalized[stageKey] = value.trim();
    }
  }
  return normalized;
}

function getStandaloneStageSkipReason(standalone, stage) {
  if (!standalone.skipStages || stage.key === 'node') {
    return null;
  }

  if (standalone.skipStages[stage.key]) {
    return standalone.skipStages[stage.key];
  }

  if (standalone.skipStages.comparison && stage.key !== 'node') {
    return standalone.skipStages.comparison;
  }

  return null;
}

function prepareProject(project, stage) {
  log(`[${stage.label}] preparing ${project.name}`);
  injectRunner(project, stage.runnerCommandParts);
  return {
    compatibleLaunchers: findCompatibleLaunchersSafe(project),
    reuseExistingBuild: stage.key !== 'node',
  };
}

function findCompatibleLaunchersSafe(project) {
  const binDir = path.join(project.dir, 'node_modules', '.bin');
  if (!fs.existsSync(binDir)) {
    return [];
  }
  return fs.readdirSync(binDir).filter((name) => name !== 'node').sort();
}

async function testProject(project, stage, index, total, preparation) {
  logProgress(index + 1, total, `[${stage.label}] testing ${project.name}`);

  if (project.standalone.skip) {
    const error = new Error(project.standalone.skipReason);
    error.skip = true;
    error.detail = project.standalone.skipReason;
    throw error;
  }

  const stageSkipReason = getStandaloneStageSkipReason(project.standalone, stage);
  if (stageSkipReason) {
    const error = new Error(stageSkipReason);
    error.skip = true;
    error.detail = stageSkipReason;
    throw error;
  }

  const portCandidates = buildPortCandidates(index);
  const routesFile = project.standalone.routes;
  const runtime = {
    mode: 'standalone-entry',
    name: 'standalone',
    command: project.standalone.entry.path,
  };

  if (stage.key === 'node' || !preparation.reuseExistingBuild) {
    await runNodeBuild(project, stage);
    await runPrelaunchSteps(project, stage);
  } else {
    log(`reusing Node build output for ${project.name} on ${stage.label}`);
  }

  const entryAbsolutePath = path.join(project.dir, project.standalone.entry.path);
  if (!fs.existsSync(entryAbsolutePath)) {
    fail(`standalone entry missing for ${project.name}: ${entryAbsolutePath}`);
  }

  const readinessPath = routeReadinessPath(project, stage, runtime, routesFile);
  const server = await startStandaloneEntry(project, stage, portCandidates, runtime);
  try {
    const routes = loadRouteMatrix(project, stage, runtime, routesFile);
    const routeResults = await validateRouteMatrix(project, runtime, server.port, routes);
    return {
      port: server.port,
      project,
      response: server.response,
      routeResults,
      runtime,
      serverLogPath: server.logPath,
    };
  } finally {
    await stopProcess(server.handle);
  }
}

async function runNodeBuild(project, stage) {
  const buildCommand = project.standalone.build.command || 'pnpm run build';
  const logPath = path.join(LOG_DIR, `${project.name}.${stage.key}.build.log`);
  log(`building ${project.name} with host Node.js (${buildCommand})`);

  const startedAt = Date.now();
  const result = spawnSync(buildCommand, {
    cwd: project.dir,
    encoding: 'utf8',
    env: {
      ...process.env,
      CI: process.env.CI || 'true',
    },
    shell: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  fs.writeFileSync(logPath, [
    `${buildCommand}${os.EOL}`,
    result.stdout || '',
    result.stderr || '',
    `exit: ${result.status}${os.EOL}`,
  ].join(''));

  if (result.status !== 0) {
    fail(`build failed for ${project.name} on host Node.js`, {
      detail: (result.stderr || result.stdout || '').trim().split(/\r?\n/).slice(-3).join(' | '),
      logPath,
    });
  }

  log(`build completed for ${project.name} (${formatDuration(Date.now() - startedAt)})`);
}

async function runPrelaunchSteps(project, stage) {
  for (const command of project.standalone.entry.prelaunch) {
    log(`running prelaunch for ${project.name}: ${command}`);
    const logPath = path.join(LOG_DIR, `${project.name}.${stage.key}.prelaunch.log`);
    const result = spawnSync(command, {
      cwd: project.dir,
      encoding: 'utf8',
      env: process.env,
      shell: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    fs.writeFileSync(logPath, [
      `${command}${os.EOL}`,
      result.stdout || '',
      result.stderr || '',
      `exit: ${result.status}${os.EOL}`,
    ].join(''));
    if (result.status !== 0) {
      fail(`prelaunch failed for ${project.name}: ${command}`, {
        detail: (result.stderr || result.stdout || '').trim().split(/\r?\n/).slice(-2).join(' | '),
        logPath,
      });
    }
  }
}

async function startStandaloneEntry(project, stage, portCandidates, runtime) {
  const entryAbsolutePath = path.resolve(project.dir, project.standalone.entry.path);
  const entryCwd = path.resolve(project.dir, project.standalone.entry.cwd);
  const entryRelPath = path.relative(entryCwd, entryAbsolutePath);
  const logPath = path.join(LOG_DIR, `${project.name}.${stage.key}.server.log`);

  let lastError = null;
  for (let index = 0; index < portCandidates.length; index += 1) {
    const port = portCandidates[index];
    const env = expandEntryEnv(project.standalone.entry.env, port);
    const runnerParts = normalizeRunnerParts(stage.runnerCommandParts);
    const args = project.standalone.entry.args.map(String);
    const commandDisplay = [
      `cd ${shellQuote(entryCwd)}`,
      `&& ${runnerParts.map(shellQuote).join(' ')} ${[entryRelPath, ...args].map(shellQuote).join(' ')}`,
    ].join(' ');

    log(`starting ${project.name} standalone entry on ${DEFAULT_HOST}:${port} (attempt ${index + 1})`);
    const handle = spawnLoggedProcess({
      append: index > 0,
      commandDisplay,
      cwd: project.dir,
      description: `standalone entry for ${project.name} on ${DEFAULT_HOST}:${port}`,
      env: {
        ...process.env,
        ...env,
      },
      logPath,
      shellCommand: commandDisplay,
    });

    try {
      const readinessPath = routeReadinessPath(project, stage, runtime, project.standalone.routes);
      const response = await waitForHttpResponse(handle, buildRouteUrl(port, readinessPath));
      return {
        handle,
        logPath,
        port,
        response,
      };
    } catch (error) {
      lastError = error;
      await stopProcess(handle);
      logWarn(`standalone start attempt failed for ${project.name} on ${DEFAULT_HOST}:${port}: ${error.message}`);
    }
  }

  fail(`unable to start standalone entry for ${project.name} on ${stage.label}; tried ports ${portCandidates.join(', ')}`, {
    detail: lastError && lastError.detail ? lastError.detail : lastError && lastError.message,
    logPath,
  });
}

function normalizeRunnerParts(runnerCommandParts) {
  if (Array.isArray(runnerCommandParts)) {
    return runnerCommandParts.slice();
  }
  return [runnerCommandParts];
}

function expandEntryEnv(envTemplate, port) {
  const env = {};
  for (const [name, value] of Object.entries(envTemplate || {})) {
    env[name] = String(value).replace(/\{port\}/g, String(port));
  }
  return env;
}
