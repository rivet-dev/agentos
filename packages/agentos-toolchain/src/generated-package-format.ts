// @generated - run pnpm --dir packages/build-tools build:package-format
import * as bare from "@rivetkit/bare-ts"

const DEFAULT_CONFIG = /* @__PURE__ */ bare.Config({})

export type i64 = bigint
export type u32 = number
export type u64 = bigint

/**
 * File type of a mount tar entry, as served by `TarFileSystem` stat/readdir.
 */
export enum TarEntryKind {
    File = "File",
    Directory = "Directory",
    Symlink = "Symlink",
}

export function readTarEntryKind(bc: bare.ByteCursor): TarEntryKind {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return TarEntryKind.File
        case 1:
            return TarEntryKind.Directory
        case 2:
            return TarEntryKind.Symlink
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeTarEntryKind(bc: bare.ByteCursor, x: TarEntryKind): void {
    switch (x) {
        case TarEntryKind.File: {
            bare.writeU8(bc, 0)
            break
        }
        case TarEntryKind.Directory: {
            bare.writeU8(bc, 1)
            break
        }
        case TarEntryKind.Symlink: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

function read0(bc: bare.ByteCursor): string | null {
    return bare.readBool(bc) ? bare.readString(bc) : null
}

function write0(bc: bare.ByteCursor, x: string | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeString(bc, x)
    }
}

/**
 * One entry of the chunk3 mount tar. The index lets the VFS project the tar
 * without parsing tar headers at VM startup: file contents are served as
 * byte ranges directly out of chunk3.
 */
export type TarEntry = {
    /**
     * Absolute path within the package mount root, e.g. `/bin/jq`.
     */
    readonly path: string
    readonly kind: TarEntryKind
    /**
     * Byte offset of the file's content within chunk3 (the mount tar). 0 for
     * directories and symlinks, which carry no content.
     */
    readonly offset: u64
    /**
     * Content length in bytes. 0 for directories and symlinks.
     */
    readonly size: u64
    /**
     * POSIX mode bits (file-type bits + permissions) reported by guest stat.
     */
    readonly mode: u32
    /**
     * Ownership reported by guest stat. Deterministic packs emit 0.
     */
    readonly uid: u32
    readonly gid: u32
    /**
     * Modification time (Unix seconds) reported by guest stat. Deterministic
     * packs emit 0.
     */
    readonly mtime: i64
    /**
     * Symlink target; present only when kind is SYMLINK.
     */
    readonly linkTarget: string | null
}

export function readTarEntry(bc: bare.ByteCursor): TarEntry {
    return {
        path: bare.readString(bc),
        kind: readTarEntryKind(bc),
        offset: bare.readU64(bc),
        size: bare.readU64(bc),
        mode: bare.readU32(bc),
        uid: bare.readU32(bc),
        gid: bare.readU32(bc),
        mtime: bare.readI64(bc),
        linkTarget: read0(bc),
    }
}

export function writeTarEntry(bc: bare.ByteCursor, x: TarEntry): void {
    bare.writeString(bc, x.path)
    writeTarEntryKind(bc, x.kind)
    bare.writeU64(bc, x.offset)
    bare.writeU64(bc, x.size)
    bare.writeU32(bc, x.mode)
    bare.writeU32(bc, x.uid)
    bare.writeU32(bc, x.gid)
    bare.writeI64(bc, x.mtime)
    write0(bc, x.linkTarget)
}

function read1(bc: bare.ByteCursor): ReadonlyMap<string, string> {
    const len = bare.readUintSafe(bc)
    const result = new Map<string, string>()
    for (let i = 0; i < len; i++) {
        const offset = bc.offset
        const key = bare.readString(bc)
        if (result.has(key)) {
            bc.offset = offset
            throw new bare.BareError(offset, "duplicated key")
        }
        result.set(key, bare.readString(bc))
    }
    return result
}

function write1(bc: bare.ByteCursor, x: ReadonlyMap<string, string>): void {
    bare.writeUintSafe(bc, x.size)
    for (const kv of x) {
        bare.writeString(bc, kv[0])
        bare.writeString(bc, kv[1])
    }
}

function read2(bc: bare.ByteCursor): readonly string[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [bare.readString(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = bare.readString(bc)
    }
    return result
}

function write2(bc: bare.ByteCursor, x: readonly string[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        bare.writeString(bc, x[i])
    }
}

/**
 * Agent metadata for packages that ship an ACP agent adapter. Present only on
 * agent packages; absence means the package is not launchable as an agent.
 */
export type AgentBlock = {
    /**
     * Name of a command in `PackageManifest.commands` that speaks ACP on
     * stdio. The sidecar launches this to run the agent.
     */
    readonly acpEntrypoint: string
    /**
     * True when the package ships a prebuilt SDK snapshot bundle for fast agent
     * cold starts; see `PackageManifest.snapshotBundlePath`.
     */
    readonly snapshot: boolean
    /**
     * Environment variables applied to the agent process at launch.
     */
    readonly env: ReadonlyMap<string, string>
    /**
     * Extra argv appended when launching the ACP entrypoint.
     */
    readonly launchArgs: readonly string[]
}

export function readAgentBlock(bc: bare.ByteCursor): AgentBlock {
    return {
        acpEntrypoint: bare.readString(bc),
        snapshot: bare.readBool(bc),
        env: read1(bc),
        launchArgs: read2(bc),
    }
}

export function writeAgentBlock(bc: bare.ByteCursor, x: AgentBlock): void {
    bare.writeString(bc, x.acpEntrypoint)
    bare.writeBool(bc, x.snapshot)
    write1(bc, x.env)
    write2(bc, x.launchArgs)
}

/**
 * One command projected into the shared `$PATH` dir. All packages link their
 * commands into a single `/opt/agentos/bin`, each as its own virtual symlink
 * leaf, so the sidecar can enumerate and remap commands from chunk1 alone —
 * no tar scan or JSON parse at VM startup.
 */
export type CommandTarget = {
    /**
     * The name the guest types: the link name under `/opt/agentos/bin`.
     */
    readonly command: string
    /**
     * Path of the executable it resolves to, relative to the package mount root
     * at `/opt/agentos/pkgs/<name>/<version>/`, e.g. `bin/jq` or
     * `dist/claude-cli.mjs`. Not necessarily under `bin/`: npm-style
     * `package.json` `bin` maps may point anywhere in the package.
     */
    readonly entry: string
}

export function readCommandTarget(bc: bare.ByteCursor): CommandTarget {
    return {
        command: bare.readString(bc),
        entry: bare.readString(bc),
    }
}

export function writeCommandTarget(bc: bare.ByteCursor, x: CommandTarget): void {
    bare.writeString(bc, x.command)
    bare.writeString(bc, x.entry)
}

/**
 * One man page shipped by the package, derived at pack time from mount paths
 * of the form `/share/man/<section>/<page>`. The projection serves manpage
 * aliases from this list without scanning the mount index at startup.
 */
export type ManPage = {
    /**
     * Man section directory name, e.g. `man1`.
     */
    readonly section: string
    /**
     * Page file name within the section, e.g. `jq.1`.
     */
    readonly page: string
}

export function readManPage(bc: bare.ByteCursor): ManPage {
    return {
        section: bare.readString(bc),
        page: bare.readString(bc),
    }
}

export function writeManPage(bc: bare.ByteCursor, x: ManPage): void {
    bare.writeString(bc, x.section)
    bare.writeString(bc, x.page)
}

/**
 * One directory the package projects to a fixed location in the guest
 * filesystem, for software that expects its runtime files at a well-known
 * absolute path (e.g. vim's `$VIMRUNTIME` tree).
 */
export type ProvidesFile = {
    /**
     * Source directory relative to the package mount root, e.g. `share/vim/vim92`.
     * Must be a directory; non-directory sources are skipped with a warning.
     */
    readonly source: string
    /**
     * Absolute guest path where the source is mounted read-only, e.g.
     * `/usr/local/share/vim/vim92`.
     */
    readonly target: string
}

export function readProvidesFile(bc: bare.ByteCursor): ProvidesFile {
    return {
        source: bare.readString(bc),
        target: bare.readString(bc),
    }
}

export function writeProvidesFile(bc: bare.ByteCursor, x: ProvidesFile): void {
    bare.writeString(bc, x.source)
    bare.writeString(bc, x.target)
}

function read3(bc: bare.ByteCursor): readonly ProvidesFile[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readProvidesFile(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readProvidesFile(bc)
    }
    return result
}

function write3(bc: bare.ByteCursor, x: readonly ProvidesFile[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeProvidesFile(bc, x[i])
    }
}

/**
 * Guest environment and filesystem projections the package provides.
 */
export type ProvidesBlock = {
    /**
     * Guest-wide environment defaults applied at VM creation. First package
     * wins: a key already set (by the VM config or an earlier package) is not
     * overridden.
     */
    readonly env: ReadonlyMap<string, string>
    /**
     * Package directories mounted at fixed guest paths; see `ProvidesFile`.
     */
    readonly files: readonly ProvidesFile[]
}

export function readProvidesBlock(bc: bare.ByteCursor): ProvidesBlock {
    return {
        env: read1(bc),
        files: read3(bc),
    }
}

export function writeProvidesBlock(bc: bare.ByteCursor, x: ProvidesBlock): void {
    write1(bc, x.env)
    write3(bc, x.files)
}

function read4(bc: bare.ByteCursor): AgentBlock | null {
    return bare.readBool(bc) ? readAgentBlock(bc) : null
}

function write4(bc: bare.ByteCursor, x: AgentBlock | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeAgentBlock(bc, x)
    }
}

function read5(bc: bare.ByteCursor): ProvidesBlock | null {
    return bare.readBool(bc) ? readProvidesBlock(bc) : null
}

function write5(bc: bare.ByteCursor, x: ProvidesBlock | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeProvidesBlock(bc, x)
    }
}

function read6(bc: bare.ByteCursor): readonly CommandTarget[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readCommandTarget(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readCommandTarget(bc)
    }
    return result
}

function write6(bc: bare.ByteCursor, x: readonly CommandTarget[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeCommandTarget(bc, x[i])
    }
}

function read7(bc: bare.ByteCursor): readonly ManPage[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readManPage(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readManPage(bc)
    }
    return result
}

function write7(bc: bare.ByteCursor, x: readonly ManPage[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeManPage(bc, x[i])
    }
}

/**
 * Chunk1: the runtime package manifest. Read alone (without chunk2/chunk3) on
 * the VM cold-start path to project `/opt/agentos` — keep it small and free of
 * anything that would require reading the mount payload.
 */
export type PackageManifest = {
    /**
     * Runtime package name, e.g. `jq` or `claude-code`. Names the projection dir
     * `/opt/agentos/pkgs/<name>/` and, for agent packages, the agent id.
     */
    readonly name: string
    /**
     * Package version, e.g. `0.3.0-rc.2`. Names the version dir under the
     * package projection dir.
     */
    readonly version: string
    /**
     * Agent metadata; present only for packages launchable as ACP agents.
     */
    readonly agent: AgentBlock | null
    /**
     * Env/file projections; present only when the package provides them.
     */
    readonly provides: ProvidesBlock | null
    /**
     * Commands linked under `/opt/agentos/bin`. Derived at pack time from the
     * `package.json` `bin` map when present, else from `/bin/*` in the mount.
     */
    readonly commands: readonly CommandTarget[]
    /**
     * Man pages shipped under `/share/man/` in the mount.
     */
    readonly manPages: readonly ManPage[]
    /**
     * Mount-root-relative path to the prebuilt SDK snapshot bundle
     * (`/dist/sdk-snapshot.js`), set iff `agent.snapshot` is true and the file
     * exists in the mount. Null otherwise.
     */
    readonly snapshotBundlePath: string | null
}

export function readPackageManifest(bc: bare.ByteCursor): PackageManifest {
    return {
        name: bare.readString(bc),
        version: bare.readString(bc),
        agent: read4(bc),
        provides: read5(bc),
        commands: read6(bc),
        manPages: read7(bc),
        snapshotBundlePath: read0(bc),
    }
}

export function writePackageManifest(bc: bare.ByteCursor, x: PackageManifest): void {
    bare.writeString(bc, x.name)
    bare.writeString(bc, x.version)
    write4(bc, x.agent)
    write5(bc, x.provides)
    write6(bc, x.commands)
    write7(bc, x.manPages)
    write0(bc, x.snapshotBundlePath)
}

export function encodePackageManifest(x: PackageManifest, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writePackageManifest(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodePackageManifest(bytes: Uint8Array): PackageManifest {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readPackageManifest(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

function read8(bc: bare.ByteCursor): readonly TarEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readTarEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readTarEntry(bc)
    }
    return result
}

function write8(bc: bare.ByteCursor, x: readonly TarEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeTarEntry(bc, x[i])
    }
}

/**
 * Chunk2: index of the chunk3 mount tar, entries sorted by path.
 */
export type MountIndex = {
    readonly tarEntries: readonly TarEntry[]
}

export function readMountIndex(bc: bare.ByteCursor): MountIndex {
    return {
        tarEntries: read8(bc),
    }
}

export function writeMountIndex(bc: bare.ByteCursor, x: MountIndex): void {
    write8(bc, x.tarEntries)
}

export function encodeMountIndex(x: MountIndex, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeMountIndex(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeMountIndex(bytes: Uint8Array): MountIndex {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readMountIndex(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}
