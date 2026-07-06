// @generated - run pnpm --dir packages/build-tools build:package-format
import * as bare from "@rivetkit/bare-ts"

const DEFAULT_CONFIG = /* @__PURE__ */ bare.Config({})

export type i64 = bigint
export type u32 = number
export type u64 = bigint

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

export type TarEntry = {
    readonly path: string
    readonly kind: TarEntryKind
    readonly offset: u64
    readonly size: u64
    readonly mode: u32
    readonly uid: u32
    readonly gid: u32
    readonly mtime: i64
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

export type AgentBlock = {
    readonly acpEntrypoint: string
    readonly snapshot: boolean
    readonly env: ReadonlyMap<string, string>
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

export type CommandTarget = {
    readonly command: string
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

export type ManPage = {
    readonly section: string
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

export type ProvidesFile = {
    readonly source: string
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

export type ProvidesBlock = {
    readonly env: ReadonlyMap<string, string>
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

export type PackageManifest = {
    readonly name: string
    readonly version: string
    readonly agent: AgentBlock | null
    readonly provides: ProvidesBlock | null
    readonly commands: readonly CommandTarget[]
    readonly manPages: readonly ManPage[]
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
