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
for (const cmd of ['printf hi', 'echo hello | wc -c', 'printf hi > /tmp/x && cat /tmp/x']) {
  const r = await vm.exec(cmd);
  console.log('CMD', cmd);
  console.log(JSON.stringify(r, null, 2));
}
await vm.dispose();
