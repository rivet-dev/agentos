# Publishing secure-exec

`secure-exec` is released in lockstep with agentOS by `scripts/publish` through
`.github/workflows/publish.yaml`. Its committed version remains `0.0.1`; the
release helper rewrites its version and agentOS dependency together at publish
time. Use the normal agentOS release flow; do not publish this workspace directly
with an unresolved `workspace:*` dependency.

Before publishing, a package owner must configure the GitHub Actions trusted
publisher at <https://www.npmjs.com/package/secure-exec/access>:

- Organization: `rivet-dev`
- Repository: `agentos`
- Workflow filename: `publish.yaml`
- Environment: leave empty (the publish job does not use an environment)
- Allowed actions: enable direct `npm publish`

The existing `publish-npm` job uses a GitHub-hosted runner, Node.js 24, and
`id-token: write`. npm automatically exchanges the job's OIDC token; no
`NPM_TOKEN` or interactive npm login is needed in CI. Trusted publisher settings
live on npm and cannot be enabled by committing a workflow alone.

The existing `secure-exec` npm package has its own release history. Choose an
unused release version shared with agentOS; publishing an agentOS version below
the existing secure-exec latest version needs an explicit decision about the
`latest` dist-tag.

See <https://docs.npmjs.com/trusted-publishers/> for registry setup.
