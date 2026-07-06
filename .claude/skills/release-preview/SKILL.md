---
name: release-preview
description: Cut an AgentOS release-preview. Use when the user asks for a preview, release-preview, or a branch dist-tag build.
---

# AgentOS Release Preview

AgentOS previews are branch snapshots from this repository. The workflow builds
debug artifacts and publishes npm packages under a sanitized branch dist-tag.
No crates.io release, git tag, or GitHub release is created.

secure-exec is a generated mirror. Do not bump a secure-exec ref or cut a
secure-exec preview by hand.

## Procedure

1. Push the AgentOS branch.
2. Dispatch and watch:

```bash
just release-preview <agentos-branch>
run=$(gh run list -R rivet-dev/agentos --workflow=publish.yaml -L1 --json databaseId --jq '.[0].databaseId')
gh run watch -R rivet-dev/agentos "$run" --exit-status
```

3. Install a preview with:

```bash
npm install @rivet-dev/agentos-core@<sanitized-branch>
```

## Rules

- Release-preview is for previews only; releases use the `release` skill.
- All code changes belong in AgentOS. The secure-exec mirror is regenerated
  from AgentOS and follows the published AgentOS version.
- On failure: `gh run view <run> --log-failed`, fix, re-dispatch, re-watch.
