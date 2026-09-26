# 08: Implement URL-Based Software Installation

**Status:** Implemented; process-local caching and publication migration were
completed in steps 09 and 14

## Outcome

Install standalone `.aospkg` software from remote URLs, verify and pin the
immutable bytes for the live VM, and project them through Core/VFS under
`/opt/agentos`. Trusted embedded Core callers can use the same resolver with a
local `.aospkg` path.

The URL is only a locator. Package identity and installation correctness come from the
digest of the downloaded bytes. The hosted Rust actor never accepts or opens a
local path, host mount, file URL, descriptor, or caller-supplied filesystem.

This revision does not add package names, semantic versions, ranges, dist-tags,
dependency resolution, or a registry protocol. Step 14 removed
registry-software npm publication and added `.aospkg` artifact staging for
S3-compatible object storage.

## Public actor actions

```ts
interface RemotePackageSource {
  url: string;
  digest?: string;
}

interface InstallSoftwareInput {
  source: RemotePackageSource;
  expectedRevision?: number;
}

interface UninstallSoftwareInput {
  packageId: string;
  expectedRevision?: number;
}
```

- `software.install`: resolve, verify, project, and durably add one remote package.
- `software.uninstall`: remove one exact installed package from desired state.
- `software.list`: return Core's live installed `/opt/agentos` package view.

`packageId` is a stable installed identity derived from the verified digest, not
a package name or version selector. The result of installation includes the
resolved digest, exact size, bounded manifest metadata, and the resulting
complete config snapshot (including revision and application state).

Installing the same resolved digest is idempotent. A conflicting package or
command projection fails with a typed Core error. Uninstalling an absent exact
id returns `software_not_found`.

`software.install` and `software.uninstall` serialize with other durable config
mutations and accept `expectedRevision`. A successful mutation increments the
complete config revision. If durable persistence fails after a live Core change,
the actor attempts the inverse Core operation and reports both failures if that
rollback also fails.

## Core source model

Core supports a closed source enum equivalent to:

```text
PackageSource =
  Url { url, expected_digest? }
  | Path { path, expected_digest? }

resolve(source, limits) -> VerifiedPackage
project(package_set, runtime_generation) -> InstalledPackageSet
```

The embedded Core API may use either variant because its caller is deliberately
running in the host process. The hosted actor DTO exposes only `Url`. Serde or
manual conversion must make it impossible for an actor request to construct the
`Path` variant.

Both sources converge immediately on the same verified immutable package:

1. Open the source through the appropriate trusted Core adapter.
2. Read it under byte, time, header, and redirect limits.
3. Compute and verify its digest and size.
4. Parse and validate the `.aospkg` format and bounded manifest.
5. Pin the verified immutable file for the live installation. URL downloads use
   a private temporary file; path sources retain the canonical trusted path.
6. Project packages and commands through VFS under `/opt/agentos`.

Step 09 replaces the per-resolution URL temporary file with the process-local
digest cache and shared single-flight acquisition without changing this source
or installed-package API.

Core owns verification, mmap lifetime, collision handling, and VFS projection.
The actor owns only URL DTO validation, durable actor state, action routing, and
status translation.

Remote package bytes are untrusted input even when their URL came from a trusted
client or registry. Parsing and projection remain bounded trusted-Core code;
package executables run only inside the untrusted guest boundary.

Installing packages from a path inside the guest/actor filesystem is deferred.
It may be useful later, but it is not needed to establish remote package
installation and would require a separate immutable-snapshot design.

## Durable package state

Creation config may include a bounded list of `RemotePackageSource`.
`config.set` adds full-list replacement in step 11. `software.install` and
`software.uninstall` are domain-specific mutations of that same durable desired
package set; they do not
create a second authoritative software database.

When the caller omits `digest`, the first successful fetch computes it and the
actor persists the resolved URL, digest, size, and package id. From that point
the digest is pinned:

- Step 09 cache hits reuse bytes by digest.
- A missing local copy may fetch the URL again but must reproduce the persisted
  digest.
- Changed bytes at the same URL return a typed package-digest mismatch; they
  never silently upgrade an existing actor.
- Installing the URL again explicitly may resolve new bytes and replace or add
  them according to the package collision rules.

`software.list` reports the live applied projection, not merely requested URLs.
`config.get` reports the complete durable desired sources and reconciliation
state. This distinction keeps pending or failed installation visible.

## URL semantics

The installed identity is the lower-case `sha256:<64 hex>` digest of the exact
artifact bytes. URL text alone is not an immutable identity because servers can
replace bytes at the same location.

URL handling must:

- Permit HTTPS by default and allow plain HTTP only under an explicit local-test
  policy.
- Reject `file:`, `data:`, Unix socket, and other non-HTTP schemes.
- Revalidate every redirect and enforce a small redirect limit.
- Use no ambient cookies, cloud credentials, proxy credentials, or host auth.
- Bound URL bytes, DNS/connect/read time, object bytes, response headers, and
  redirects. Request identity encoding so compressed transfer expansion cannot
  bypass the byte limit.
- Resolve and pin a bounded public address set before connecting. Reject
  loopback, private, link-local, multicast, documentation, and other special
  addresses; local-test HTTP permits only loopback.
- Redact URL userinfo and sensitive query parameters from logs and errors.

Direct durable URLs should be stable and re-fetchable. Expiring presigned URLs
are a poor durable source because a cold cache may need the artifact after the
signature expires. Private registry authentication and renewable download URLs
belong in the later registry design.

## Deferred system cache and preloading

Step 09 adds one process-local content-addressed cache shared by actors in the
process.
The cache stores verified immutable `.aospkg` content, not writable installed
filesystems. Each VM receives its own package projection backed by the same
memory-mapped bytes.

The preload coordinator records exact `{ url, digest, size? }` identities. It can
warm packages before an actor starts because the URL is globally resolvable.
Usage messages remain approximate and coalesced; only desired package state is a
correctness source.

## npm publication removal completed in step 14

Step 14 completed the publication cutover:

- Remove `@agentos-software/*` from npm publish discovery.
- Remove runtime imports of software package JavaScript descriptors.
- Remove npm dependency graphs as the mechanism for default bundles or meta
  packages.
- Remove packument, semver, dist-tag, npm integrity, npm authentication, and npm
  lifecycle logic from registry software.
- Software directories may retain private workspace manifests needed by build
  tooling, but those manifests are not published products or a runtime registry.
- Publish `.aospkg` files and a checksummed release index directly to
  S3-compatible object storage.

This does not remove npm from untrusted guest JavaScript projects.
`javascript.npm.*` remains a guest execution feature with normal VM policy.

## Artifact publication completed in step 14

The publisher stages immutable object keys as:

```text
packages/<package-name>/<sha256>.aospkg
manifest.json
```

The schema-v1 manifest records each package name, `sha256:` digest, exact size,
and relative immutable artifact path. Release aliases and provenance live in the
object-store prefix selected by `scripts/publish`: commit SHA, release version,
and optional `latest`. The release workflow requires the complete canonical
command set, packs every artifact, validates the index, uploads the digest-keyed
objects, and publishes no `@agentos-software/*` package to npm.

The actor consumes only the URL and optional expected digest. S3 endpoint,
bucket, credentials, and upload policy are never actor configuration.

## Local testing

Resolver tests pack artifacts into a temporary directory and serve them through
an explicit loopback HTTP test server. The actor never receives the temporary
host path. `just software-artifacts-dry-run` rebuilds the complete tool set and
stages the exact credential-free artifact tree used by the release workflow.

Use the same download, digest, validation, and projection path as production.
Local HTTP is enabled only by the operator environment
`AGENTOS_ALLOW_INSECURE_LOCAL_PACKAGE_HTTP=1`; it is not an actor config field.

## Implemented tests

- Embedded Core path and URL sources converge on identical verified packages.
- Actor decoding cannot construct a path source, including malformed or
  adversarial tagged input.
- URL/path content identity, expected-digest mismatch, digest syntax, scheme and
  address rejection, and URL redaction.
- Actor action registration and URL-only creation normalization.
- Real sidecar install/list/uninstall projection, including removal of only the
  cosmetic VFS mountpoints created by the package.
- Unlink request and response wire round trips.

Step 09 added concurrent acquisition, cache identity, eviction, and pin tests.
Step 11 added full desired-versus-live replacement and restart reconciliation
tests. Step 14 added publisher tests; a hosted deployment URL smoke remains a
rollout gate.

## Acceptance criteria

- Hosted callers install `.aospkg` software using remote URLs only.
- No hosted DTO can identify or open a host or guest filesystem path.
- Core supports trusted embedded path sources and remote URL sources through one
  verified package pipeline.
- Installed content is keyed by digest, pinned for the lifetime of its live
  projection, and deduplicated by the process-local cache from step 09.
- A mutable URL cannot silently change an actor's installed package.
- `software.list` reflects the live installed view.
- The actor and Core installation APIs do not resolve package names or npm
  metadata. Registry software is staged as direct object-store artifacts.
- No semantic-versioning or registry protocol is implemented.
- A future registry can return `{ url, digest, size }` without changing the Core
  installation boundary.

## Dependencies and follow-up

Depends on the filesystem projection in step 04 and actor foundation in step 03.
Step 09 implements the process-local content cache. Step 10 preloads exact URL
and digest identities. Step 11 reconciles the durable package set alongside full
config replacement. Step 14 removes npm registry-software publication and adds
S3-compatible artifact publication. Package installation from an actor
filesystem and a possible future name/version registry remain step 15 work.
