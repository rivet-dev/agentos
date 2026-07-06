// @generated - run pnpm --dir packages/build-tools build:protocol
import * as bare from "@rivetkit/bare-ts"

const DEFAULT_CONFIG = /* @__PURE__ */ bare.Config({})

export type i32 = number
export type i64 = bigint
export type u16 = number
export type u32 = number
export type u64 = bigint

export type JsonUtf8 = string

export function readJsonUtf8(bc: bare.ByteCursor): JsonUtf8 {
    return bare.readString(bc)
}

export function writeJsonUtf8(bc: bare.ByteCursor, x: JsonUtf8): void {
    bare.writeString(bc, x)
}

export type ProtocolSchema = {
    readonly name: string
    readonly version: u16
}

export function readProtocolSchema(bc: bare.ByteCursor): ProtocolSchema {
    return {
        name: bare.readString(bc),
        version: bare.readU16(bc),
    }
}

export function writeProtocolSchema(bc: bare.ByteCursor, x: ProtocolSchema): void {
    bare.writeString(bc, x.name)
    bare.writeU16(bc, x.version)
}

export type RequestId = i64

export function readRequestId(bc: bare.ByteCursor): RequestId {
    return bare.readI64(bc)
}

export function writeRequestId(bc: bare.ByteCursor, x: RequestId): void {
    bare.writeI64(bc, x)
}

export type ExtEnvelope = {
    readonly namespace: string
    readonly payload: ArrayBuffer
}

export function readExtEnvelope(bc: bare.ByteCursor): ExtEnvelope {
    return {
        namespace: bare.readString(bc),
        payload: bare.readData(bc),
    }
}

export function writeExtEnvelope(bc: bare.ByteCursor, x: ExtEnvelope): void {
    bare.writeString(bc, x.namespace)
    bare.writeData(bc, x.payload)
}

export type ConnectionOwnership = {
    readonly connectionId: string
}

export function readConnectionOwnership(bc: bare.ByteCursor): ConnectionOwnership {
    return {
        connectionId: bare.readString(bc),
    }
}

export function writeConnectionOwnership(bc: bare.ByteCursor, x: ConnectionOwnership): void {
    bare.writeString(bc, x.connectionId)
}

export type SessionOwnership = {
    readonly connectionId: string
    readonly sessionId: string
}

export function readSessionOwnership(bc: bare.ByteCursor): SessionOwnership {
    return {
        connectionId: bare.readString(bc),
        sessionId: bare.readString(bc),
    }
}

export function writeSessionOwnership(bc: bare.ByteCursor, x: SessionOwnership): void {
    bare.writeString(bc, x.connectionId)
    bare.writeString(bc, x.sessionId)
}

export type VmOwnership = {
    readonly connectionId: string
    readonly sessionId: string
    readonly vmId: string
}

export function readVmOwnership(bc: bare.ByteCursor): VmOwnership {
    return {
        connectionId: bare.readString(bc),
        sessionId: bare.readString(bc),
        vmId: bare.readString(bc),
    }
}

export function writeVmOwnership(bc: bare.ByteCursor, x: VmOwnership): void {
    bare.writeString(bc, x.connectionId)
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.vmId)
}

export type OwnershipScope =
    | { readonly tag: "ConnectionOwnership"; readonly val: ConnectionOwnership }
    | { readonly tag: "SessionOwnership"; readonly val: SessionOwnership }
    | { readonly tag: "VmOwnership"; readonly val: VmOwnership }

export function readOwnershipScope(bc: bare.ByteCursor): OwnershipScope {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "ConnectionOwnership", val: readConnectionOwnership(bc) }
        case 1:
            return { tag: "SessionOwnership", val: readSessionOwnership(bc) }
        case 2:
            return { tag: "VmOwnership", val: readVmOwnership(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeOwnershipScope(bc: bare.ByteCursor, x: OwnershipScope): void {
    switch (x.tag) {
        case "ConnectionOwnership": {
            bare.writeU8(bc, 0)
            writeConnectionOwnership(bc, x.val)
            break
        }
        case "SessionOwnership": {
            bare.writeU8(bc, 1)
            writeSessionOwnership(bc, x.val)
            break
        }
        case "VmOwnership": {
            bare.writeU8(bc, 2)
            writeVmOwnership(bc, x.val)
            break
        }
    }
}

export type AuthenticateRequest = {
    readonly clientName: string
    readonly authToken: string
    readonly protocolVersion: u16
    readonly bridgeVersion: u32
}

export function readAuthenticateRequest(bc: bare.ByteCursor): AuthenticateRequest {
    return {
        clientName: bare.readString(bc),
        authToken: bare.readString(bc),
        protocolVersion: bare.readU16(bc),
        bridgeVersion: bare.readU32(bc),
    }
}

export function writeAuthenticateRequest(bc: bare.ByteCursor, x: AuthenticateRequest): void {
    bare.writeString(bc, x.clientName)
    bare.writeString(bc, x.authToken)
    bare.writeU16(bc, x.protocolVersion)
    bare.writeU32(bc, x.bridgeVersion)
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

export type SidecarPlacementShared = {
    readonly pool: string | null
}

export function readSidecarPlacementShared(bc: bare.ByteCursor): SidecarPlacementShared {
    return {
        pool: read0(bc),
    }
}

export function writeSidecarPlacementShared(bc: bare.ByteCursor, x: SidecarPlacementShared): void {
    write0(bc, x.pool)
}

export type SidecarPlacementExplicit = {
    readonly sidecarId: string
}

export function readSidecarPlacementExplicit(bc: bare.ByteCursor): SidecarPlacementExplicit {
    return {
        sidecarId: bare.readString(bc),
    }
}

export function writeSidecarPlacementExplicit(bc: bare.ByteCursor, x: SidecarPlacementExplicit): void {
    bare.writeString(bc, x.sidecarId)
}

export type SidecarPlacement =
    | { readonly tag: "SidecarPlacementShared"; readonly val: SidecarPlacementShared }
    | { readonly tag: "SidecarPlacementExplicit"; readonly val: SidecarPlacementExplicit }

export function readSidecarPlacement(bc: bare.ByteCursor): SidecarPlacement {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SidecarPlacementShared", val: readSidecarPlacementShared(bc) }
        case 1:
            return { tag: "SidecarPlacementExplicit", val: readSidecarPlacementExplicit(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSidecarPlacement(bc: bare.ByteCursor, x: SidecarPlacement): void {
    switch (x.tag) {
        case "SidecarPlacementShared": {
            bare.writeU8(bc, 0)
            writeSidecarPlacementShared(bc, x.val)
            break
        }
        case "SidecarPlacementExplicit": {
            bare.writeU8(bc, 1)
            writeSidecarPlacementExplicit(bc, x.val)
            break
        }
    }
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

export type OpenSessionRequest = {
    readonly placement: SidecarPlacement
    readonly metadata: ReadonlyMap<string, string>
}

export function readOpenSessionRequest(bc: bare.ByteCursor): OpenSessionRequest {
    return {
        placement: readSidecarPlacement(bc),
        metadata: read1(bc),
    }
}

export function writeOpenSessionRequest(bc: bare.ByteCursor, x: OpenSessionRequest): void {
    writeSidecarPlacement(bc, x.placement)
    write1(bc, x.metadata)
}

export enum GuestRuntimeKind {
    JavaScript = "JavaScript",
    Python = "Python",
    WebAssembly = "WebAssembly",
}

export function readGuestRuntimeKind(bc: bare.ByteCursor): GuestRuntimeKind {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return GuestRuntimeKind.JavaScript
        case 1:
            return GuestRuntimeKind.Python
        case 2:
            return GuestRuntimeKind.WebAssembly
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeGuestRuntimeKind(bc: bare.ByteCursor, x: GuestRuntimeKind): void {
    switch (x) {
        case GuestRuntimeKind.JavaScript: {
            bare.writeU8(bc, 0)
            break
        }
        case GuestRuntimeKind.Python: {
            bare.writeU8(bc, 1)
            break
        }
        case GuestRuntimeKind.WebAssembly: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

export enum RootFilesystemMode {
    Ephemeral = "Ephemeral",
    ReadOnly = "ReadOnly",
}

export function readRootFilesystemMode(bc: bare.ByteCursor): RootFilesystemMode {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return RootFilesystemMode.Ephemeral
        case 1:
            return RootFilesystemMode.ReadOnly
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeRootFilesystemMode(bc: bare.ByteCursor, x: RootFilesystemMode): void {
    switch (x) {
        case RootFilesystemMode.Ephemeral: {
            bare.writeU8(bc, 0)
            break
        }
        case RootFilesystemMode.ReadOnly: {
            bare.writeU8(bc, 1)
            break
        }
    }
}

export enum RootFilesystemEntryKind {
    File = "File",
    Directory = "Directory",
    Symlink = "Symlink",
}

export function readRootFilesystemEntryKind(bc: bare.ByteCursor): RootFilesystemEntryKind {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return RootFilesystemEntryKind.File
        case 1:
            return RootFilesystemEntryKind.Directory
        case 2:
            return RootFilesystemEntryKind.Symlink
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeRootFilesystemEntryKind(bc: bare.ByteCursor, x: RootFilesystemEntryKind): void {
    switch (x) {
        case RootFilesystemEntryKind.File: {
            bare.writeU8(bc, 0)
            break
        }
        case RootFilesystemEntryKind.Directory: {
            bare.writeU8(bc, 1)
            break
        }
        case RootFilesystemEntryKind.Symlink: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

export enum RootFilesystemEntryEncoding {
    UtF8 = "UtF8",
    BasE64 = "BasE64",
}

export function readRootFilesystemEntryEncoding(bc: bare.ByteCursor): RootFilesystemEntryEncoding {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return RootFilesystemEntryEncoding.UtF8
        case 1:
            return RootFilesystemEntryEncoding.BasE64
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeRootFilesystemEntryEncoding(bc: bare.ByteCursor, x: RootFilesystemEntryEncoding): void {
    switch (x) {
        case RootFilesystemEntryEncoding.UtF8: {
            bare.writeU8(bc, 0)
            break
        }
        case RootFilesystemEntryEncoding.BasE64: {
            bare.writeU8(bc, 1)
            break
        }
    }
}

function read2(bc: bare.ByteCursor): u32 | null {
    return bare.readBool(bc) ? bare.readU32(bc) : null
}

function write2(bc: bare.ByteCursor, x: u32 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeU32(bc, x)
    }
}

function read3(bc: bare.ByteCursor): RootFilesystemEntryEncoding | null {
    return bare.readBool(bc) ? readRootFilesystemEntryEncoding(bc) : null
}

function write3(bc: bare.ByteCursor, x: RootFilesystemEntryEncoding | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeRootFilesystemEntryEncoding(bc, x)
    }
}

export type RootFilesystemEntry = {
    readonly path: string
    readonly kind: RootFilesystemEntryKind
    readonly mode: u32 | null
    readonly uid: u32 | null
    readonly gid: u32 | null
    readonly content: string | null
    readonly encoding: RootFilesystemEntryEncoding | null
    readonly target: string | null
    readonly executable: boolean
}

export function readRootFilesystemEntry(bc: bare.ByteCursor): RootFilesystemEntry {
    return {
        path: bare.readString(bc),
        kind: readRootFilesystemEntryKind(bc),
        mode: read2(bc),
        uid: read2(bc),
        gid: read2(bc),
        content: read0(bc),
        encoding: read3(bc),
        target: read0(bc),
        executable: bare.readBool(bc),
    }
}

export function writeRootFilesystemEntry(bc: bare.ByteCursor, x: RootFilesystemEntry): void {
    bare.writeString(bc, x.path)
    writeRootFilesystemEntryKind(bc, x.kind)
    write2(bc, x.mode)
    write2(bc, x.uid)
    write2(bc, x.gid)
    write0(bc, x.content)
    write3(bc, x.encoding)
    write0(bc, x.target)
    bare.writeBool(bc, x.executable)
}

function read4(bc: bare.ByteCursor): readonly RootFilesystemEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readRootFilesystemEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readRootFilesystemEntry(bc)
    }
    return result
}

function write4(bc: bare.ByteCursor, x: readonly RootFilesystemEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeRootFilesystemEntry(bc, x[i])
    }
}

export type SnapshotRootFilesystemLower = {
    readonly entries: readonly RootFilesystemEntry[]
}

export function readSnapshotRootFilesystemLower(bc: bare.ByteCursor): SnapshotRootFilesystemLower {
    return {
        entries: read4(bc),
    }
}

export function writeSnapshotRootFilesystemLower(bc: bare.ByteCursor, x: SnapshotRootFilesystemLower): void {
    write4(bc, x.entries)
}

export type BundledBaseFilesystemLower = null

export type RootFilesystemLowerDescriptor =
    | { readonly tag: "SnapshotRootFilesystemLower"; readonly val: SnapshotRootFilesystemLower }
    | { readonly tag: "BundledBaseFilesystemLower"; readonly val: BundledBaseFilesystemLower }

export function readRootFilesystemLowerDescriptor(bc: bare.ByteCursor): RootFilesystemLowerDescriptor {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "SnapshotRootFilesystemLower", val: readSnapshotRootFilesystemLower(bc) }
        case 1:
            return { tag: "BundledBaseFilesystemLower", val: null }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeRootFilesystemLowerDescriptor(bc: bare.ByteCursor, x: RootFilesystemLowerDescriptor): void {
    switch (x.tag) {
        case "SnapshotRootFilesystemLower": {
            bare.writeU8(bc, 0)
            writeSnapshotRootFilesystemLower(bc, x.val)
            break
        }
        case "BundledBaseFilesystemLower": {
            bare.writeU8(bc, 1)
            break
        }
    }
}

function read5(bc: bare.ByteCursor): readonly RootFilesystemLowerDescriptor[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readRootFilesystemLowerDescriptor(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readRootFilesystemLowerDescriptor(bc)
    }
    return result
}

function write5(bc: bare.ByteCursor, x: readonly RootFilesystemLowerDescriptor[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeRootFilesystemLowerDescriptor(bc, x[i])
    }
}

export type RootFilesystemDescriptor = {
    readonly mode: RootFilesystemMode
    readonly disableDefaultBaseLayer: boolean
    readonly lowers: readonly RootFilesystemLowerDescriptor[]
    readonly bootstrapEntries: readonly RootFilesystemEntry[]
}

export function readRootFilesystemDescriptor(bc: bare.ByteCursor): RootFilesystemDescriptor {
    return {
        mode: readRootFilesystemMode(bc),
        disableDefaultBaseLayer: bare.readBool(bc),
        lowers: read5(bc),
        bootstrapEntries: read4(bc),
    }
}

export function writeRootFilesystemDescriptor(bc: bare.ByteCursor, x: RootFilesystemDescriptor): void {
    writeRootFilesystemMode(bc, x.mode)
    bare.writeBool(bc, x.disableDefaultBaseLayer)
    write5(bc, x.lowers)
    write4(bc, x.bootstrapEntries)
}

export function encodeRootFilesystemDescriptor(x: RootFilesystemDescriptor, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeRootFilesystemDescriptor(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeRootFilesystemDescriptor(bytes: Uint8Array): RootFilesystemDescriptor {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readRootFilesystemDescriptor(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

export enum PermissionMode {
    Allow = "Allow",
    Ask = "Ask",
    Deny = "Deny",
}

export function readPermissionMode(bc: bare.ByteCursor): PermissionMode {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return PermissionMode.Allow
        case 1:
            return PermissionMode.Ask
        case 2:
            return PermissionMode.Deny
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writePermissionMode(bc: bare.ByteCursor, x: PermissionMode): void {
    switch (x) {
        case PermissionMode.Allow: {
            bare.writeU8(bc, 0)
            break
        }
        case PermissionMode.Ask: {
            bare.writeU8(bc, 1)
            break
        }
        case PermissionMode.Deny: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

function read6(bc: bare.ByteCursor): readonly string[] {
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

function write6(bc: bare.ByteCursor, x: readonly string[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        bare.writeString(bc, x[i])
    }
}

export type FsPermissionRule = {
    readonly mode: PermissionMode
    readonly operations: readonly string[]
    readonly paths: readonly string[]
}

export function readFsPermissionRule(bc: bare.ByteCursor): FsPermissionRule {
    return {
        mode: readPermissionMode(bc),
        operations: read6(bc),
        paths: read6(bc),
    }
}

export function writeFsPermissionRule(bc: bare.ByteCursor, x: FsPermissionRule): void {
    writePermissionMode(bc, x.mode)
    write6(bc, x.operations)
    write6(bc, x.paths)
}

function read7(bc: bare.ByteCursor): PermissionMode | null {
    return bare.readBool(bc) ? readPermissionMode(bc) : null
}

function write7(bc: bare.ByteCursor, x: PermissionMode | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writePermissionMode(bc, x)
    }
}

function read8(bc: bare.ByteCursor): readonly FsPermissionRule[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readFsPermissionRule(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readFsPermissionRule(bc)
    }
    return result
}

function write8(bc: bare.ByteCursor, x: readonly FsPermissionRule[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeFsPermissionRule(bc, x[i])
    }
}

export type FsPermissionRuleSet = {
    readonly default: PermissionMode | null
    readonly rules: readonly FsPermissionRule[]
}

export function readFsPermissionRuleSet(bc: bare.ByteCursor): FsPermissionRuleSet {
    return {
        default: read7(bc),
        rules: read8(bc),
    }
}

export function writeFsPermissionRuleSet(bc: bare.ByteCursor, x: FsPermissionRuleSet): void {
    write7(bc, x.default)
    write8(bc, x.rules)
}

export type FsPermissionScope =
    | { readonly tag: "PermissionMode"; readonly val: PermissionMode }
    | { readonly tag: "FsPermissionRuleSet"; readonly val: FsPermissionRuleSet }

export function readFsPermissionScope(bc: bare.ByteCursor): FsPermissionScope {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "PermissionMode", val: readPermissionMode(bc) }
        case 1:
            return { tag: "FsPermissionRuleSet", val: readFsPermissionRuleSet(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeFsPermissionScope(bc: bare.ByteCursor, x: FsPermissionScope): void {
    switch (x.tag) {
        case "PermissionMode": {
            bare.writeU8(bc, 0)
            writePermissionMode(bc, x.val)
            break
        }
        case "FsPermissionRuleSet": {
            bare.writeU8(bc, 1)
            writeFsPermissionRuleSet(bc, x.val)
            break
        }
    }
}

export type PatternPermissionRule = {
    readonly mode: PermissionMode
    readonly operations: readonly string[]
    readonly patterns: readonly string[]
}

export function readPatternPermissionRule(bc: bare.ByteCursor): PatternPermissionRule {
    return {
        mode: readPermissionMode(bc),
        operations: read6(bc),
        patterns: read6(bc),
    }
}

export function writePatternPermissionRule(bc: bare.ByteCursor, x: PatternPermissionRule): void {
    writePermissionMode(bc, x.mode)
    write6(bc, x.operations)
    write6(bc, x.patterns)
}

function read9(bc: bare.ByteCursor): readonly PatternPermissionRule[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readPatternPermissionRule(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readPatternPermissionRule(bc)
    }
    return result
}

function write9(bc: bare.ByteCursor, x: readonly PatternPermissionRule[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writePatternPermissionRule(bc, x[i])
    }
}

export type PatternPermissionRuleSet = {
    readonly default: PermissionMode | null
    readonly rules: readonly PatternPermissionRule[]
}

export function readPatternPermissionRuleSet(bc: bare.ByteCursor): PatternPermissionRuleSet {
    return {
        default: read7(bc),
        rules: read9(bc),
    }
}

export function writePatternPermissionRuleSet(bc: bare.ByteCursor, x: PatternPermissionRuleSet): void {
    write7(bc, x.default)
    write9(bc, x.rules)
}

export type PatternPermissionScope =
    | { readonly tag: "PermissionMode"; readonly val: PermissionMode }
    | { readonly tag: "PatternPermissionRuleSet"; readonly val: PatternPermissionRuleSet }

export function readPatternPermissionScope(bc: bare.ByteCursor): PatternPermissionScope {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "PermissionMode", val: readPermissionMode(bc) }
        case 1:
            return { tag: "PatternPermissionRuleSet", val: readPatternPermissionRuleSet(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writePatternPermissionScope(bc: bare.ByteCursor, x: PatternPermissionScope): void {
    switch (x.tag) {
        case "PermissionMode": {
            bare.writeU8(bc, 0)
            writePermissionMode(bc, x.val)
            break
        }
        case "PatternPermissionRuleSet": {
            bare.writeU8(bc, 1)
            writePatternPermissionRuleSet(bc, x.val)
            break
        }
    }
}

function read10(bc: bare.ByteCursor): FsPermissionScope | null {
    return bare.readBool(bc) ? readFsPermissionScope(bc) : null
}

function write10(bc: bare.ByteCursor, x: FsPermissionScope | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeFsPermissionScope(bc, x)
    }
}

function read11(bc: bare.ByteCursor): PatternPermissionScope | null {
    return bare.readBool(bc) ? readPatternPermissionScope(bc) : null
}

function write11(bc: bare.ByteCursor, x: PatternPermissionScope | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writePatternPermissionScope(bc, x)
    }
}

export type PermissionsPolicy = {
    readonly fs: FsPermissionScope | null
    readonly network: PatternPermissionScope | null
    readonly childProcess: PatternPermissionScope | null
    readonly process: PatternPermissionScope | null
    readonly env: PatternPermissionScope | null
    readonly binding: PatternPermissionScope | null
}

export function readPermissionsPolicy(bc: bare.ByteCursor): PermissionsPolicy {
    return {
        fs: read10(bc),
        network: read11(bc),
        childProcess: read11(bc),
        process: read11(bc),
        env: read11(bc),
        binding: read11(bc),
    }
}

export function writePermissionsPolicy(bc: bare.ByteCursor, x: PermissionsPolicy): void {
    write10(bc, x.fs)
    write11(bc, x.network)
    write11(bc, x.childProcess)
    write11(bc, x.process)
    write11(bc, x.env)
    write11(bc, x.binding)
}

export type CreateVmRequest = {
    readonly runtime: GuestRuntimeKind
    readonly config: JsonUtf8
}

export function readCreateVmRequest(bc: bare.ByteCursor): CreateVmRequest {
    return {
        runtime: readGuestRuntimeKind(bc),
        config: readJsonUtf8(bc),
    }
}

export function writeCreateVmRequest(bc: bare.ByteCursor, x: CreateVmRequest): void {
    writeGuestRuntimeKind(bc, x.runtime)
    writeJsonUtf8(bc, x.config)
}

export enum DisposeReason {
    Requested = "Requested",
    ConnectionClosed = "ConnectionClosed",
    HostShutdown = "HostShutdown",
}

export function readDisposeReason(bc: bare.ByteCursor): DisposeReason {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return DisposeReason.Requested
        case 1:
            return DisposeReason.ConnectionClosed
        case 2:
            return DisposeReason.HostShutdown
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeDisposeReason(bc: bare.ByteCursor, x: DisposeReason): void {
    switch (x) {
        case DisposeReason.Requested: {
            bare.writeU8(bc, 0)
            break
        }
        case DisposeReason.ConnectionClosed: {
            bare.writeU8(bc, 1)
            break
        }
        case DisposeReason.HostShutdown: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

export type DisposeVmRequest = {
    readonly reason: DisposeReason
}

export function readDisposeVmRequest(bc: bare.ByteCursor): DisposeVmRequest {
    return {
        reason: readDisposeReason(bc),
    }
}

export function writeDisposeVmRequest(bc: bare.ByteCursor, x: DisposeVmRequest): void {
    writeDisposeReason(bc, x.reason)
}

export type BootstrapRootFilesystemRequest = {
    readonly entries: readonly RootFilesystemEntry[]
}

export function readBootstrapRootFilesystemRequest(bc: bare.ByteCursor): BootstrapRootFilesystemRequest {
    return {
        entries: read4(bc),
    }
}

export function writeBootstrapRootFilesystemRequest(bc: bare.ByteCursor, x: BootstrapRootFilesystemRequest): void {
    write4(bc, x.entries)
}

export type MountPluginDescriptor = {
    readonly id: string
    readonly config: JsonUtf8
}

export function readMountPluginDescriptor(bc: bare.ByteCursor): MountPluginDescriptor {
    return {
        id: bare.readString(bc),
        config: readJsonUtf8(bc),
    }
}

export function writeMountPluginDescriptor(bc: bare.ByteCursor, x: MountPluginDescriptor): void {
    bare.writeString(bc, x.id)
    writeJsonUtf8(bc, x.config)
}

export type MountDescriptor = {
    readonly guestPath: string
    readonly readOnly: boolean
    readonly plugin: MountPluginDescriptor
}

export function readMountDescriptor(bc: bare.ByteCursor): MountDescriptor {
    return {
        guestPath: bare.readString(bc),
        readOnly: bare.readBool(bc),
        plugin: readMountPluginDescriptor(bc),
    }
}

export function writeMountDescriptor(bc: bare.ByteCursor, x: MountDescriptor): void {
    bare.writeString(bc, x.guestPath)
    bare.writeBool(bc, x.readOnly)
    writeMountPluginDescriptor(bc, x.plugin)
}

export type SoftwareDescriptor = {
    readonly packageName: string
    readonly root: string
}

export function readSoftwareDescriptor(bc: bare.ByteCursor): SoftwareDescriptor {
    return {
        packageName: bare.readString(bc),
        root: bare.readString(bc),
    }
}

export function writeSoftwareDescriptor(bc: bare.ByteCursor, x: SoftwareDescriptor): void {
    bare.writeString(bc, x.packageName)
    bare.writeString(bc, x.root)
}

export type ProjectedModuleDescriptor = {
    readonly packageName: string
    readonly entrypoint: string
}

export function readProjectedModuleDescriptor(bc: bare.ByteCursor): ProjectedModuleDescriptor {
    return {
        packageName: bare.readString(bc),
        entrypoint: bare.readString(bc),
    }
}

export function writeProjectedModuleDescriptor(bc: bare.ByteCursor, x: ProjectedModuleDescriptor): void {
    bare.writeString(bc, x.packageName)
    bare.writeString(bc, x.entrypoint)
}

export enum WasmPermissionTier {
    Full = "Full",
    ReadWrite = "ReadWrite",
    ReadOnly = "ReadOnly",
    Isolated = "Isolated",
}

export function readWasmPermissionTier(bc: bare.ByteCursor): WasmPermissionTier {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return WasmPermissionTier.Full
        case 1:
            return WasmPermissionTier.ReadWrite
        case 2:
            return WasmPermissionTier.ReadOnly
        case 3:
            return WasmPermissionTier.Isolated
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeWasmPermissionTier(bc: bare.ByteCursor, x: WasmPermissionTier): void {
    switch (x) {
        case WasmPermissionTier.Full: {
            bare.writeU8(bc, 0)
            break
        }
        case WasmPermissionTier.ReadWrite: {
            bare.writeU8(bc, 1)
            break
        }
        case WasmPermissionTier.ReadOnly: {
            bare.writeU8(bc, 2)
            break
        }
        case WasmPermissionTier.Isolated: {
            bare.writeU8(bc, 3)
            break
        }
    }
}

/**
 * agentOS package descriptor. The sidecar reads `agentos-package.json`, projects
 * the self-contained package read-only under `<packagesMountAt>/<manifest.name>/<version>`,
 * and links its `bin/` commands onto $PATH. `tar`/`dir` are trusted host paths
 * (the client configures its own VM); the sidecar is the host-side TCB that builds
 * the staging tree. New clients send `tar`; `dir` remains accepted for local tests
 * and transition fixtures.
 */
export type PackageDescriptor = {
    readonly dir: string | null
    readonly tar: string | null
}

export function readPackageDescriptor(bc: bare.ByteCursor): PackageDescriptor {
    return {
        dir: read0(bc),
        tar: read0(bc),
    }
}

export function writePackageDescriptor(bc: bare.ByteCursor, x: PackageDescriptor): void {
    write0(bc, x.dir)
    write0(bc, x.tar)
}

export type AgentosProjectedAgent = {
    readonly id: string
    readonly acpEntrypoint: string
    readonly adapterEntrypoint: string
}

export function readAgentosProjectedAgent(bc: bare.ByteCursor): AgentosProjectedAgent {
    return {
        id: bare.readString(bc),
        acpEntrypoint: bare.readString(bc),
        adapterEntrypoint: bare.readString(bc),
    }
}

export function writeAgentosProjectedAgent(bc: bare.ByteCursor, x: AgentosProjectedAgent): void {
    bare.writeString(bc, x.id)
    bare.writeString(bc, x.acpEntrypoint)
    bare.writeString(bc, x.adapterEntrypoint)
}

export type LinkPackageRequest = {
    readonly package: PackageDescriptor
}

export function readLinkPackageRequest(bc: bare.ByteCursor): LinkPackageRequest {
    return {
        package: readPackageDescriptor(bc),
    }
}

export function writeLinkPackageRequest(bc: bare.ByteCursor, x: LinkPackageRequest): void {
    writePackageDescriptor(bc, x.package)
}

export type PackageCommands = {
    readonly packageName: string
    readonly commands: readonly string[]
}

export function readPackageCommands(bc: bare.ByteCursor): PackageCommands {
    return {
        packageName: bare.readString(bc),
        commands: read6(bc),
    }
}

export function writePackageCommands(bc: bare.ByteCursor, x: PackageCommands): void {
    bare.writeString(bc, x.packageName)
    write6(bc, x.commands)
}

export type ProvidedCommandsRequest = null

function read12(bc: bare.ByteCursor): readonly PackageCommands[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readPackageCommands(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readPackageCommands(bc)
    }
    return result
}

function write12(bc: bare.ByteCursor, x: readonly PackageCommands[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writePackageCommands(bc, x[i])
    }
}

export type ProvidedCommandsResponse = {
    readonly packages: readonly PackageCommands[]
}

export function readProvidedCommandsResponse(bc: bare.ByteCursor): ProvidedCommandsResponse {
    return {
        packages: read12(bc),
    }
}

export function writeProvidedCommandsResponse(bc: bare.ByteCursor, x: ProvidedCommandsResponse): void {
    write12(bc, x.packages)
}

export type ProjectedCommand = {
    readonly name: string
    readonly guestPath: string
}

export function readProjectedCommand(bc: bare.ByteCursor): ProjectedCommand {
    return {
        name: bare.readString(bc),
        guestPath: bare.readString(bc),
    }
}

export function writeProjectedCommand(bc: bare.ByteCursor, x: ProjectedCommand): void {
    bare.writeString(bc, x.name)
    bare.writeString(bc, x.guestPath)
}

function read13(bc: bare.ByteCursor): readonly ProjectedCommand[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readProjectedCommand(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readProjectedCommand(bc)
    }
    return result
}

function write13(bc: bare.ByteCursor, x: readonly ProjectedCommand[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeProjectedCommand(bc, x[i])
    }
}

function read14(bc: bare.ByteCursor): readonly AgentosProjectedAgent[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readAgentosProjectedAgent(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readAgentosProjectedAgent(bc)
    }
    return result
}

function write14(bc: bare.ByteCursor, x: readonly AgentosProjectedAgent[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeAgentosProjectedAgent(bc, x[i])
    }
}

export type PackageLinkedResponse = {
    readonly projectedCommands: readonly ProjectedCommand[]
    readonly agents: readonly AgentosProjectedAgent[]
}

export function readPackageLinkedResponse(bc: bare.ByteCursor): PackageLinkedResponse {
    return {
        projectedCommands: read13(bc),
        agents: read14(bc),
    }
}

export function writePackageLinkedResponse(bc: bare.ByteCursor, x: PackageLinkedResponse): void {
    write13(bc, x.projectedCommands)
    write14(bc, x.agents)
}

function read15(bc: bare.ByteCursor): readonly MountDescriptor[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readMountDescriptor(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readMountDescriptor(bc)
    }
    return result
}

function write15(bc: bare.ByteCursor, x: readonly MountDescriptor[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeMountDescriptor(bc, x[i])
    }
}

function read16(bc: bare.ByteCursor): readonly SoftwareDescriptor[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readSoftwareDescriptor(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readSoftwareDescriptor(bc)
    }
    return result
}

function write16(bc: bare.ByteCursor, x: readonly SoftwareDescriptor[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeSoftwareDescriptor(bc, x[i])
    }
}

function read17(bc: bare.ByteCursor): PermissionsPolicy | null {
    return bare.readBool(bc) ? readPermissionsPolicy(bc) : null
}

function write17(bc: bare.ByteCursor, x: PermissionsPolicy | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writePermissionsPolicy(bc, x)
    }
}

function read18(bc: bare.ByteCursor): readonly ProjectedModuleDescriptor[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readProjectedModuleDescriptor(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readProjectedModuleDescriptor(bc)
    }
    return result
}

function write18(bc: bare.ByteCursor, x: readonly ProjectedModuleDescriptor[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeProjectedModuleDescriptor(bc, x[i])
    }
}

function read19(bc: bare.ByteCursor): ReadonlyMap<string, WasmPermissionTier> {
    const len = bare.readUintSafe(bc)
    const result = new Map<string, WasmPermissionTier>()
    for (let i = 0; i < len; i++) {
        const offset = bc.offset
        const key = bare.readString(bc)
        if (result.has(key)) {
            bc.offset = offset
            throw new bare.BareError(offset, "duplicated key")
        }
        result.set(key, readWasmPermissionTier(bc))
    }
    return result
}

function write19(bc: bare.ByteCursor, x: ReadonlyMap<string, WasmPermissionTier>): void {
    bare.writeUintSafe(bc, x.size)
    for (const kv of x) {
        bare.writeString(bc, kv[0])
        writeWasmPermissionTier(bc, kv[1])
    }
}

function read20(bc: bare.ByteCursor): readonly PackageDescriptor[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readPackageDescriptor(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readPackageDescriptor(bc)
    }
    return result
}

function write20(bc: bare.ByteCursor, x: readonly PackageDescriptor[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writePackageDescriptor(bc, x[i])
    }
}

export type ConfigureVmRequest = {
    readonly mounts: readonly MountDescriptor[]
    readonly software: readonly SoftwareDescriptor[]
    readonly permissions: PermissionsPolicy | null
    readonly moduleAccessCwd: string | null
    readonly instructions: readonly string[]
    readonly projectedModules: readonly ProjectedModuleDescriptor[]
    readonly commandPermissions: ReadonlyMap<string, WasmPermissionTier>
    readonly loopbackExemptPorts: Uint16Array
    readonly packages: readonly PackageDescriptor[]
    readonly packagesMountAt: string
    readonly bootstrapCommands: readonly string[]
    readonly toolShimCommands: readonly string[]
}

export function readConfigureVmRequest(bc: bare.ByteCursor): ConfigureVmRequest {
    return {
        mounts: read15(bc),
        software: read16(bc),
        permissions: read17(bc),
        moduleAccessCwd: read0(bc),
        instructions: read6(bc),
        projectedModules: read18(bc),
        commandPermissions: read19(bc),
        loopbackExemptPorts: bare.readU16Array(bc),
        packages: read20(bc),
        packagesMountAt: bare.readString(bc),
        bootstrapCommands: read6(bc),
        toolShimCommands: read6(bc),
    }
}

export function writeConfigureVmRequest(bc: bare.ByteCursor, x: ConfigureVmRequest): void {
    write15(bc, x.mounts)
    write16(bc, x.software)
    write17(bc, x.permissions)
    write0(bc, x.moduleAccessCwd)
    write6(bc, x.instructions)
    write18(bc, x.projectedModules)
    write19(bc, x.commandPermissions)
    bare.writeU16Array(bc, x.loopbackExemptPorts)
    write20(bc, x.packages)
    bare.writeString(bc, x.packagesMountAt)
    write6(bc, x.bootstrapCommands)
    write6(bc, x.toolShimCommands)
}

export type RegisteredHostCallbackExample = {
    readonly description: string
    readonly input: JsonUtf8
}

export function readRegisteredHostCallbackExample(bc: bare.ByteCursor): RegisteredHostCallbackExample {
    return {
        description: bare.readString(bc),
        input: readJsonUtf8(bc),
    }
}

export function writeRegisteredHostCallbackExample(bc: bare.ByteCursor, x: RegisteredHostCallbackExample): void {
    bare.writeString(bc, x.description)
    writeJsonUtf8(bc, x.input)
}

function read21(bc: bare.ByteCursor): u64 | null {
    return bare.readBool(bc) ? bare.readU64(bc) : null
}

function write21(bc: bare.ByteCursor, x: u64 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeU64(bc, x)
    }
}

function read22(bc: bare.ByteCursor): readonly RegisteredHostCallbackExample[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readRegisteredHostCallbackExample(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readRegisteredHostCallbackExample(bc)
    }
    return result
}

function write22(bc: bare.ByteCursor, x: readonly RegisteredHostCallbackExample[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeRegisteredHostCallbackExample(bc, x[i])
    }
}

export type RegisteredHostCallbackDefinition = {
    readonly description: string
    readonly inputSchema: JsonUtf8
    readonly timeoutMs: u64 | null
    readonly examples: readonly RegisteredHostCallbackExample[]
}

export function readRegisteredHostCallbackDefinition(bc: bare.ByteCursor): RegisteredHostCallbackDefinition {
    return {
        description: bare.readString(bc),
        inputSchema: readJsonUtf8(bc),
        timeoutMs: read21(bc),
        examples: read22(bc),
    }
}

export function writeRegisteredHostCallbackDefinition(bc: bare.ByteCursor, x: RegisteredHostCallbackDefinition): void {
    bare.writeString(bc, x.description)
    writeJsonUtf8(bc, x.inputSchema)
    write21(bc, x.timeoutMs)
    write22(bc, x.examples)
}

function read23(bc: bare.ByteCursor): ReadonlyMap<string, RegisteredHostCallbackDefinition> {
    const len = bare.readUintSafe(bc)
    const result = new Map<string, RegisteredHostCallbackDefinition>()
    for (let i = 0; i < len; i++) {
        const offset = bc.offset
        const key = bare.readString(bc)
        if (result.has(key)) {
            bc.offset = offset
            throw new bare.BareError(offset, "duplicated key")
        }
        result.set(key, readRegisteredHostCallbackDefinition(bc))
    }
    return result
}

function write23(bc: bare.ByteCursor, x: ReadonlyMap<string, RegisteredHostCallbackDefinition>): void {
    bare.writeUintSafe(bc, x.size)
    for (const kv of x) {
        bare.writeString(bc, kv[0])
        writeRegisteredHostCallbackDefinition(bc, kv[1])
    }
}

export type RegisterHostCallbacksRequest = {
    readonly name: string
    readonly description: string
    readonly commandAliases: readonly string[]
    readonly registryCommandAliases: readonly string[]
    readonly callbacks: ReadonlyMap<string, RegisteredHostCallbackDefinition>
}

export function readRegisterHostCallbacksRequest(bc: bare.ByteCursor): RegisterHostCallbacksRequest {
    return {
        name: bare.readString(bc),
        description: bare.readString(bc),
        commandAliases: read6(bc),
        registryCommandAliases: read6(bc),
        callbacks: read23(bc),
    }
}

export function writeRegisterHostCallbacksRequest(bc: bare.ByteCursor, x: RegisterHostCallbacksRequest): void {
    bare.writeString(bc, x.name)
    bare.writeString(bc, x.description)
    write6(bc, x.commandAliases)
    write6(bc, x.registryCommandAliases)
    write23(bc, x.callbacks)
}

export type CreateLayerRequest = null

export type SealLayerRequest = {
    readonly layerId: string
}

export function readSealLayerRequest(bc: bare.ByteCursor): SealLayerRequest {
    return {
        layerId: bare.readString(bc),
    }
}

export function writeSealLayerRequest(bc: bare.ByteCursor, x: SealLayerRequest): void {
    bare.writeString(bc, x.layerId)
}

export type ImportSnapshotRequest = {
    readonly entries: readonly RootFilesystemEntry[]
}

export function readImportSnapshotRequest(bc: bare.ByteCursor): ImportSnapshotRequest {
    return {
        entries: read4(bc),
    }
}

export function writeImportSnapshotRequest(bc: bare.ByteCursor, x: ImportSnapshotRequest): void {
    write4(bc, x.entries)
}

export type ExportSnapshotRequest = {
    readonly layerId: string
}

export function readExportSnapshotRequest(bc: bare.ByteCursor): ExportSnapshotRequest {
    return {
        layerId: bare.readString(bc),
    }
}

export function writeExportSnapshotRequest(bc: bare.ByteCursor, x: ExportSnapshotRequest): void {
    bare.writeString(bc, x.layerId)
}

export type CreateOverlayRequest = {
    readonly mode: RootFilesystemMode
    readonly upperLayerId: string | null
    readonly lowerLayerIds: readonly string[]
}

export function readCreateOverlayRequest(bc: bare.ByteCursor): CreateOverlayRequest {
    return {
        mode: readRootFilesystemMode(bc),
        upperLayerId: read0(bc),
        lowerLayerIds: read6(bc),
    }
}

export function writeCreateOverlayRequest(bc: bare.ByteCursor, x: CreateOverlayRequest): void {
    writeRootFilesystemMode(bc, x.mode)
    write0(bc, x.upperLayerId)
    write6(bc, x.lowerLayerIds)
}

export enum GuestFilesystemOperation {
    ReadFile = "ReadFile",
    WriteFile = "WriteFile",
    CreateDir = "CreateDir",
    Mkdir = "Mkdir",
    Exists = "Exists",
    Stat = "Stat",
    Lstat = "Lstat",
    ReadDir = "ReadDir",
    ReadDirRecursive = "ReadDirRecursive",
    RemoveFile = "RemoveFile",
    RemoveDir = "RemoveDir",
    Remove = "Remove",
    Copy = "Copy",
    Move = "Move",
    Rename = "Rename",
    Realpath = "Realpath",
    Symlink = "Symlink",
    ReadLink = "ReadLink",
    Link = "Link",
    Chmod = "Chmod",
    Chown = "Chown",
    Utimes = "Utimes",
    Truncate = "Truncate",
    Pread = "Pread",
    Pwrite = "Pwrite",
}

export function readGuestFilesystemOperation(bc: bare.ByteCursor): GuestFilesystemOperation {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return GuestFilesystemOperation.ReadFile
        case 1:
            return GuestFilesystemOperation.WriteFile
        case 2:
            return GuestFilesystemOperation.CreateDir
        case 3:
            return GuestFilesystemOperation.Mkdir
        case 4:
            return GuestFilesystemOperation.Exists
        case 5:
            return GuestFilesystemOperation.Stat
        case 6:
            return GuestFilesystemOperation.Lstat
        case 7:
            return GuestFilesystemOperation.ReadDir
        case 8:
            return GuestFilesystemOperation.ReadDirRecursive
        case 9:
            return GuestFilesystemOperation.RemoveFile
        case 10:
            return GuestFilesystemOperation.RemoveDir
        case 11:
            return GuestFilesystemOperation.Remove
        case 12:
            return GuestFilesystemOperation.Copy
        case 13:
            return GuestFilesystemOperation.Move
        case 14:
            return GuestFilesystemOperation.Rename
        case 15:
            return GuestFilesystemOperation.Realpath
        case 16:
            return GuestFilesystemOperation.Symlink
        case 17:
            return GuestFilesystemOperation.ReadLink
        case 18:
            return GuestFilesystemOperation.Link
        case 19:
            return GuestFilesystemOperation.Chmod
        case 20:
            return GuestFilesystemOperation.Chown
        case 21:
            return GuestFilesystemOperation.Utimes
        case 22:
            return GuestFilesystemOperation.Truncate
        case 23:
            return GuestFilesystemOperation.Pread
        case 24:
            return GuestFilesystemOperation.Pwrite
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeGuestFilesystemOperation(bc: bare.ByteCursor, x: GuestFilesystemOperation): void {
    switch (x) {
        case GuestFilesystemOperation.ReadFile: {
            bare.writeU8(bc, 0)
            break
        }
        case GuestFilesystemOperation.WriteFile: {
            bare.writeU8(bc, 1)
            break
        }
        case GuestFilesystemOperation.CreateDir: {
            bare.writeU8(bc, 2)
            break
        }
        case GuestFilesystemOperation.Mkdir: {
            bare.writeU8(bc, 3)
            break
        }
        case GuestFilesystemOperation.Exists: {
            bare.writeU8(bc, 4)
            break
        }
        case GuestFilesystemOperation.Stat: {
            bare.writeU8(bc, 5)
            break
        }
        case GuestFilesystemOperation.Lstat: {
            bare.writeU8(bc, 6)
            break
        }
        case GuestFilesystemOperation.ReadDir: {
            bare.writeU8(bc, 7)
            break
        }
        case GuestFilesystemOperation.ReadDirRecursive: {
            bare.writeU8(bc, 8)
            break
        }
        case GuestFilesystemOperation.RemoveFile: {
            bare.writeU8(bc, 9)
            break
        }
        case GuestFilesystemOperation.RemoveDir: {
            bare.writeU8(bc, 10)
            break
        }
        case GuestFilesystemOperation.Remove: {
            bare.writeU8(bc, 11)
            break
        }
        case GuestFilesystemOperation.Copy: {
            bare.writeU8(bc, 12)
            break
        }
        case GuestFilesystemOperation.Move: {
            bare.writeU8(bc, 13)
            break
        }
        case GuestFilesystemOperation.Rename: {
            bare.writeU8(bc, 14)
            break
        }
        case GuestFilesystemOperation.Realpath: {
            bare.writeU8(bc, 15)
            break
        }
        case GuestFilesystemOperation.Symlink: {
            bare.writeU8(bc, 16)
            break
        }
        case GuestFilesystemOperation.ReadLink: {
            bare.writeU8(bc, 17)
            break
        }
        case GuestFilesystemOperation.Link: {
            bare.writeU8(bc, 18)
            break
        }
        case GuestFilesystemOperation.Chmod: {
            bare.writeU8(bc, 19)
            break
        }
        case GuestFilesystemOperation.Chown: {
            bare.writeU8(bc, 20)
            break
        }
        case GuestFilesystemOperation.Utimes: {
            bare.writeU8(bc, 21)
            break
        }
        case GuestFilesystemOperation.Truncate: {
            bare.writeU8(bc, 22)
            break
        }
        case GuestFilesystemOperation.Pread: {
            bare.writeU8(bc, 23)
            break
        }
        case GuestFilesystemOperation.Pwrite: {
            bare.writeU8(bc, 24)
            break
        }
    }
}

export type GuestFilesystemCallRequest = {
    readonly operation: GuestFilesystemOperation
    readonly path: string
    readonly destinationPath: string | null
    readonly target: string | null
    readonly content: string | null
    readonly encoding: RootFilesystemEntryEncoding | null
    readonly recursive: boolean
    readonly maxDepth: u32 | null
    readonly mode: u32 | null
    readonly uid: u32 | null
    readonly gid: u32 | null
    readonly atimeMs: u64 | null
    readonly mtimeMs: u64 | null
    readonly len: u64 | null
    readonly offset: u64 | null
}

export function readGuestFilesystemCallRequest(bc: bare.ByteCursor): GuestFilesystemCallRequest {
    return {
        operation: readGuestFilesystemOperation(bc),
        path: bare.readString(bc),
        destinationPath: read0(bc),
        target: read0(bc),
        content: read0(bc),
        encoding: read3(bc),
        recursive: bare.readBool(bc),
        maxDepth: read2(bc),
        mode: read2(bc),
        uid: read2(bc),
        gid: read2(bc),
        atimeMs: read21(bc),
        mtimeMs: read21(bc),
        len: read21(bc),
        offset: read21(bc),
    }
}

export function writeGuestFilesystemCallRequest(bc: bare.ByteCursor, x: GuestFilesystemCallRequest): void {
    writeGuestFilesystemOperation(bc, x.operation)
    bare.writeString(bc, x.path)
    write0(bc, x.destinationPath)
    write0(bc, x.target)
    write0(bc, x.content)
    write3(bc, x.encoding)
    bare.writeBool(bc, x.recursive)
    write2(bc, x.maxDepth)
    write2(bc, x.mode)
    write2(bc, x.uid)
    write2(bc, x.gid)
    write21(bc, x.atimeMs)
    write21(bc, x.mtimeMs)
    write21(bc, x.len)
    write21(bc, x.offset)
}

export type GuestKernelCallRequest = {
    readonly executionId: string
    readonly operation: string
    readonly payload: ArrayBuffer
}

export function readGuestKernelCallRequest(bc: bare.ByteCursor): GuestKernelCallRequest {
    return {
        executionId: bare.readString(bc),
        operation: bare.readString(bc),
        payload: bare.readData(bc),
    }
}

export function writeGuestKernelCallRequest(bc: bare.ByteCursor, x: GuestKernelCallRequest): void {
    bare.writeString(bc, x.executionId)
    bare.writeString(bc, x.operation)
    bare.writeData(bc, x.payload)
}

export type SnapshotRootFilesystemRequest = null

function read24(bc: bare.ByteCursor): GuestRuntimeKind | null {
    return bare.readBool(bc) ? readGuestRuntimeKind(bc) : null
}

function write24(bc: bare.ByteCursor, x: GuestRuntimeKind | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeGuestRuntimeKind(bc, x)
    }
}

function read25(bc: bare.ByteCursor): WasmPermissionTier | null {
    return bare.readBool(bc) ? readWasmPermissionTier(bc) : null
}

function write25(bc: bare.ByteCursor, x: WasmPermissionTier | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeWasmPermissionTier(bc, x)
    }
}

export type ExecuteRequest = {
    readonly processId: string
    readonly command: string | null
    readonly runtime: GuestRuntimeKind | null
    readonly entrypoint: string | null
    readonly args: readonly string[]
    readonly env: ReadonlyMap<string, string>
    readonly cwd: string | null
    readonly wasmPermissionTier: WasmPermissionTier | null
}

export function readExecuteRequest(bc: bare.ByteCursor): ExecuteRequest {
    return {
        processId: bare.readString(bc),
        command: read0(bc),
        runtime: read24(bc),
        entrypoint: read0(bc),
        args: read6(bc),
        env: read1(bc),
        cwd: read0(bc),
        wasmPermissionTier: read25(bc),
    }
}

export function writeExecuteRequest(bc: bare.ByteCursor, x: ExecuteRequest): void {
    bare.writeString(bc, x.processId)
    write0(bc, x.command)
    write24(bc, x.runtime)
    write0(bc, x.entrypoint)
    write6(bc, x.args)
    write1(bc, x.env)
    write0(bc, x.cwd)
    write25(bc, x.wasmPermissionTier)
}

export type WriteStdinRequest = {
    readonly processId: string
    readonly chunk: ArrayBuffer
}

export function readWriteStdinRequest(bc: bare.ByteCursor): WriteStdinRequest {
    return {
        processId: bare.readString(bc),
        chunk: bare.readData(bc),
    }
}

export function writeWriteStdinRequest(bc: bare.ByteCursor, x: WriteStdinRequest): void {
    bare.writeString(bc, x.processId)
    bare.writeData(bc, x.chunk)
}

export type ResizePtyRequest = {
    readonly processId: string
    readonly cols: u16
    readonly rows: u16
}

export function readResizePtyRequest(bc: bare.ByteCursor): ResizePtyRequest {
    return {
        processId: bare.readString(bc),
        cols: bare.readU16(bc),
        rows: bare.readU16(bc),
    }
}

export function writeResizePtyRequest(bc: bare.ByteCursor, x: ResizePtyRequest): void {
    bare.writeString(bc, x.processId)
    bare.writeU16(bc, x.cols)
    bare.writeU16(bc, x.rows)
}

export type CloseStdinRequest = {
    readonly processId: string
}

export function readCloseStdinRequest(bc: bare.ByteCursor): CloseStdinRequest {
    return {
        processId: bare.readString(bc),
    }
}

export function writeCloseStdinRequest(bc: bare.ByteCursor, x: CloseStdinRequest): void {
    bare.writeString(bc, x.processId)
}

export type KillProcessRequest = {
    readonly processId: string
    readonly signal: string
}

export function readKillProcessRequest(bc: bare.ByteCursor): KillProcessRequest {
    return {
        processId: bare.readString(bc),
        signal: bare.readString(bc),
    }
}

export function writeKillProcessRequest(bc: bare.ByteCursor, x: KillProcessRequest): void {
    bare.writeString(bc, x.processId)
    bare.writeString(bc, x.signal)
}

export type GetProcessSnapshotRequest = null

export type GetResourceSnapshotRequest = null

function read26(bc: bare.ByteCursor): u16 | null {
    return bare.readBool(bc) ? bare.readU16(bc) : null
}

function write26(bc: bare.ByteCursor, x: u16 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeU16(bc, x)
    }
}

export type FindListenerRequest = {
    readonly host: string | null
    readonly port: u16 | null
    readonly path: string | null
}

export function readFindListenerRequest(bc: bare.ByteCursor): FindListenerRequest {
    return {
        host: read0(bc),
        port: read26(bc),
        path: read0(bc),
    }
}

export function writeFindListenerRequest(bc: bare.ByteCursor, x: FindListenerRequest): void {
    write0(bc, x.host)
    write26(bc, x.port)
    write0(bc, x.path)
}

export type FindBoundUdpRequest = {
    readonly host: string | null
    readonly port: u16 | null
}

export function readFindBoundUdpRequest(bc: bare.ByteCursor): FindBoundUdpRequest {
    return {
        host: read0(bc),
        port: read26(bc),
    }
}

export function writeFindBoundUdpRequest(bc: bare.ByteCursor, x: FindBoundUdpRequest): void {
    write0(bc, x.host)
    write26(bc, x.port)
}

export type GetSignalStateRequest = {
    readonly processId: string
}

export function readGetSignalStateRequest(bc: bare.ByteCursor): GetSignalStateRequest {
    return {
        processId: bare.readString(bc),
    }
}

export function writeGetSignalStateRequest(bc: bare.ByteCursor, x: GetSignalStateRequest): void {
    bare.writeString(bc, x.processId)
}

export type GetZombieTimerCountRequest = null

export enum FilesystemOperation {
    Read = "Read",
    Write = "Write",
    Stat = "Stat",
    ReadDir = "ReadDir",
    Mkdir = "Mkdir",
    Remove = "Remove",
    Rename = "Rename",
}

export function readFilesystemOperation(bc: bare.ByteCursor): FilesystemOperation {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return FilesystemOperation.Read
        case 1:
            return FilesystemOperation.Write
        case 2:
            return FilesystemOperation.Stat
        case 3:
            return FilesystemOperation.ReadDir
        case 4:
            return FilesystemOperation.Mkdir
        case 5:
            return FilesystemOperation.Remove
        case 6:
            return FilesystemOperation.Rename
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeFilesystemOperation(bc: bare.ByteCursor, x: FilesystemOperation): void {
    switch (x) {
        case FilesystemOperation.Read: {
            bare.writeU8(bc, 0)
            break
        }
        case FilesystemOperation.Write: {
            bare.writeU8(bc, 1)
            break
        }
        case FilesystemOperation.Stat: {
            bare.writeU8(bc, 2)
            break
        }
        case FilesystemOperation.ReadDir: {
            bare.writeU8(bc, 3)
            break
        }
        case FilesystemOperation.Mkdir: {
            bare.writeU8(bc, 4)
            break
        }
        case FilesystemOperation.Remove: {
            bare.writeU8(bc, 5)
            break
        }
        case FilesystemOperation.Rename: {
            bare.writeU8(bc, 6)
            break
        }
    }
}

export type HostFilesystemCallRequest = {
    readonly operation: FilesystemOperation
    readonly path: string
    readonly payloadSizeBytes: u64
}

export function readHostFilesystemCallRequest(bc: bare.ByteCursor): HostFilesystemCallRequest {
    return {
        operation: readFilesystemOperation(bc),
        path: bare.readString(bc),
        payloadSizeBytes: bare.readU64(bc),
    }
}

export function writeHostFilesystemCallRequest(bc: bare.ByteCursor, x: HostFilesystemCallRequest): void {
    writeFilesystemOperation(bc, x.operation)
    bare.writeString(bc, x.path)
    bare.writeU64(bc, x.payloadSizeBytes)
}

export type PersistenceLoadRequest = {
    readonly key: string
}

export function readPersistenceLoadRequest(bc: bare.ByteCursor): PersistenceLoadRequest {
    return {
        key: bare.readString(bc),
    }
}

export function writePersistenceLoadRequest(bc: bare.ByteCursor, x: PersistenceLoadRequest): void {
    bare.writeString(bc, x.key)
}

export type PersistenceFlushRequest = {
    readonly key: string
    readonly payloadSizeBytes: u64
}

export function readPersistenceFlushRequest(bc: bare.ByteCursor): PersistenceFlushRequest {
    return {
        key: bare.readString(bc),
        payloadSizeBytes: bare.readU64(bc),
    }
}

export function writePersistenceFlushRequest(bc: bare.ByteCursor, x: PersistenceFlushRequest): void {
    bare.writeString(bc, x.key)
    bare.writeU64(bc, x.payloadSizeBytes)
}

export type VmFetchRequest = {
    readonly port: u16
    readonly method: string
    readonly path: string
    readonly headersJson: string
    readonly body: string | null
}

export function readVmFetchRequest(bc: bare.ByteCursor): VmFetchRequest {
    return {
        port: bare.readU16(bc),
        method: bare.readString(bc),
        path: bare.readString(bc),
        headersJson: bare.readString(bc),
        body: read0(bc),
    }
}

export function writeVmFetchRequest(bc: bare.ByteCursor, x: VmFetchRequest): void {
    bare.writeU16(bc, x.port)
    bare.writeString(bc, x.method)
    bare.writeString(bc, x.path)
    bare.writeString(bc, x.headersJson)
    write0(bc, x.body)
}

export type RequestPayload =
    | { readonly tag: "AuthenticateRequest"; readonly val: AuthenticateRequest }
    | { readonly tag: "OpenSessionRequest"; readonly val: OpenSessionRequest }
    | { readonly tag: "CreateVmRequest"; readonly val: CreateVmRequest }
    | { readonly tag: "DisposeVmRequest"; readonly val: DisposeVmRequest }
    | { readonly tag: "BootstrapRootFilesystemRequest"; readonly val: BootstrapRootFilesystemRequest }
    | { readonly tag: "ConfigureVmRequest"; readonly val: ConfigureVmRequest }
    | { readonly tag: "RegisterHostCallbacksRequest"; readonly val: RegisterHostCallbacksRequest }
    | { readonly tag: "CreateLayerRequest"; readonly val: CreateLayerRequest }
    | { readonly tag: "SealLayerRequest"; readonly val: SealLayerRequest }
    | { readonly tag: "ImportSnapshotRequest"; readonly val: ImportSnapshotRequest }
    | { readonly tag: "ExportSnapshotRequest"; readonly val: ExportSnapshotRequest }
    | { readonly tag: "CreateOverlayRequest"; readonly val: CreateOverlayRequest }
    | { readonly tag: "GuestFilesystemCallRequest"; readonly val: GuestFilesystemCallRequest }
    | { readonly tag: "SnapshotRootFilesystemRequest"; readonly val: SnapshotRootFilesystemRequest }
    | { readonly tag: "ExecuteRequest"; readonly val: ExecuteRequest }
    | { readonly tag: "WriteStdinRequest"; readonly val: WriteStdinRequest }
    | { readonly tag: "CloseStdinRequest"; readonly val: CloseStdinRequest }
    | { readonly tag: "KillProcessRequest"; readonly val: KillProcessRequest }
    | { readonly tag: "GetProcessSnapshotRequest"; readonly val: GetProcessSnapshotRequest }
    | { readonly tag: "FindListenerRequest"; readonly val: FindListenerRequest }
    | { readonly tag: "FindBoundUdpRequest"; readonly val: FindBoundUdpRequest }
    | { readonly tag: "GetSignalStateRequest"; readonly val: GetSignalStateRequest }
    | { readonly tag: "GetZombieTimerCountRequest"; readonly val: GetZombieTimerCountRequest }
    | { readonly tag: "HostFilesystemCallRequest"; readonly val: HostFilesystemCallRequest }
    | { readonly tag: "PersistenceLoadRequest"; readonly val: PersistenceLoadRequest }
    | { readonly tag: "PersistenceFlushRequest"; readonly val: PersistenceFlushRequest }
    | { readonly tag: "VmFetchRequest"; readonly val: VmFetchRequest }
    | { readonly tag: "ExtEnvelope"; readonly val: ExtEnvelope }
    | { readonly tag: "GuestKernelCallRequest"; readonly val: GuestKernelCallRequest }
    | { readonly tag: "ResizePtyRequest"; readonly val: ResizePtyRequest }
    | { readonly tag: "GetResourceSnapshotRequest"; readonly val: GetResourceSnapshotRequest }
    | { readonly tag: "LinkPackageRequest"; readonly val: LinkPackageRequest }
    | { readonly tag: "ProvidedCommandsRequest"; readonly val: ProvidedCommandsRequest }

export function readRequestPayload(bc: bare.ByteCursor): RequestPayload {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AuthenticateRequest", val: readAuthenticateRequest(bc) }
        case 1:
            return { tag: "OpenSessionRequest", val: readOpenSessionRequest(bc) }
        case 2:
            return { tag: "CreateVmRequest", val: readCreateVmRequest(bc) }
        case 3:
            return { tag: "DisposeVmRequest", val: readDisposeVmRequest(bc) }
        case 4:
            return { tag: "BootstrapRootFilesystemRequest", val: readBootstrapRootFilesystemRequest(bc) }
        case 5:
            return { tag: "ConfigureVmRequest", val: readConfigureVmRequest(bc) }
        case 6:
            return { tag: "RegisterHostCallbacksRequest", val: readRegisterHostCallbacksRequest(bc) }
        case 7:
            return { tag: "CreateLayerRequest", val: null }
        case 8:
            return { tag: "SealLayerRequest", val: readSealLayerRequest(bc) }
        case 9:
            return { tag: "ImportSnapshotRequest", val: readImportSnapshotRequest(bc) }
        case 10:
            return { tag: "ExportSnapshotRequest", val: readExportSnapshotRequest(bc) }
        case 11:
            return { tag: "CreateOverlayRequest", val: readCreateOverlayRequest(bc) }
        case 12:
            return { tag: "GuestFilesystemCallRequest", val: readGuestFilesystemCallRequest(bc) }
        case 13:
            return { tag: "SnapshotRootFilesystemRequest", val: null }
        case 14:
            return { tag: "ExecuteRequest", val: readExecuteRequest(bc) }
        case 15:
            return { tag: "WriteStdinRequest", val: readWriteStdinRequest(bc) }
        case 16:
            return { tag: "CloseStdinRequest", val: readCloseStdinRequest(bc) }
        case 17:
            return { tag: "KillProcessRequest", val: readKillProcessRequest(bc) }
        case 18:
            return { tag: "GetProcessSnapshotRequest", val: null }
        case 19:
            return { tag: "FindListenerRequest", val: readFindListenerRequest(bc) }
        case 20:
            return { tag: "FindBoundUdpRequest", val: readFindBoundUdpRequest(bc) }
        case 21:
            return { tag: "GetSignalStateRequest", val: readGetSignalStateRequest(bc) }
        case 22:
            return { tag: "GetZombieTimerCountRequest", val: null }
        case 23:
            return { tag: "HostFilesystemCallRequest", val: readHostFilesystemCallRequest(bc) }
        case 24:
            return { tag: "PersistenceLoadRequest", val: readPersistenceLoadRequest(bc) }
        case 25:
            return { tag: "PersistenceFlushRequest", val: readPersistenceFlushRequest(bc) }
        case 26:
            return { tag: "VmFetchRequest", val: readVmFetchRequest(bc) }
        case 27:
            return { tag: "ExtEnvelope", val: readExtEnvelope(bc) }
        case 28:
            return { tag: "GuestKernelCallRequest", val: readGuestKernelCallRequest(bc) }
        case 29:
            return { tag: "ResizePtyRequest", val: readResizePtyRequest(bc) }
        case 30:
            return { tag: "GetResourceSnapshotRequest", val: null }
        case 31:
            return { tag: "LinkPackageRequest", val: readLinkPackageRequest(bc) }
        case 32:
            return { tag: "ProvidedCommandsRequest", val: null }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeRequestPayload(bc: bare.ByteCursor, x: RequestPayload): void {
    switch (x.tag) {
        case "AuthenticateRequest": {
            bare.writeU8(bc, 0)
            writeAuthenticateRequest(bc, x.val)
            break
        }
        case "OpenSessionRequest": {
            bare.writeU8(bc, 1)
            writeOpenSessionRequest(bc, x.val)
            break
        }
        case "CreateVmRequest": {
            bare.writeU8(bc, 2)
            writeCreateVmRequest(bc, x.val)
            break
        }
        case "DisposeVmRequest": {
            bare.writeU8(bc, 3)
            writeDisposeVmRequest(bc, x.val)
            break
        }
        case "BootstrapRootFilesystemRequest": {
            bare.writeU8(bc, 4)
            writeBootstrapRootFilesystemRequest(bc, x.val)
            break
        }
        case "ConfigureVmRequest": {
            bare.writeU8(bc, 5)
            writeConfigureVmRequest(bc, x.val)
            break
        }
        case "RegisterHostCallbacksRequest": {
            bare.writeU8(bc, 6)
            writeRegisterHostCallbacksRequest(bc, x.val)
            break
        }
        case "CreateLayerRequest": {
            bare.writeU8(bc, 7)
            break
        }
        case "SealLayerRequest": {
            bare.writeU8(bc, 8)
            writeSealLayerRequest(bc, x.val)
            break
        }
        case "ImportSnapshotRequest": {
            bare.writeU8(bc, 9)
            writeImportSnapshotRequest(bc, x.val)
            break
        }
        case "ExportSnapshotRequest": {
            bare.writeU8(bc, 10)
            writeExportSnapshotRequest(bc, x.val)
            break
        }
        case "CreateOverlayRequest": {
            bare.writeU8(bc, 11)
            writeCreateOverlayRequest(bc, x.val)
            break
        }
        case "GuestFilesystemCallRequest": {
            bare.writeU8(bc, 12)
            writeGuestFilesystemCallRequest(bc, x.val)
            break
        }
        case "SnapshotRootFilesystemRequest": {
            bare.writeU8(bc, 13)
            break
        }
        case "ExecuteRequest": {
            bare.writeU8(bc, 14)
            writeExecuteRequest(bc, x.val)
            break
        }
        case "WriteStdinRequest": {
            bare.writeU8(bc, 15)
            writeWriteStdinRequest(bc, x.val)
            break
        }
        case "CloseStdinRequest": {
            bare.writeU8(bc, 16)
            writeCloseStdinRequest(bc, x.val)
            break
        }
        case "KillProcessRequest": {
            bare.writeU8(bc, 17)
            writeKillProcessRequest(bc, x.val)
            break
        }
        case "GetProcessSnapshotRequest": {
            bare.writeU8(bc, 18)
            break
        }
        case "FindListenerRequest": {
            bare.writeU8(bc, 19)
            writeFindListenerRequest(bc, x.val)
            break
        }
        case "FindBoundUdpRequest": {
            bare.writeU8(bc, 20)
            writeFindBoundUdpRequest(bc, x.val)
            break
        }
        case "GetSignalStateRequest": {
            bare.writeU8(bc, 21)
            writeGetSignalStateRequest(bc, x.val)
            break
        }
        case "GetZombieTimerCountRequest": {
            bare.writeU8(bc, 22)
            break
        }
        case "HostFilesystemCallRequest": {
            bare.writeU8(bc, 23)
            writeHostFilesystemCallRequest(bc, x.val)
            break
        }
        case "PersistenceLoadRequest": {
            bare.writeU8(bc, 24)
            writePersistenceLoadRequest(bc, x.val)
            break
        }
        case "PersistenceFlushRequest": {
            bare.writeU8(bc, 25)
            writePersistenceFlushRequest(bc, x.val)
            break
        }
        case "VmFetchRequest": {
            bare.writeU8(bc, 26)
            writeVmFetchRequest(bc, x.val)
            break
        }
        case "ExtEnvelope": {
            bare.writeU8(bc, 27)
            writeExtEnvelope(bc, x.val)
            break
        }
        case "GuestKernelCallRequest": {
            bare.writeU8(bc, 28)
            writeGuestKernelCallRequest(bc, x.val)
            break
        }
        case "ResizePtyRequest": {
            bare.writeU8(bc, 29)
            writeResizePtyRequest(bc, x.val)
            break
        }
        case "GetResourceSnapshotRequest": {
            bare.writeU8(bc, 30)
            break
        }
        case "LinkPackageRequest": {
            bare.writeU8(bc, 31)
            writeLinkPackageRequest(bc, x.val)
            break
        }
        case "ProvidedCommandsRequest": {
            bare.writeU8(bc, 32)
            break
        }
    }
}

export type RequestFrame = {
    readonly schema: ProtocolSchema
    readonly requestId: RequestId
    readonly ownership: OwnershipScope
    readonly payload: RequestPayload
}

export function readRequestFrame(bc: bare.ByteCursor): RequestFrame {
    return {
        schema: readProtocolSchema(bc),
        requestId: readRequestId(bc),
        ownership: readOwnershipScope(bc),
        payload: readRequestPayload(bc),
    }
}

export function writeRequestFrame(bc: bare.ByteCursor, x: RequestFrame): void {
    writeProtocolSchema(bc, x.schema)
    writeRequestId(bc, x.requestId)
    writeOwnershipScope(bc, x.ownership)
    writeRequestPayload(bc, x.payload)
}

export type AuthenticatedResponse = {
    readonly sidecarId: string
    readonly connectionId: string
    readonly maxFrameBytes: u32
}

export function readAuthenticatedResponse(bc: bare.ByteCursor): AuthenticatedResponse {
    return {
        sidecarId: bare.readString(bc),
        connectionId: bare.readString(bc),
        maxFrameBytes: bare.readU32(bc),
    }
}

export function writeAuthenticatedResponse(bc: bare.ByteCursor, x: AuthenticatedResponse): void {
    bare.writeString(bc, x.sidecarId)
    bare.writeString(bc, x.connectionId)
    bare.writeU32(bc, x.maxFrameBytes)
}

export type SessionOpenedResponse = {
    readonly sessionId: string
    readonly ownerConnectionId: string
}

export function readSessionOpenedResponse(bc: bare.ByteCursor): SessionOpenedResponse {
    return {
        sessionId: bare.readString(bc),
        ownerConnectionId: bare.readString(bc),
    }
}

export function writeSessionOpenedResponse(bc: bare.ByteCursor, x: SessionOpenedResponse): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.ownerConnectionId)
}

export type VmCreatedResponse = {
    readonly vmId: string
}

export function readVmCreatedResponse(bc: bare.ByteCursor): VmCreatedResponse {
    return {
        vmId: bare.readString(bc),
    }
}

export function writeVmCreatedResponse(bc: bare.ByteCursor, x: VmCreatedResponse): void {
    bare.writeString(bc, x.vmId)
}

export type VmDisposedResponse = {
    readonly vmId: string
}

export function readVmDisposedResponse(bc: bare.ByteCursor): VmDisposedResponse {
    return {
        vmId: bare.readString(bc),
    }
}

export function writeVmDisposedResponse(bc: bare.ByteCursor, x: VmDisposedResponse): void {
    bare.writeString(bc, x.vmId)
}

export type RootFilesystemBootstrappedResponse = {
    readonly entryCount: u32
}

export function readRootFilesystemBootstrappedResponse(bc: bare.ByteCursor): RootFilesystemBootstrappedResponse {
    return {
        entryCount: bare.readU32(bc),
    }
}

export function writeRootFilesystemBootstrappedResponse(bc: bare.ByteCursor, x: RootFilesystemBootstrappedResponse): void {
    bare.writeU32(bc, x.entryCount)
}

export type VmConfiguredResponse = {
    readonly appliedMounts: u32
    readonly appliedSoftware: u32
    readonly projectedCommands: readonly ProjectedCommand[]
    readonly agents: readonly AgentosProjectedAgent[]
}

export function readVmConfiguredResponse(bc: bare.ByteCursor): VmConfiguredResponse {
    return {
        appliedMounts: bare.readU32(bc),
        appliedSoftware: bare.readU32(bc),
        projectedCommands: read13(bc),
        agents: read14(bc),
    }
}

export function writeVmConfiguredResponse(bc: bare.ByteCursor, x: VmConfiguredResponse): void {
    bare.writeU32(bc, x.appliedMounts)
    bare.writeU32(bc, x.appliedSoftware)
    write13(bc, x.projectedCommands)
    write14(bc, x.agents)
}

export type HostCallbacksRegisteredResponse = {
    readonly registration: string
    readonly commandCount: u32
}

export function readHostCallbacksRegisteredResponse(bc: bare.ByteCursor): HostCallbacksRegisteredResponse {
    return {
        registration: bare.readString(bc),
        commandCount: bare.readU32(bc),
    }
}

export function writeHostCallbacksRegisteredResponse(bc: bare.ByteCursor, x: HostCallbacksRegisteredResponse): void {
    bare.writeString(bc, x.registration)
    bare.writeU32(bc, x.commandCount)
}

export type LayerCreatedResponse = {
    readonly layerId: string
}

export function readLayerCreatedResponse(bc: bare.ByteCursor): LayerCreatedResponse {
    return {
        layerId: bare.readString(bc),
    }
}

export function writeLayerCreatedResponse(bc: bare.ByteCursor, x: LayerCreatedResponse): void {
    bare.writeString(bc, x.layerId)
}

export type LayerSealedResponse = {
    readonly layerId: string
}

export function readLayerSealedResponse(bc: bare.ByteCursor): LayerSealedResponse {
    return {
        layerId: bare.readString(bc),
    }
}

export function writeLayerSealedResponse(bc: bare.ByteCursor, x: LayerSealedResponse): void {
    bare.writeString(bc, x.layerId)
}

export type SnapshotImportedResponse = {
    readonly layerId: string
}

export function readSnapshotImportedResponse(bc: bare.ByteCursor): SnapshotImportedResponse {
    return {
        layerId: bare.readString(bc),
    }
}

export function writeSnapshotImportedResponse(bc: bare.ByteCursor, x: SnapshotImportedResponse): void {
    bare.writeString(bc, x.layerId)
}

export type SnapshotExportedResponse = {
    readonly layerId: string
    readonly entries: readonly RootFilesystemEntry[]
}

export function readSnapshotExportedResponse(bc: bare.ByteCursor): SnapshotExportedResponse {
    return {
        layerId: bare.readString(bc),
        entries: read4(bc),
    }
}

export function writeSnapshotExportedResponse(bc: bare.ByteCursor, x: SnapshotExportedResponse): void {
    bare.writeString(bc, x.layerId)
    write4(bc, x.entries)
}

export type OverlayCreatedResponse = {
    readonly layerId: string
}

export function readOverlayCreatedResponse(bc: bare.ByteCursor): OverlayCreatedResponse {
    return {
        layerId: bare.readString(bc),
    }
}

export function writeOverlayCreatedResponse(bc: bare.ByteCursor, x: OverlayCreatedResponse): void {
    bare.writeString(bc, x.layerId)
}

export type GuestFilesystemStat = {
    readonly mode: u32
    readonly size: u64
    readonly blocks: u64
    readonly dev: u64
    readonly rdev: u64
    readonly isDirectory: boolean
    readonly isSymbolicLink: boolean
    readonly atimeMs: u64
    readonly mtimeMs: u64
    readonly ctimeMs: u64
    readonly birthtimeMs: u64
    readonly ino: u64
    readonly nlink: u64
    readonly uid: u32
    readonly gid: u32
}

export function readGuestFilesystemStat(bc: bare.ByteCursor): GuestFilesystemStat {
    return {
        mode: bare.readU32(bc),
        size: bare.readU64(bc),
        blocks: bare.readU64(bc),
        dev: bare.readU64(bc),
        rdev: bare.readU64(bc),
        isDirectory: bare.readBool(bc),
        isSymbolicLink: bare.readBool(bc),
        atimeMs: bare.readU64(bc),
        mtimeMs: bare.readU64(bc),
        ctimeMs: bare.readU64(bc),
        birthtimeMs: bare.readU64(bc),
        ino: bare.readU64(bc),
        nlink: bare.readU64(bc),
        uid: bare.readU32(bc),
        gid: bare.readU32(bc),
    }
}

export function writeGuestFilesystemStat(bc: bare.ByteCursor, x: GuestFilesystemStat): void {
    bare.writeU32(bc, x.mode)
    bare.writeU64(bc, x.size)
    bare.writeU64(bc, x.blocks)
    bare.writeU64(bc, x.dev)
    bare.writeU64(bc, x.rdev)
    bare.writeBool(bc, x.isDirectory)
    bare.writeBool(bc, x.isSymbolicLink)
    bare.writeU64(bc, x.atimeMs)
    bare.writeU64(bc, x.mtimeMs)
    bare.writeU64(bc, x.ctimeMs)
    bare.writeU64(bc, x.birthtimeMs)
    bare.writeU64(bc, x.ino)
    bare.writeU64(bc, x.nlink)
    bare.writeU32(bc, x.uid)
    bare.writeU32(bc, x.gid)
}

export type GuestDirEntry = {
    readonly name: string
    readonly path: string
    readonly isDirectory: boolean
    readonly isSymbolicLink: boolean
    readonly size: u64
}

export function readGuestDirEntry(bc: bare.ByteCursor): GuestDirEntry {
    return {
        name: bare.readString(bc),
        path: bare.readString(bc),
        isDirectory: bare.readBool(bc),
        isSymbolicLink: bare.readBool(bc),
        size: bare.readU64(bc),
    }
}

export function writeGuestDirEntry(bc: bare.ByteCursor, x: GuestDirEntry): void {
    bare.writeString(bc, x.name)
    bare.writeString(bc, x.path)
    bare.writeBool(bc, x.isDirectory)
    bare.writeBool(bc, x.isSymbolicLink)
    bare.writeU64(bc, x.size)
}

function read27(bc: bare.ByteCursor): readonly GuestDirEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readGuestDirEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readGuestDirEntry(bc)
    }
    return result
}

function write27(bc: bare.ByteCursor, x: readonly GuestDirEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeGuestDirEntry(bc, x[i])
    }
}

function read28(bc: bare.ByteCursor): readonly GuestDirEntry[] | null {
    return bare.readBool(bc) ? read27(bc) : null
}

function write28(bc: bare.ByteCursor, x: readonly GuestDirEntry[] | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        write27(bc, x)
    }
}

function read29(bc: bare.ByteCursor): GuestFilesystemStat | null {
    return bare.readBool(bc) ? readGuestFilesystemStat(bc) : null
}

function write29(bc: bare.ByteCursor, x: GuestFilesystemStat | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeGuestFilesystemStat(bc, x)
    }
}

function read30(bc: bare.ByteCursor): boolean | null {
    return bare.readBool(bc) ? bare.readBool(bc) : null
}

function write30(bc: bare.ByteCursor, x: boolean | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeBool(bc, x)
    }
}

export type GuestFilesystemResultResponse = {
    readonly operation: GuestFilesystemOperation
    readonly path: string
    readonly content: string | null
    readonly encoding: RootFilesystemEntryEncoding | null
    readonly entries: readonly GuestDirEntry[] | null
    readonly stat: GuestFilesystemStat | null
    readonly exists: boolean | null
    readonly target: string | null
}

export function readGuestFilesystemResultResponse(bc: bare.ByteCursor): GuestFilesystemResultResponse {
    return {
        operation: readGuestFilesystemOperation(bc),
        path: bare.readString(bc),
        content: read0(bc),
        encoding: read3(bc),
        entries: read28(bc),
        stat: read29(bc),
        exists: read30(bc),
        target: read0(bc),
    }
}

export function writeGuestFilesystemResultResponse(bc: bare.ByteCursor, x: GuestFilesystemResultResponse): void {
    writeGuestFilesystemOperation(bc, x.operation)
    bare.writeString(bc, x.path)
    write0(bc, x.content)
    write3(bc, x.encoding)
    write28(bc, x.entries)
    write29(bc, x.stat)
    write30(bc, x.exists)
    write0(bc, x.target)
}

export type GuestKernelResultResponse = {
    readonly payload: ArrayBuffer
}

export function readGuestKernelResultResponse(bc: bare.ByteCursor): GuestKernelResultResponse {
    return {
        payload: bare.readData(bc),
    }
}

export function writeGuestKernelResultResponse(bc: bare.ByteCursor, x: GuestKernelResultResponse): void {
    bare.writeData(bc, x.payload)
}

export type RootFilesystemSnapshotResponse = {
    readonly entries: readonly RootFilesystemEntry[]
}

export function readRootFilesystemSnapshotResponse(bc: bare.ByteCursor): RootFilesystemSnapshotResponse {
    return {
        entries: read4(bc),
    }
}

export function writeRootFilesystemSnapshotResponse(bc: bare.ByteCursor, x: RootFilesystemSnapshotResponse): void {
    write4(bc, x.entries)
}

export type ProcessStartedResponse = {
    readonly processId: string
    readonly pid: u32 | null
}

export function readProcessStartedResponse(bc: bare.ByteCursor): ProcessStartedResponse {
    return {
        processId: bare.readString(bc),
        pid: read2(bc),
    }
}

export function writeProcessStartedResponse(bc: bare.ByteCursor, x: ProcessStartedResponse): void {
    bare.writeString(bc, x.processId)
    write2(bc, x.pid)
}

export type StdinWrittenResponse = {
    readonly processId: string
    readonly acceptedBytes: u64
}

export function readStdinWrittenResponse(bc: bare.ByteCursor): StdinWrittenResponse {
    return {
        processId: bare.readString(bc),
        acceptedBytes: bare.readU64(bc),
    }
}

export function writeStdinWrittenResponse(bc: bare.ByteCursor, x: StdinWrittenResponse): void {
    bare.writeString(bc, x.processId)
    bare.writeU64(bc, x.acceptedBytes)
}

export type PtyResizedResponse = {
    readonly processId: string
    readonly cols: u16
    readonly rows: u16
}

export function readPtyResizedResponse(bc: bare.ByteCursor): PtyResizedResponse {
    return {
        processId: bare.readString(bc),
        cols: bare.readU16(bc),
        rows: bare.readU16(bc),
    }
}

export function writePtyResizedResponse(bc: bare.ByteCursor, x: PtyResizedResponse): void {
    bare.writeString(bc, x.processId)
    bare.writeU16(bc, x.cols)
    bare.writeU16(bc, x.rows)
}

export type StdinClosedResponse = {
    readonly processId: string
}

export function readStdinClosedResponse(bc: bare.ByteCursor): StdinClosedResponse {
    return {
        processId: bare.readString(bc),
    }
}

export function writeStdinClosedResponse(bc: bare.ByteCursor, x: StdinClosedResponse): void {
    bare.writeString(bc, x.processId)
}

export type ProcessKilledResponse = {
    readonly processId: string
}

export function readProcessKilledResponse(bc: bare.ByteCursor): ProcessKilledResponse {
    return {
        processId: bare.readString(bc),
    }
}

export function writeProcessKilledResponse(bc: bare.ByteCursor, x: ProcessKilledResponse): void {
    bare.writeString(bc, x.processId)
}

export enum ProcessSnapshotStatus {
    Running = "Running",
    Exited = "Exited",
    Stopped = "Stopped",
}

export function readProcessSnapshotStatus(bc: bare.ByteCursor): ProcessSnapshotStatus {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return ProcessSnapshotStatus.Running
        case 1:
            return ProcessSnapshotStatus.Exited
        case 2:
            return ProcessSnapshotStatus.Stopped
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeProcessSnapshotStatus(bc: bare.ByteCursor, x: ProcessSnapshotStatus): void {
    switch (x) {
        case ProcessSnapshotStatus.Running: {
            bare.writeU8(bc, 0)
            break
        }
        case ProcessSnapshotStatus.Exited: {
            bare.writeU8(bc, 1)
            break
        }
        case ProcessSnapshotStatus.Stopped: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

function read31(bc: bare.ByteCursor): i32 | null {
    return bare.readBool(bc) ? bare.readI32(bc) : null
}

function write31(bc: bare.ByteCursor, x: i32 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeI32(bc, x)
    }
}

export type ProcessSnapshotEntry = {
    readonly processId: string
    readonly pid: u32
    readonly ppid: u32
    readonly pgid: u32
    readonly sid: u32
    readonly driver: string
    readonly command: string
    readonly args: readonly string[]
    readonly cwd: string
    readonly status: ProcessSnapshotStatus
    readonly exitCode: i32 | null
}

export function readProcessSnapshotEntry(bc: bare.ByteCursor): ProcessSnapshotEntry {
    return {
        processId: bare.readString(bc),
        pid: bare.readU32(bc),
        ppid: bare.readU32(bc),
        pgid: bare.readU32(bc),
        sid: bare.readU32(bc),
        driver: bare.readString(bc),
        command: bare.readString(bc),
        args: read6(bc),
        cwd: bare.readString(bc),
        status: readProcessSnapshotStatus(bc),
        exitCode: read31(bc),
    }
}

export function writeProcessSnapshotEntry(bc: bare.ByteCursor, x: ProcessSnapshotEntry): void {
    bare.writeString(bc, x.processId)
    bare.writeU32(bc, x.pid)
    bare.writeU32(bc, x.ppid)
    bare.writeU32(bc, x.pgid)
    bare.writeU32(bc, x.sid)
    bare.writeString(bc, x.driver)
    bare.writeString(bc, x.command)
    write6(bc, x.args)
    bare.writeString(bc, x.cwd)
    writeProcessSnapshotStatus(bc, x.status)
    write31(bc, x.exitCode)
}

function read32(bc: bare.ByteCursor): readonly ProcessSnapshotEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readProcessSnapshotEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readProcessSnapshotEntry(bc)
    }
    return result
}

function write32(bc: bare.ByteCursor, x: readonly ProcessSnapshotEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeProcessSnapshotEntry(bc, x[i])
    }
}

export type ProcessSnapshotResponse = {
    readonly processes: readonly ProcessSnapshotEntry[]
}

export function readProcessSnapshotResponse(bc: bare.ByteCursor): ProcessSnapshotResponse {
    return {
        processes: read32(bc),
    }
}

export function writeProcessSnapshotResponse(bc: bare.ByteCursor, x: ProcessSnapshotResponse): void {
    write32(bc, x.processes)
}

export type QueueSnapshotEntry = {
    readonly name: string
    readonly category: string
    readonly depth: u64
    readonly highWater: u64
    readonly capacity: u64
    readonly fillPercent: u64
}

export function readQueueSnapshotEntry(bc: bare.ByteCursor): QueueSnapshotEntry {
    return {
        name: bare.readString(bc),
        category: bare.readString(bc),
        depth: bare.readU64(bc),
        highWater: bare.readU64(bc),
        capacity: bare.readU64(bc),
        fillPercent: bare.readU64(bc),
    }
}

export function writeQueueSnapshotEntry(bc: bare.ByteCursor, x: QueueSnapshotEntry): void {
    bare.writeString(bc, x.name)
    bare.writeString(bc, x.category)
    bare.writeU64(bc, x.depth)
    bare.writeU64(bc, x.highWater)
    bare.writeU64(bc, x.capacity)
    bare.writeU64(bc, x.fillPercent)
}

function read33(bc: bare.ByteCursor): readonly QueueSnapshotEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readQueueSnapshotEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readQueueSnapshotEntry(bc)
    }
    return result
}

function write33(bc: bare.ByteCursor, x: readonly QueueSnapshotEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeQueueSnapshotEntry(bc, x[i])
    }
}

export type ResourceSnapshotResponse = {
    readonly runningProcesses: u64
    readonly exitedProcesses: u64
    readonly fdTables: u64
    readonly openFds: u64
    readonly pipes: u64
    readonly pipeBufferedBytes: u64
    readonly ptys: u64
    readonly ptyBufferedInputBytes: u64
    readonly ptyBufferedOutputBytes: u64
    readonly sockets: u64
    readonly socketListeners: u64
    readonly socketConnections: u64
    readonly socketBufferedBytes: u64
    readonly socketDatagramQueueLen: u64
    readonly queueSnapshots: readonly QueueSnapshotEntry[]
}

export function readResourceSnapshotResponse(bc: bare.ByteCursor): ResourceSnapshotResponse {
    return {
        runningProcesses: bare.readU64(bc),
        exitedProcesses: bare.readU64(bc),
        fdTables: bare.readU64(bc),
        openFds: bare.readU64(bc),
        pipes: bare.readU64(bc),
        pipeBufferedBytes: bare.readU64(bc),
        ptys: bare.readU64(bc),
        ptyBufferedInputBytes: bare.readU64(bc),
        ptyBufferedOutputBytes: bare.readU64(bc),
        sockets: bare.readU64(bc),
        socketListeners: bare.readU64(bc),
        socketConnections: bare.readU64(bc),
        socketBufferedBytes: bare.readU64(bc),
        socketDatagramQueueLen: bare.readU64(bc),
        queueSnapshots: read33(bc),
    }
}

export function writeResourceSnapshotResponse(bc: bare.ByteCursor, x: ResourceSnapshotResponse): void {
    bare.writeU64(bc, x.runningProcesses)
    bare.writeU64(bc, x.exitedProcesses)
    bare.writeU64(bc, x.fdTables)
    bare.writeU64(bc, x.openFds)
    bare.writeU64(bc, x.pipes)
    bare.writeU64(bc, x.pipeBufferedBytes)
    bare.writeU64(bc, x.ptys)
    bare.writeU64(bc, x.ptyBufferedInputBytes)
    bare.writeU64(bc, x.ptyBufferedOutputBytes)
    bare.writeU64(bc, x.sockets)
    bare.writeU64(bc, x.socketListeners)
    bare.writeU64(bc, x.socketConnections)
    bare.writeU64(bc, x.socketBufferedBytes)
    bare.writeU64(bc, x.socketDatagramQueueLen)
    write33(bc, x.queueSnapshots)
}

export type SocketStateEntry = {
    readonly processId: string
    readonly host: string | null
    readonly port: u16 | null
    readonly path: string | null
}

export function readSocketStateEntry(bc: bare.ByteCursor): SocketStateEntry {
    return {
        processId: bare.readString(bc),
        host: read0(bc),
        port: read26(bc),
        path: read0(bc),
    }
}

export function writeSocketStateEntry(bc: bare.ByteCursor, x: SocketStateEntry): void {
    bare.writeString(bc, x.processId)
    write0(bc, x.host)
    write26(bc, x.port)
    write0(bc, x.path)
}

function read34(bc: bare.ByteCursor): SocketStateEntry | null {
    return bare.readBool(bc) ? readSocketStateEntry(bc) : null
}

function write34(bc: bare.ByteCursor, x: SocketStateEntry | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeSocketStateEntry(bc, x)
    }
}

export type ListenerSnapshotResponse = {
    readonly listener: SocketStateEntry | null
}

export function readListenerSnapshotResponse(bc: bare.ByteCursor): ListenerSnapshotResponse {
    return {
        listener: read34(bc),
    }
}

export function writeListenerSnapshotResponse(bc: bare.ByteCursor, x: ListenerSnapshotResponse): void {
    write34(bc, x.listener)
}

export type BoundUdpSnapshotResponse = {
    readonly socket: SocketStateEntry | null
}

export function readBoundUdpSnapshotResponse(bc: bare.ByteCursor): BoundUdpSnapshotResponse {
    return {
        socket: read34(bc),
    }
}

export function writeBoundUdpSnapshotResponse(bc: bare.ByteCursor, x: BoundUdpSnapshotResponse): void {
    write34(bc, x.socket)
}

export enum SignalDispositionAction {
    Default = "Default",
    Ignore = "Ignore",
    User = "User",
}

export function readSignalDispositionAction(bc: bare.ByteCursor): SignalDispositionAction {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return SignalDispositionAction.Default
        case 1:
            return SignalDispositionAction.Ignore
        case 2:
            return SignalDispositionAction.User
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSignalDispositionAction(bc: bare.ByteCursor, x: SignalDispositionAction): void {
    switch (x) {
        case SignalDispositionAction.Default: {
            bare.writeU8(bc, 0)
            break
        }
        case SignalDispositionAction.Ignore: {
            bare.writeU8(bc, 1)
            break
        }
        case SignalDispositionAction.User: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

export type SignalHandlerRegistration = {
    readonly action: SignalDispositionAction
    readonly mask: Uint32Array
    readonly flags: u32
}

export function readSignalHandlerRegistration(bc: bare.ByteCursor): SignalHandlerRegistration {
    return {
        action: readSignalDispositionAction(bc),
        mask: bare.readU32Array(bc),
        flags: bare.readU32(bc),
    }
}

export function writeSignalHandlerRegistration(bc: bare.ByteCursor, x: SignalHandlerRegistration): void {
    writeSignalDispositionAction(bc, x.action)
    bare.writeU32Array(bc, x.mask)
    bare.writeU32(bc, x.flags)
}

function read35(bc: bare.ByteCursor): ReadonlyMap<u32, SignalHandlerRegistration> {
    const len = bare.readUintSafe(bc)
    const result = new Map<u32, SignalHandlerRegistration>()
    for (let i = 0; i < len; i++) {
        const offset = bc.offset
        const key = bare.readU32(bc)
        if (result.has(key)) {
            bc.offset = offset
            throw new bare.BareError(offset, "duplicated key")
        }
        result.set(key, readSignalHandlerRegistration(bc))
    }
    return result
}

function write35(bc: bare.ByteCursor, x: ReadonlyMap<u32, SignalHandlerRegistration>): void {
    bare.writeUintSafe(bc, x.size)
    for (const kv of x) {
        bare.writeU32(bc, kv[0])
        writeSignalHandlerRegistration(bc, kv[1])
    }
}

export type SignalStateResponse = {
    readonly processId: string
    readonly handlers: ReadonlyMap<u32, SignalHandlerRegistration>
}

export function readSignalStateResponse(bc: bare.ByteCursor): SignalStateResponse {
    return {
        processId: bare.readString(bc),
        handlers: read35(bc),
    }
}

export function writeSignalStateResponse(bc: bare.ByteCursor, x: SignalStateResponse): void {
    bare.writeString(bc, x.processId)
    write35(bc, x.handlers)
}

export type ZombieTimerCountResponse = {
    readonly count: u64
}

export function readZombieTimerCountResponse(bc: bare.ByteCursor): ZombieTimerCountResponse {
    return {
        count: bare.readU64(bc),
    }
}

export function writeZombieTimerCountResponse(bc: bare.ByteCursor, x: ZombieTimerCountResponse): void {
    bare.writeU64(bc, x.count)
}

export type FilesystemResultResponse = {
    readonly operation: FilesystemOperation
    readonly status: string
    readonly payloadSizeBytes: u64
}

export function readFilesystemResultResponse(bc: bare.ByteCursor): FilesystemResultResponse {
    return {
        operation: readFilesystemOperation(bc),
        status: bare.readString(bc),
        payloadSizeBytes: bare.readU64(bc),
    }
}

export function writeFilesystemResultResponse(bc: bare.ByteCursor, x: FilesystemResultResponse): void {
    writeFilesystemOperation(bc, x.operation)
    bare.writeString(bc, x.status)
    bare.writeU64(bc, x.payloadSizeBytes)
}

export type PermissionDecisionResponse = {
    readonly capability: string
    readonly decision: PermissionMode
}

export function readPermissionDecisionResponse(bc: bare.ByteCursor): PermissionDecisionResponse {
    return {
        capability: bare.readString(bc),
        decision: readPermissionMode(bc),
    }
}

export function writePermissionDecisionResponse(bc: bare.ByteCursor, x: PermissionDecisionResponse): void {
    bare.writeString(bc, x.capability)
    writePermissionMode(bc, x.decision)
}

export type PersistenceStateResponse = {
    readonly key: string
    readonly found: boolean
    readonly payloadSizeBytes: u64
}

export function readPersistenceStateResponse(bc: bare.ByteCursor): PersistenceStateResponse {
    return {
        key: bare.readString(bc),
        found: bare.readBool(bc),
        payloadSizeBytes: bare.readU64(bc),
    }
}

export function writePersistenceStateResponse(bc: bare.ByteCursor, x: PersistenceStateResponse): void {
    bare.writeString(bc, x.key)
    bare.writeBool(bc, x.found)
    bare.writeU64(bc, x.payloadSizeBytes)
}

export type PersistenceFlushedResponse = {
    readonly key: string
    readonly committedBytes: u64
}

export function readPersistenceFlushedResponse(bc: bare.ByteCursor): PersistenceFlushedResponse {
    return {
        key: bare.readString(bc),
        committedBytes: bare.readU64(bc),
    }
}

export function writePersistenceFlushedResponse(bc: bare.ByteCursor, x: PersistenceFlushedResponse): void {
    bare.writeString(bc, x.key)
    bare.writeU64(bc, x.committedBytes)
}

export type RejectedResponse = {
    readonly code: string
    readonly message: string
}

export function readRejectedResponse(bc: bare.ByteCursor): RejectedResponse {
    return {
        code: bare.readString(bc),
        message: bare.readString(bc),
    }
}

export function writeRejectedResponse(bc: bare.ByteCursor, x: RejectedResponse): void {
    bare.writeString(bc, x.code)
    bare.writeString(bc, x.message)
}

export type VmFetchResponse = {
    readonly responseJson: string
}

export function readVmFetchResponse(bc: bare.ByteCursor): VmFetchResponse {
    return {
        responseJson: bare.readString(bc),
    }
}

export function writeVmFetchResponse(bc: bare.ByteCursor, x: VmFetchResponse): void {
    bare.writeString(bc, x.responseJson)
}

export type ResponsePayload =
    | { readonly tag: "AuthenticatedResponse"; readonly val: AuthenticatedResponse }
    | { readonly tag: "SessionOpenedResponse"; readonly val: SessionOpenedResponse }
    | { readonly tag: "VmCreatedResponse"; readonly val: VmCreatedResponse }
    | { readonly tag: "VmDisposedResponse"; readonly val: VmDisposedResponse }
    | { readonly tag: "RootFilesystemBootstrappedResponse"; readonly val: RootFilesystemBootstrappedResponse }
    | { readonly tag: "VmConfiguredResponse"; readonly val: VmConfiguredResponse }
    | { readonly tag: "HostCallbacksRegisteredResponse"; readonly val: HostCallbacksRegisteredResponse }
    | { readonly tag: "LayerCreatedResponse"; readonly val: LayerCreatedResponse }
    | { readonly tag: "LayerSealedResponse"; readonly val: LayerSealedResponse }
    | { readonly tag: "SnapshotImportedResponse"; readonly val: SnapshotImportedResponse }
    | { readonly tag: "SnapshotExportedResponse"; readonly val: SnapshotExportedResponse }
    | { readonly tag: "OverlayCreatedResponse"; readonly val: OverlayCreatedResponse }
    | { readonly tag: "GuestFilesystemResultResponse"; readonly val: GuestFilesystemResultResponse }
    | { readonly tag: "RootFilesystemSnapshotResponse"; readonly val: RootFilesystemSnapshotResponse }
    | { readonly tag: "ProcessStartedResponse"; readonly val: ProcessStartedResponse }
    | { readonly tag: "StdinWrittenResponse"; readonly val: StdinWrittenResponse }
    | { readonly tag: "StdinClosedResponse"; readonly val: StdinClosedResponse }
    | { readonly tag: "ProcessKilledResponse"; readonly val: ProcessKilledResponse }
    | { readonly tag: "ProcessSnapshotResponse"; readonly val: ProcessSnapshotResponse }
    | { readonly tag: "ListenerSnapshotResponse"; readonly val: ListenerSnapshotResponse }
    | { readonly tag: "BoundUdpSnapshotResponse"; readonly val: BoundUdpSnapshotResponse }
    | { readonly tag: "SignalStateResponse"; readonly val: SignalStateResponse }
    | { readonly tag: "ZombieTimerCountResponse"; readonly val: ZombieTimerCountResponse }
    | { readonly tag: "FilesystemResultResponse"; readonly val: FilesystemResultResponse }
    | { readonly tag: "PermissionDecisionResponse"; readonly val: PermissionDecisionResponse }
    | { readonly tag: "PersistenceStateResponse"; readonly val: PersistenceStateResponse }
    | { readonly tag: "PersistenceFlushedResponse"; readonly val: PersistenceFlushedResponse }
    | { readonly tag: "RejectedResponse"; readonly val: RejectedResponse }
    | { readonly tag: "VmFetchResponse"; readonly val: VmFetchResponse }
    | { readonly tag: "ExtEnvelope"; readonly val: ExtEnvelope }
    | { readonly tag: "GuestKernelResultResponse"; readonly val: GuestKernelResultResponse }
    | { readonly tag: "PtyResizedResponse"; readonly val: PtyResizedResponse }
    | { readonly tag: "ResourceSnapshotResponse"; readonly val: ResourceSnapshotResponse }
    | { readonly tag: "PackageLinkedResponse"; readonly val: PackageLinkedResponse }
    | { readonly tag: "ProvidedCommandsResponse"; readonly val: ProvidedCommandsResponse }

export function readResponsePayload(bc: bare.ByteCursor): ResponsePayload {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AuthenticatedResponse", val: readAuthenticatedResponse(bc) }
        case 1:
            return { tag: "SessionOpenedResponse", val: readSessionOpenedResponse(bc) }
        case 2:
            return { tag: "VmCreatedResponse", val: readVmCreatedResponse(bc) }
        case 3:
            return { tag: "VmDisposedResponse", val: readVmDisposedResponse(bc) }
        case 4:
            return { tag: "RootFilesystemBootstrappedResponse", val: readRootFilesystemBootstrappedResponse(bc) }
        case 5:
            return { tag: "VmConfiguredResponse", val: readVmConfiguredResponse(bc) }
        case 6:
            return { tag: "HostCallbacksRegisteredResponse", val: readHostCallbacksRegisteredResponse(bc) }
        case 7:
            return { tag: "LayerCreatedResponse", val: readLayerCreatedResponse(bc) }
        case 8:
            return { tag: "LayerSealedResponse", val: readLayerSealedResponse(bc) }
        case 9:
            return { tag: "SnapshotImportedResponse", val: readSnapshotImportedResponse(bc) }
        case 10:
            return { tag: "SnapshotExportedResponse", val: readSnapshotExportedResponse(bc) }
        case 11:
            return { tag: "OverlayCreatedResponse", val: readOverlayCreatedResponse(bc) }
        case 12:
            return { tag: "GuestFilesystemResultResponse", val: readGuestFilesystemResultResponse(bc) }
        case 13:
            return { tag: "RootFilesystemSnapshotResponse", val: readRootFilesystemSnapshotResponse(bc) }
        case 14:
            return { tag: "ProcessStartedResponse", val: readProcessStartedResponse(bc) }
        case 15:
            return { tag: "StdinWrittenResponse", val: readStdinWrittenResponse(bc) }
        case 16:
            return { tag: "StdinClosedResponse", val: readStdinClosedResponse(bc) }
        case 17:
            return { tag: "ProcessKilledResponse", val: readProcessKilledResponse(bc) }
        case 18:
            return { tag: "ProcessSnapshotResponse", val: readProcessSnapshotResponse(bc) }
        case 19:
            return { tag: "ListenerSnapshotResponse", val: readListenerSnapshotResponse(bc) }
        case 20:
            return { tag: "BoundUdpSnapshotResponse", val: readBoundUdpSnapshotResponse(bc) }
        case 21:
            return { tag: "SignalStateResponse", val: readSignalStateResponse(bc) }
        case 22:
            return { tag: "ZombieTimerCountResponse", val: readZombieTimerCountResponse(bc) }
        case 23:
            return { tag: "FilesystemResultResponse", val: readFilesystemResultResponse(bc) }
        case 24:
            return { tag: "PermissionDecisionResponse", val: readPermissionDecisionResponse(bc) }
        case 25:
            return { tag: "PersistenceStateResponse", val: readPersistenceStateResponse(bc) }
        case 26:
            return { tag: "PersistenceFlushedResponse", val: readPersistenceFlushedResponse(bc) }
        case 27:
            return { tag: "RejectedResponse", val: readRejectedResponse(bc) }
        case 28:
            return { tag: "VmFetchResponse", val: readVmFetchResponse(bc) }
        case 29:
            return { tag: "ExtEnvelope", val: readExtEnvelope(bc) }
        case 30:
            return { tag: "GuestKernelResultResponse", val: readGuestKernelResultResponse(bc) }
        case 31:
            return { tag: "PtyResizedResponse", val: readPtyResizedResponse(bc) }
        case 32:
            return { tag: "ResourceSnapshotResponse", val: readResourceSnapshotResponse(bc) }
        case 33:
            return { tag: "PackageLinkedResponse", val: readPackageLinkedResponse(bc) }
        case 34:
            return { tag: "ProvidedCommandsResponse", val: readProvidedCommandsResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeResponsePayload(bc: bare.ByteCursor, x: ResponsePayload): void {
    switch (x.tag) {
        case "AuthenticatedResponse": {
            bare.writeU8(bc, 0)
            writeAuthenticatedResponse(bc, x.val)
            break
        }
        case "SessionOpenedResponse": {
            bare.writeU8(bc, 1)
            writeSessionOpenedResponse(bc, x.val)
            break
        }
        case "VmCreatedResponse": {
            bare.writeU8(bc, 2)
            writeVmCreatedResponse(bc, x.val)
            break
        }
        case "VmDisposedResponse": {
            bare.writeU8(bc, 3)
            writeVmDisposedResponse(bc, x.val)
            break
        }
        case "RootFilesystemBootstrappedResponse": {
            bare.writeU8(bc, 4)
            writeRootFilesystemBootstrappedResponse(bc, x.val)
            break
        }
        case "VmConfiguredResponse": {
            bare.writeU8(bc, 5)
            writeVmConfiguredResponse(bc, x.val)
            break
        }
        case "HostCallbacksRegisteredResponse": {
            bare.writeU8(bc, 6)
            writeHostCallbacksRegisteredResponse(bc, x.val)
            break
        }
        case "LayerCreatedResponse": {
            bare.writeU8(bc, 7)
            writeLayerCreatedResponse(bc, x.val)
            break
        }
        case "LayerSealedResponse": {
            bare.writeU8(bc, 8)
            writeLayerSealedResponse(bc, x.val)
            break
        }
        case "SnapshotImportedResponse": {
            bare.writeU8(bc, 9)
            writeSnapshotImportedResponse(bc, x.val)
            break
        }
        case "SnapshotExportedResponse": {
            bare.writeU8(bc, 10)
            writeSnapshotExportedResponse(bc, x.val)
            break
        }
        case "OverlayCreatedResponse": {
            bare.writeU8(bc, 11)
            writeOverlayCreatedResponse(bc, x.val)
            break
        }
        case "GuestFilesystemResultResponse": {
            bare.writeU8(bc, 12)
            writeGuestFilesystemResultResponse(bc, x.val)
            break
        }
        case "RootFilesystemSnapshotResponse": {
            bare.writeU8(bc, 13)
            writeRootFilesystemSnapshotResponse(bc, x.val)
            break
        }
        case "ProcessStartedResponse": {
            bare.writeU8(bc, 14)
            writeProcessStartedResponse(bc, x.val)
            break
        }
        case "StdinWrittenResponse": {
            bare.writeU8(bc, 15)
            writeStdinWrittenResponse(bc, x.val)
            break
        }
        case "StdinClosedResponse": {
            bare.writeU8(bc, 16)
            writeStdinClosedResponse(bc, x.val)
            break
        }
        case "ProcessKilledResponse": {
            bare.writeU8(bc, 17)
            writeProcessKilledResponse(bc, x.val)
            break
        }
        case "ProcessSnapshotResponse": {
            bare.writeU8(bc, 18)
            writeProcessSnapshotResponse(bc, x.val)
            break
        }
        case "ListenerSnapshotResponse": {
            bare.writeU8(bc, 19)
            writeListenerSnapshotResponse(bc, x.val)
            break
        }
        case "BoundUdpSnapshotResponse": {
            bare.writeU8(bc, 20)
            writeBoundUdpSnapshotResponse(bc, x.val)
            break
        }
        case "SignalStateResponse": {
            bare.writeU8(bc, 21)
            writeSignalStateResponse(bc, x.val)
            break
        }
        case "ZombieTimerCountResponse": {
            bare.writeU8(bc, 22)
            writeZombieTimerCountResponse(bc, x.val)
            break
        }
        case "FilesystemResultResponse": {
            bare.writeU8(bc, 23)
            writeFilesystemResultResponse(bc, x.val)
            break
        }
        case "PermissionDecisionResponse": {
            bare.writeU8(bc, 24)
            writePermissionDecisionResponse(bc, x.val)
            break
        }
        case "PersistenceStateResponse": {
            bare.writeU8(bc, 25)
            writePersistenceStateResponse(bc, x.val)
            break
        }
        case "PersistenceFlushedResponse": {
            bare.writeU8(bc, 26)
            writePersistenceFlushedResponse(bc, x.val)
            break
        }
        case "RejectedResponse": {
            bare.writeU8(bc, 27)
            writeRejectedResponse(bc, x.val)
            break
        }
        case "VmFetchResponse": {
            bare.writeU8(bc, 28)
            writeVmFetchResponse(bc, x.val)
            break
        }
        case "ExtEnvelope": {
            bare.writeU8(bc, 29)
            writeExtEnvelope(bc, x.val)
            break
        }
        case "GuestKernelResultResponse": {
            bare.writeU8(bc, 30)
            writeGuestKernelResultResponse(bc, x.val)
            break
        }
        case "PtyResizedResponse": {
            bare.writeU8(bc, 31)
            writePtyResizedResponse(bc, x.val)
            break
        }
        case "ResourceSnapshotResponse": {
            bare.writeU8(bc, 32)
            writeResourceSnapshotResponse(bc, x.val)
            break
        }
        case "PackageLinkedResponse": {
            bare.writeU8(bc, 33)
            writePackageLinkedResponse(bc, x.val)
            break
        }
        case "ProvidedCommandsResponse": {
            bare.writeU8(bc, 34)
            writeProvidedCommandsResponse(bc, x.val)
            break
        }
    }
}

export type ResponseFrame = {
    readonly schema: ProtocolSchema
    readonly requestId: RequestId
    readonly ownership: OwnershipScope
    readonly payload: ResponsePayload
}

export function readResponseFrame(bc: bare.ByteCursor): ResponseFrame {
    return {
        schema: readProtocolSchema(bc),
        requestId: readRequestId(bc),
        ownership: readOwnershipScope(bc),
        payload: readResponsePayload(bc),
    }
}

export function writeResponseFrame(bc: bare.ByteCursor, x: ResponseFrame): void {
    writeProtocolSchema(bc, x.schema)
    writeRequestId(bc, x.requestId)
    writeOwnershipScope(bc, x.ownership)
    writeResponsePayload(bc, x.payload)
}

export enum VmLifecycleState {
    Creating = "Creating",
    Ready = "Ready",
    Disposing = "Disposing",
    Disposed = "Disposed",
    Failed = "Failed",
}

export function readVmLifecycleState(bc: bare.ByteCursor): VmLifecycleState {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return VmLifecycleState.Creating
        case 1:
            return VmLifecycleState.Ready
        case 2:
            return VmLifecycleState.Disposing
        case 3:
            return VmLifecycleState.Disposed
        case 4:
            return VmLifecycleState.Failed
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeVmLifecycleState(bc: bare.ByteCursor, x: VmLifecycleState): void {
    switch (x) {
        case VmLifecycleState.Creating: {
            bare.writeU8(bc, 0)
            break
        }
        case VmLifecycleState.Ready: {
            bare.writeU8(bc, 1)
            break
        }
        case VmLifecycleState.Disposing: {
            bare.writeU8(bc, 2)
            break
        }
        case VmLifecycleState.Disposed: {
            bare.writeU8(bc, 3)
            break
        }
        case VmLifecycleState.Failed: {
            bare.writeU8(bc, 4)
            break
        }
    }
}

export type VmLifecycleEvent = {
    readonly state: VmLifecycleState
}

export function readVmLifecycleEvent(bc: bare.ByteCursor): VmLifecycleEvent {
    return {
        state: readVmLifecycleState(bc),
    }
}

export function writeVmLifecycleEvent(bc: bare.ByteCursor, x: VmLifecycleEvent): void {
    writeVmLifecycleState(bc, x.state)
}

export enum StreamChannel {
    Stdout = "Stdout",
    Stderr = "Stderr",
}

export function readStreamChannel(bc: bare.ByteCursor): StreamChannel {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return StreamChannel.Stdout
        case 1:
            return StreamChannel.Stderr
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeStreamChannel(bc: bare.ByteCursor, x: StreamChannel): void {
    switch (x) {
        case StreamChannel.Stdout: {
            bare.writeU8(bc, 0)
            break
        }
        case StreamChannel.Stderr: {
            bare.writeU8(bc, 1)
            break
        }
    }
}

export type ProcessOutputEvent = {
    readonly processId: string
    readonly channel: StreamChannel
    readonly chunk: ArrayBuffer
}

export function readProcessOutputEvent(bc: bare.ByteCursor): ProcessOutputEvent {
    return {
        processId: bare.readString(bc),
        channel: readStreamChannel(bc),
        chunk: bare.readData(bc),
    }
}

export function writeProcessOutputEvent(bc: bare.ByteCursor, x: ProcessOutputEvent): void {
    bare.writeString(bc, x.processId)
    writeStreamChannel(bc, x.channel)
    bare.writeData(bc, x.chunk)
}

export type ProcessExitedEvent = {
    readonly processId: string
    readonly exitCode: i32
}

export function readProcessExitedEvent(bc: bare.ByteCursor): ProcessExitedEvent {
    return {
        processId: bare.readString(bc),
        exitCode: bare.readI32(bc),
    }
}

export function writeProcessExitedEvent(bc: bare.ByteCursor, x: ProcessExitedEvent): void {
    bare.writeString(bc, x.processId)
    bare.writeI32(bc, x.exitCode)
}

export type StructuredEvent = {
    readonly name: string
    readonly detail: ReadonlyMap<string, string>
}

export function readStructuredEvent(bc: bare.ByteCursor): StructuredEvent {
    return {
        name: bare.readString(bc),
        detail: read1(bc),
    }
}

export function writeStructuredEvent(bc: bare.ByteCursor, x: StructuredEvent): void {
    bare.writeString(bc, x.name)
    write1(bc, x.detail)
}

export type EventPayload =
    | { readonly tag: "VmLifecycleEvent"; readonly val: VmLifecycleEvent }
    | { readonly tag: "ProcessOutputEvent"; readonly val: ProcessOutputEvent }
    | { readonly tag: "ProcessExitedEvent"; readonly val: ProcessExitedEvent }
    | { readonly tag: "StructuredEvent"; readonly val: StructuredEvent }
    | { readonly tag: "ExtEnvelope"; readonly val: ExtEnvelope }

export function readEventPayload(bc: bare.ByteCursor): EventPayload {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "VmLifecycleEvent", val: readVmLifecycleEvent(bc) }
        case 1:
            return { tag: "ProcessOutputEvent", val: readProcessOutputEvent(bc) }
        case 2:
            return { tag: "ProcessExitedEvent", val: readProcessExitedEvent(bc) }
        case 3:
            return { tag: "StructuredEvent", val: readStructuredEvent(bc) }
        case 4:
            return { tag: "ExtEnvelope", val: readExtEnvelope(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeEventPayload(bc: bare.ByteCursor, x: EventPayload): void {
    switch (x.tag) {
        case "VmLifecycleEvent": {
            bare.writeU8(bc, 0)
            writeVmLifecycleEvent(bc, x.val)
            break
        }
        case "ProcessOutputEvent": {
            bare.writeU8(bc, 1)
            writeProcessOutputEvent(bc, x.val)
            break
        }
        case "ProcessExitedEvent": {
            bare.writeU8(bc, 2)
            writeProcessExitedEvent(bc, x.val)
            break
        }
        case "StructuredEvent": {
            bare.writeU8(bc, 3)
            writeStructuredEvent(bc, x.val)
            break
        }
        case "ExtEnvelope": {
            bare.writeU8(bc, 4)
            writeExtEnvelope(bc, x.val)
            break
        }
    }
}

export type EventFrame = {
    readonly schema: ProtocolSchema
    readonly ownership: OwnershipScope
    readonly payload: EventPayload
}

export function readEventFrame(bc: bare.ByteCursor): EventFrame {
    return {
        schema: readProtocolSchema(bc),
        ownership: readOwnershipScope(bc),
        payload: readEventPayload(bc),
    }
}

export function writeEventFrame(bc: bare.ByteCursor, x: EventFrame): void {
    writeProtocolSchema(bc, x.schema)
    writeOwnershipScope(bc, x.ownership)
    writeEventPayload(bc, x.payload)
}

export type HostCallbackRequest = {
    readonly invocationId: string
    readonly callbackKey: string
    readonly input: JsonUtf8
    readonly timeoutMs: u64
}

export function readHostCallbackRequest(bc: bare.ByteCursor): HostCallbackRequest {
    return {
        invocationId: bare.readString(bc),
        callbackKey: bare.readString(bc),
        input: readJsonUtf8(bc),
        timeoutMs: bare.readU64(bc),
    }
}

export function writeHostCallbackRequest(bc: bare.ByteCursor, x: HostCallbackRequest): void {
    bare.writeString(bc, x.invocationId)
    bare.writeString(bc, x.callbackKey)
    writeJsonUtf8(bc, x.input)
    bare.writeU64(bc, x.timeoutMs)
}

export type JsBridgeCallRequest = {
    readonly callId: string
    readonly mountId: string
    readonly operation: string
    readonly args: JsonUtf8
}

export function readJsBridgeCallRequest(bc: bare.ByteCursor): JsBridgeCallRequest {
    return {
        callId: bare.readString(bc),
        mountId: bare.readString(bc),
        operation: bare.readString(bc),
        args: readJsonUtf8(bc),
    }
}

export function writeJsBridgeCallRequest(bc: bare.ByteCursor, x: JsBridgeCallRequest): void {
    bare.writeString(bc, x.callId)
    bare.writeString(bc, x.mountId)
    bare.writeString(bc, x.operation)
    writeJsonUtf8(bc, x.args)
}

export type SidecarRequestPayload =
    | { readonly tag: "HostCallbackRequest"; readonly val: HostCallbackRequest }
    | { readonly tag: "JsBridgeCallRequest"; readonly val: JsBridgeCallRequest }
    | { readonly tag: "ExtEnvelope"; readonly val: ExtEnvelope }

export function readSidecarRequestPayload(bc: bare.ByteCursor): SidecarRequestPayload {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "HostCallbackRequest", val: readHostCallbackRequest(bc) }
        case 1:
            return { tag: "JsBridgeCallRequest", val: readJsBridgeCallRequest(bc) }
        case 2:
            return { tag: "ExtEnvelope", val: readExtEnvelope(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSidecarRequestPayload(bc: bare.ByteCursor, x: SidecarRequestPayload): void {
    switch (x.tag) {
        case "HostCallbackRequest": {
            bare.writeU8(bc, 0)
            writeHostCallbackRequest(bc, x.val)
            break
        }
        case "JsBridgeCallRequest": {
            bare.writeU8(bc, 1)
            writeJsBridgeCallRequest(bc, x.val)
            break
        }
        case "ExtEnvelope": {
            bare.writeU8(bc, 2)
            writeExtEnvelope(bc, x.val)
            break
        }
    }
}

export type SidecarRequestFrame = {
    readonly schema: ProtocolSchema
    readonly requestId: RequestId
    readonly ownership: OwnershipScope
    readonly payload: SidecarRequestPayload
}

export function readSidecarRequestFrame(bc: bare.ByteCursor): SidecarRequestFrame {
    return {
        schema: readProtocolSchema(bc),
        requestId: readRequestId(bc),
        ownership: readOwnershipScope(bc),
        payload: readSidecarRequestPayload(bc),
    }
}

export function writeSidecarRequestFrame(bc: bare.ByteCursor, x: SidecarRequestFrame): void {
    writeProtocolSchema(bc, x.schema)
    writeRequestId(bc, x.requestId)
    writeOwnershipScope(bc, x.ownership)
    writeSidecarRequestPayload(bc, x.payload)
}

function read36(bc: bare.ByteCursor): JsonUtf8 | null {
    return bare.readBool(bc) ? readJsonUtf8(bc) : null
}

function write36(bc: bare.ByteCursor, x: JsonUtf8 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeJsonUtf8(bc, x)
    }
}

export type HostCallbackResultResponse = {
    readonly invocationId: string
    readonly result: JsonUtf8 | null
    readonly error: string | null
}

export function readHostCallbackResultResponse(bc: bare.ByteCursor): HostCallbackResultResponse {
    return {
        invocationId: bare.readString(bc),
        result: read36(bc),
        error: read0(bc),
    }
}

export function writeHostCallbackResultResponse(bc: bare.ByteCursor, x: HostCallbackResultResponse): void {
    bare.writeString(bc, x.invocationId)
    write36(bc, x.result)
    write0(bc, x.error)
}

export type JsBridgeResultResponse = {
    readonly callId: string
    readonly result: JsonUtf8 | null
    readonly error: string | null
}

export function readJsBridgeResultResponse(bc: bare.ByteCursor): JsBridgeResultResponse {
    return {
        callId: bare.readString(bc),
        result: read36(bc),
        error: read0(bc),
    }
}

export function writeJsBridgeResultResponse(bc: bare.ByteCursor, x: JsBridgeResultResponse): void {
    bare.writeString(bc, x.callId)
    write36(bc, x.result)
    write0(bc, x.error)
}

export type SidecarResponsePayload =
    | { readonly tag: "HostCallbackResultResponse"; readonly val: HostCallbackResultResponse }
    | { readonly tag: "JsBridgeResultResponse"; readonly val: JsBridgeResultResponse }
    | { readonly tag: "ExtEnvelope"; readonly val: ExtEnvelope }

export function readSidecarResponsePayload(bc: bare.ByteCursor): SidecarResponsePayload {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "HostCallbackResultResponse", val: readHostCallbackResultResponse(bc) }
        case 1:
            return { tag: "JsBridgeResultResponse", val: readJsBridgeResultResponse(bc) }
        case 2:
            return { tag: "ExtEnvelope", val: readExtEnvelope(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeSidecarResponsePayload(bc: bare.ByteCursor, x: SidecarResponsePayload): void {
    switch (x.tag) {
        case "HostCallbackResultResponse": {
            bare.writeU8(bc, 0)
            writeHostCallbackResultResponse(bc, x.val)
            break
        }
        case "JsBridgeResultResponse": {
            bare.writeU8(bc, 1)
            writeJsBridgeResultResponse(bc, x.val)
            break
        }
        case "ExtEnvelope": {
            bare.writeU8(bc, 2)
            writeExtEnvelope(bc, x.val)
            break
        }
    }
}

export type SidecarResponseFrame = {
    readonly schema: ProtocolSchema
    readonly requestId: RequestId
    readonly ownership: OwnershipScope
    readonly payload: SidecarResponsePayload
}

export function readSidecarResponseFrame(bc: bare.ByteCursor): SidecarResponseFrame {
    return {
        schema: readProtocolSchema(bc),
        requestId: readRequestId(bc),
        ownership: readOwnershipScope(bc),
        payload: readSidecarResponsePayload(bc),
    }
}

export function writeSidecarResponseFrame(bc: bare.ByteCursor, x: SidecarResponseFrame): void {
    writeProtocolSchema(bc, x.schema)
    writeRequestId(bc, x.requestId)
    writeOwnershipScope(bc, x.ownership)
    writeSidecarResponsePayload(bc, x.payload)
}

export type ProtocolFrame =
    | { readonly tag: "RequestFrame"; readonly val: RequestFrame }
    | { readonly tag: "ResponseFrame"; readonly val: ResponseFrame }
    | { readonly tag: "EventFrame"; readonly val: EventFrame }
    | { readonly tag: "SidecarRequestFrame"; readonly val: SidecarRequestFrame }
    | { readonly tag: "SidecarResponseFrame"; readonly val: SidecarResponseFrame }

export function readProtocolFrame(bc: bare.ByteCursor): ProtocolFrame {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "RequestFrame", val: readRequestFrame(bc) }
        case 1:
            return { tag: "ResponseFrame", val: readResponseFrame(bc) }
        case 2:
            return { tag: "EventFrame", val: readEventFrame(bc) }
        case 3:
            return { tag: "SidecarRequestFrame", val: readSidecarRequestFrame(bc) }
        case 4:
            return { tag: "SidecarResponseFrame", val: readSidecarResponseFrame(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeProtocolFrame(bc: bare.ByteCursor, x: ProtocolFrame): void {
    switch (x.tag) {
        case "RequestFrame": {
            bare.writeU8(bc, 0)
            writeRequestFrame(bc, x.val)
            break
        }
        case "ResponseFrame": {
            bare.writeU8(bc, 1)
            writeResponseFrame(bc, x.val)
            break
        }
        case "EventFrame": {
            bare.writeU8(bc, 2)
            writeEventFrame(bc, x.val)
            break
        }
        case "SidecarRequestFrame": {
            bare.writeU8(bc, 3)
            writeSidecarRequestFrame(bc, x.val)
            break
        }
        case "SidecarResponseFrame": {
            bare.writeU8(bc, 4)
            writeSidecarResponseFrame(bc, x.val)
            break
        }
    }
}

export function encodeProtocolFrame(x: ProtocolFrame, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeProtocolFrame(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeProtocolFrame(bytes: Uint8Array): ProtocolFrame {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readProtocolFrame(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}
