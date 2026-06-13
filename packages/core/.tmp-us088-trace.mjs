import { AgentOs } from './dist/index.js';
import coreutils from '@rivet-dev/agent-os-coreutils';
import sed from '@rivet-dev/agent-os-sed';
import grep from '@rivet-dev/agent-os-grep';
import gawk from '@rivet-dev/agent-os-gawk';
import findutils from '@rivet-dev/agent-os-findutils';
import diffutils from '@rivet-dev/agent-os-diffutils';
import tar from '@rivet-dev/agent-os-tar';
import gzip from '@rivet-dev/agent-os-gzip';
import jq from '@rivet-dev/agent-os-jq';
import ripgrep from '@rivet-dev/agent-os-ripgrep';
import fd from '@rivet-dev/agent-os-fd';
import tree from '@rivet-dev/agent-os-tree';
import filePkg from '@rivet-dev/agent-os-file';
import yq from '@rivet-dev/agent-os-yq';
import codex from '@rivet-dev/agent-os-codex';
import curl from '@rivet-dev/agent-os-curl';
const vm = await AgentOs.create({
  software: [coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip, jq, ripgrep, fd, tree, filePkg, yq, codex, curl],
  permissions: { fs: 'allow', network: 'allow', childProcess: 'allow', process: 'allow', env: 'allow', tool: 'allow' },
});
const { pid } = vm.spawn('sh', ['-c', 'echo hello | wc -c'], {
  env: { AGENT_OS_TRACE_HOST_PROCESS: '1' },
});
vm.onProcessStdout(pid, (data) => {
  console.log('STDOUT', JSON.stringify(Buffer.from(data).toString()));
});
vm.onProcessStderr(pid, (data) => {
  console.log('STDERR', JSON.stringify(Buffer.from(data).toString()));
});
try {
  const exitCode = await Promise.race([
    vm.waitProcess(pid),
    new Promise((_, reject) => setTimeout(() => reject(new Error('timeout waiting for shell')), 10000)),
  ]);
  console.log('EXIT', exitCode);
} catch (error) {
  console.log('ERROR', error instanceof Error ? error.message : String(error));
  console.log('PROCESSES', JSON.stringify(vm.allProcesses(), null, 2));
}
await vm.dispose();
