// @generated - run pnpm --dir packages/core build:agentos-protocol
import * as bare from "@rivetkit/bare-ts"

const DEFAULT_CONFIG = /* @__PURE__ */ bare.Config({})

export type i32 = number
export type u32 = number
export type u64 = bigint

export type JsonUtf8 = string

export function readJsonUtf8(bc: bare.ByteCursor): JsonUtf8 {
    return bare.readString(bc)
}

export function writeJsonUtf8(bc: bare.ByteCursor, x: JsonUtf8): void {
    bare.writeString(bc, x)
}

export enum AcpRuntimeKind {
    JavaScript = "JavaScript",
    Python = "Python",
    WebAssembly = "WebAssembly",
}

export function readAcpRuntimeKind(bc: bare.ByteCursor): AcpRuntimeKind {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return AcpRuntimeKind.JavaScript
        case 1:
            return AcpRuntimeKind.Python
        case 2:
            return AcpRuntimeKind.WebAssembly
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeAcpRuntimeKind(bc: bare.ByteCursor, x: AcpRuntimeKind): void {
    switch (x) {
        case AcpRuntimeKind.JavaScript: {
            bare.writeU8(bc, 0)
            break
        }
        case AcpRuntimeKind.Python: {
            bare.writeU8(bc, 1)
            break
        }
        case AcpRuntimeKind.WebAssembly: {
            bare.writeU8(bc, 2)
            break
        }
    }
}

function read0(bc: bare.ByteCursor): AcpRuntimeKind | null {
    return bare.readBool(bc) ? readAcpRuntimeKind(bc) : null
}

function write0(bc: bare.ByteCursor, x: AcpRuntimeKind | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeAcpRuntimeKind(bc, x)
    }
}

function read1(bc: bare.ByteCursor): string | null {
    return bare.readBool(bc) ? bare.readString(bc) : null
}

function write1(bc: bare.ByteCursor, x: string | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeString(bc, x)
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

function read3(bc: bare.ByteCursor): readonly string[] | null {
    return bare.readBool(bc) ? read2(bc) : null
}

function write3(bc: bare.ByteCursor, x: readonly string[] | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        write2(bc, x)
    }
}

function read4(bc: bare.ByteCursor): ReadonlyMap<string, string> {
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

function write4(bc: bare.ByteCursor, x: ReadonlyMap<string, string>): void {
    bare.writeUintSafe(bc, x.size)
    for (const kv of x) {
        bare.writeString(bc, kv[0])
        bare.writeString(bc, kv[1])
    }
}

function read5(bc: bare.ByteCursor): ReadonlyMap<string, string> | null {
    return bare.readBool(bc) ? read4(bc) : null
}

function write5(bc: bare.ByteCursor, x: ReadonlyMap<string, string> | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        write4(bc, x)
    }
}

function read6(bc: bare.ByteCursor): i32 | null {
    return bare.readBool(bc) ? bare.readI32(bc) : null
}

function write6(bc: bare.ByteCursor, x: i32 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeI32(bc, x)
    }
}

function read7(bc: bare.ByteCursor): JsonUtf8 | null {
    return bare.readBool(bc) ? readJsonUtf8(bc) : null
}

function write7(bc: bare.ByteCursor, x: JsonUtf8 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeJsonUtf8(bc, x)
    }
}

function read8(bc: bare.ByteCursor): boolean | null {
    return bare.readBool(bc) ? bare.readBool(bc) : null
}

function write8(bc: bare.ByteCursor, x: boolean | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeBool(bc, x)
    }
}

export type AcpCreateSessionRequest = {
    readonly agentType: string
    readonly runtime: AcpRuntimeKind | null
    readonly cwd: string | null
    readonly args: readonly string[] | null
    readonly env: ReadonlyMap<string, string> | null
    readonly protocolVersion: i32 | null
    readonly clientCapabilities: JsonUtf8 | null
    readonly mcpServers: JsonUtf8 | null
    readonly skipOsInstructions: boolean | null
    readonly additionalInstructions: string | null
}

export function readAcpCreateSessionRequest(bc: bare.ByteCursor): AcpCreateSessionRequest {
    return {
        agentType: bare.readString(bc),
        runtime: read0(bc),
        cwd: read1(bc),
        args: read3(bc),
        env: read5(bc),
        protocolVersion: read6(bc),
        clientCapabilities: read7(bc),
        mcpServers: read7(bc),
        skipOsInstructions: read8(bc),
        additionalInstructions: read1(bc),
    }
}

export function writeAcpCreateSessionRequest(bc: bare.ByteCursor, x: AcpCreateSessionRequest): void {
    bare.writeString(bc, x.agentType)
    write0(bc, x.runtime)
    write1(bc, x.cwd)
    write3(bc, x.args)
    write5(bc, x.env)
    write6(bc, x.protocolVersion)
    write7(bc, x.clientCapabilities)
    write7(bc, x.mcpServers)
    write8(bc, x.skipOsInstructions)
    write1(bc, x.additionalInstructions)
}

export type AcpSessionRequest = {
    readonly sessionId: string
    readonly method: string
    readonly params: JsonUtf8 | null
}

export function readAcpSessionRequest(bc: bare.ByteCursor): AcpSessionRequest {
    return {
        sessionId: bare.readString(bc),
        method: bare.readString(bc),
        params: read7(bc),
    }
}

export function writeAcpSessionRequest(bc: bare.ByteCursor, x: AcpSessionRequest): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.method)
    write7(bc, x.params)
}

/**
 * Select a session configuration option by its adapter-reported category. The
 * sidecar owns category-to-config-id resolution and adapter-specific read-only
 * behavior; clients forward only the caller's requested value.
 */
export type AcpSetSessionConfigRequest = {
    readonly sessionId: string
    readonly category: string
    readonly value: string
}

export function readAcpSetSessionConfigRequest(bc: bare.ByteCursor): AcpSetSessionConfigRequest {
    return {
        sessionId: bare.readString(bc),
        category: bare.readString(bc),
        value: bare.readString(bc),
    }
}

export function writeAcpSetSessionConfigRequest(bc: bare.ByteCursor, x: AcpSetSessionConfigRequest): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.category)
    bare.writeString(bc, x.value)
}

/**
 * Enumerate the agents available in this VM. The sidecar answers from the already
 * projected `/opt/agentos` packages (client parses no manifests).
 */
export type AcpListAgentsRequest = {
    readonly reserved: boolean
}

export function readAcpListAgentsRequest(bc: bare.ByteCursor): AcpListAgentsRequest {
    return {
        reserved: bare.readBool(bc),
    }
}

export function writeAcpListAgentsRequest(bc: bare.ByteCursor, x: AcpListAgentsRequest): void {
    bare.writeBool(bc, x.reserved)
}

export type AcpAgentEntry = {
    readonly id: string
    readonly installed: boolean
    readonly adapterEntrypoint: string
}

export function readAcpAgentEntry(bc: bare.ByteCursor): AcpAgentEntry {
    return {
        id: bare.readString(bc),
        installed: bare.readBool(bc),
        adapterEntrypoint: bare.readString(bc),
    }
}

export function writeAcpAgentEntry(bc: bare.ByteCursor, x: AcpAgentEntry): void {
    bare.writeString(bc, x.id)
    bare.writeBool(bc, x.installed)
    bare.writeString(bc, x.adapterEntrypoint)
}

function read9(bc: bare.ByteCursor): readonly AcpAgentEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readAcpAgentEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readAcpAgentEntry(bc)
    }
    return result
}

function write9(bc: bare.ByteCursor, x: readonly AcpAgentEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeAcpAgentEntry(bc, x[i])
    }
}

export type AcpListAgentsResponse = {
    readonly agents: readonly AcpAgentEntry[]
}

export function readAcpListAgentsResponse(bc: bare.ByteCursor): AcpListAgentsResponse {
    return {
        agents: read9(bc),
    }
}

export function writeAcpListAgentsResponse(bc: bare.ByteCursor, x: AcpListAgentsResponse): void {
    write9(bc, x.agents)
}

export type AcpGetSessionStateRequest = {
    readonly sessionId: string
}

export function readAcpGetSessionStateRequest(bc: bare.ByteCursor): AcpGetSessionStateRequest {
    return {
        sessionId: bare.readString(bc),
    }
}

export function writeAcpGetSessionStateRequest(bc: bare.ByteCursor, x: AcpGetSessionStateRequest): void {
    bare.writeString(bc, x.sessionId)
}

export type AcpListSessionsRequest = {
    readonly reserved: boolean
}

export function readAcpListSessionsRequest(bc: bare.ByteCursor): AcpListSessionsRequest {
    return {
        reserved: bare.readBool(bc),
    }
}

export function writeAcpListSessionsRequest(bc: bare.ByteCursor, x: AcpListSessionsRequest): void {
    bare.writeBool(bc, x.reserved)
}

export type AcpCloseSessionRequest = {
    readonly sessionId: string
}

export function readAcpCloseSessionRequest(bc: bare.ByteCursor): AcpCloseSessionRequest {
    return {
        sessionId: bare.readString(bc),
    }
}

export function writeAcpCloseSessionRequest(bc: bare.ByteCursor, x: AcpCloseSessionRequest): void {
    bare.writeString(bc, x.sessionId)
}

/**
 * Resume a session that exists in durable storage but is not live in the current
 * VM (e.g. after a Rivet actor slept and woke with a fresh VM). The sidecar runs
 * the stateless resume state machine (native session/load when the agent supports
 * it, else a fresh session/new + transcript continuation preamble). `cwd`/`env`
 * describe the fresh adapter launch used by the fallback tier. `transcriptPath`,
 * when present, is a guest-readable path the fallback preamble points the agent at.
 */
export type AcpResumeSessionRequest = {
    readonly sessionId: string
    readonly agentType: string
    readonly transcriptPath: string | null
    readonly cwd: string | null
    readonly env: ReadonlyMap<string, string> | null
}

export function readAcpResumeSessionRequest(bc: bare.ByteCursor): AcpResumeSessionRequest {
    return {
        sessionId: bare.readString(bc),
        agentType: bare.readString(bc),
        transcriptPath: read1(bc),
        cwd: read1(bc),
        env: read5(bc),
    }
}

export function writeAcpResumeSessionRequest(bc: bare.ByteCursor, x: AcpResumeSessionRequest): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    write1(bc, x.transcriptPath)
    write1(bc, x.cwd)
    write5(bc, x.env)
}

/**
 * Browser RESUMABLE path only (AGENTOS-WEB-ASYNC-AGENTS.md §3.2.1): the kernel
 * worker feeds a chunk of the agent's stdout into the in-flight create_session /
 * session/prompt handshake. The synchronous sidecar would block inside one
 * pushFrame; the resumable browser path returns between steps so the worker can
 * service the agent's own syscalls (incl. pi's net call for inference) on fresh,
 * non-nested pushFrames. `processId` is the handshake handle returned in the
 * AcpPendingResponse for the originating create/prompt request.
 */
export type AcpDeliverAgentOutputRequest = {
    readonly processId: string
    readonly chunk: ArrayBuffer
}

export function readAcpDeliverAgentOutputRequest(bc: bare.ByteCursor): AcpDeliverAgentOutputRequest {
    return {
        processId: bare.readString(bc),
        chunk: bare.readData(bc),
    }
}

export function writeAcpDeliverAgentOutputRequest(bc: bare.ByteCursor, x: AcpDeliverAgentOutputRequest): void {
    bare.writeString(bc, x.processId)
    bare.writeData(bc, x.chunk)
}

/**
 * Host-observed adapter stderr. The host forwards opaque bytes; the sidecar owns
 * session identity, limits, event construction, and retryable delivery.
 */
export type AcpDeliverAgentStderrRequest = {
    readonly processId: string
    readonly chunk: ArrayBuffer
}

export function readAcpDeliverAgentStderrRequest(bc: bare.ByteCursor): AcpDeliverAgentStderrRequest {
    return {
        processId: bare.readString(bc),
        chunk: bare.readData(bc),
    }
}

export function writeAcpDeliverAgentStderrRequest(bc: bare.ByteCursor, x: AcpDeliverAgentStderrRequest): void {
    bare.writeString(bc, x.processId)
    bare.writeData(bc, x.chunk)
}

/**
 * Browser RESUMABLE terminal cleanup. The sidecar owns the stable error code and
 * message for each observed terminal condition; the browser driver supplies only
 * the process handle and the fact it observed.
 */
export enum AcpPendingAbortReason {
    AgentExited = "AgentExited",
    InteractionTimeout = "InteractionTimeout",
    DriverFailed = "DriverFailed",
    CallerCancelled = "CallerCancelled",
}

export function readAcpPendingAbortReason(bc: bare.ByteCursor): AcpPendingAbortReason {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return AcpPendingAbortReason.AgentExited
        case 1:
            return AcpPendingAbortReason.InteractionTimeout
        case 2:
            return AcpPendingAbortReason.DriverFailed
        case 3:
            return AcpPendingAbortReason.CallerCancelled
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeAcpPendingAbortReason(bc: bare.ByteCursor, x: AcpPendingAbortReason): void {
    switch (x) {
        case AcpPendingAbortReason.AgentExited: {
            bare.writeU8(bc, 0)
            break
        }
        case AcpPendingAbortReason.InteractionTimeout: {
            bare.writeU8(bc, 1)
            break
        }
        case AcpPendingAbortReason.DriverFailed: {
            bare.writeU8(bc, 2)
            break
        }
        case AcpPendingAbortReason.CallerCancelled: {
            bare.writeU8(bc, 3)
            break
        }
    }
}

export type AcpAbortPendingRequest = {
    readonly processId: string
    readonly reason: AcpPendingAbortReason
    /**
     * Present only when the browser execution transport directly observed a
     * process exit status. Timeouts/driver failures and indirect exits omit it.
     */
    readonly exitCode: i32 | null
}

export function readAcpAbortPendingRequest(bc: bare.ByteCursor): AcpAbortPendingRequest {
    return {
        processId: bare.readString(bc),
        reason: readAcpPendingAbortReason(bc),
        exitCode: read6(bc),
    }
}

export function writeAcpAbortPendingRequest(bc: bare.ByteCursor, x: AcpAbortPendingRequest): void {
    bare.writeString(bc, x.processId)
    writeAcpPendingAbortReason(bc, x.reason)
    write6(bc, x.exitCode)
}

export type AcpRequest =
    | { readonly tag: "AcpCreateSessionRequest"; readonly val: AcpCreateSessionRequest }
    | { readonly tag: "AcpSessionRequest"; readonly val: AcpSessionRequest }
    | { readonly tag: "AcpSetSessionConfigRequest"; readonly val: AcpSetSessionConfigRequest }
    | { readonly tag: "AcpGetSessionStateRequest"; readonly val: AcpGetSessionStateRequest }
    | { readonly tag: "AcpListSessionsRequest"; readonly val: AcpListSessionsRequest }
    | { readonly tag: "AcpCloseSessionRequest"; readonly val: AcpCloseSessionRequest }
    | { readonly tag: "AcpResumeSessionRequest"; readonly val: AcpResumeSessionRequest }
    | { readonly tag: "AcpDeliverAgentOutputRequest"; readonly val: AcpDeliverAgentOutputRequest }
    | { readonly tag: "AcpDeliverAgentStderrRequest"; readonly val: AcpDeliverAgentStderrRequest }
    | { readonly tag: "AcpAbortPendingRequest"; readonly val: AcpAbortPendingRequest }
    | { readonly tag: "AcpListAgentsRequest"; readonly val: AcpListAgentsRequest }

export function readAcpRequest(bc: bare.ByteCursor): AcpRequest {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AcpCreateSessionRequest", val: readAcpCreateSessionRequest(bc) }
        case 1:
            return { tag: "AcpSessionRequest", val: readAcpSessionRequest(bc) }
        case 2:
            return { tag: "AcpSetSessionConfigRequest", val: readAcpSetSessionConfigRequest(bc) }
        case 3:
            return { tag: "AcpGetSessionStateRequest", val: readAcpGetSessionStateRequest(bc) }
        case 4:
            return { tag: "AcpListSessionsRequest", val: readAcpListSessionsRequest(bc) }
        case 5:
            return { tag: "AcpCloseSessionRequest", val: readAcpCloseSessionRequest(bc) }
        case 6:
            return { tag: "AcpResumeSessionRequest", val: readAcpResumeSessionRequest(bc) }
        case 7:
            return { tag: "AcpDeliverAgentOutputRequest", val: readAcpDeliverAgentOutputRequest(bc) }
        case 8:
            return { tag: "AcpDeliverAgentStderrRequest", val: readAcpDeliverAgentStderrRequest(bc) }
        case 9:
            return { tag: "AcpAbortPendingRequest", val: readAcpAbortPendingRequest(bc) }
        case 10:
            return { tag: "AcpListAgentsRequest", val: readAcpListAgentsRequest(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeAcpRequest(bc: bare.ByteCursor, x: AcpRequest): void {
    switch (x.tag) {
        case "AcpCreateSessionRequest": {
            bare.writeU8(bc, 0)
            writeAcpCreateSessionRequest(bc, x.val)
            break
        }
        case "AcpSessionRequest": {
            bare.writeU8(bc, 1)
            writeAcpSessionRequest(bc, x.val)
            break
        }
        case "AcpSetSessionConfigRequest": {
            bare.writeU8(bc, 2)
            writeAcpSetSessionConfigRequest(bc, x.val)
            break
        }
        case "AcpGetSessionStateRequest": {
            bare.writeU8(bc, 3)
            writeAcpGetSessionStateRequest(bc, x.val)
            break
        }
        case "AcpListSessionsRequest": {
            bare.writeU8(bc, 4)
            writeAcpListSessionsRequest(bc, x.val)
            break
        }
        case "AcpCloseSessionRequest": {
            bare.writeU8(bc, 5)
            writeAcpCloseSessionRequest(bc, x.val)
            break
        }
        case "AcpResumeSessionRequest": {
            bare.writeU8(bc, 6)
            writeAcpResumeSessionRequest(bc, x.val)
            break
        }
        case "AcpDeliverAgentOutputRequest": {
            bare.writeU8(bc, 7)
            writeAcpDeliverAgentOutputRequest(bc, x.val)
            break
        }
        case "AcpDeliverAgentStderrRequest": {
            bare.writeU8(bc, 8)
            writeAcpDeliverAgentStderrRequest(bc, x.val)
            break
        }
        case "AcpAbortPendingRequest": {
            bare.writeU8(bc, 9)
            writeAcpAbortPendingRequest(bc, x.val)
            break
        }
        case "AcpListAgentsRequest": {
            bare.writeU8(bc, 10)
            writeAcpListAgentsRequest(bc, x.val)
            break
        }
    }
}

export function encodeAcpRequest(x: AcpRequest, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeAcpRequest(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeAcpRequest(bytes: Uint8Array): AcpRequest {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readAcpRequest(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

function read10(bc: bare.ByteCursor): u32 | null {
    return bare.readBool(bc) ? bare.readU32(bc) : null
}

function write10(bc: bare.ByteCursor, x: u32 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeU32(bc, x)
    }
}

function read11(bc: bare.ByteCursor): readonly JsonUtf8[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readJsonUtf8(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readJsonUtf8(bc)
    }
    return result
}

function write11(bc: bare.ByteCursor, x: readonly JsonUtf8[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeJsonUtf8(bc, x[i])
    }
}

export type AcpSessionCreatedResponse = {
    readonly sessionId: string
    /**
     * Complete host-route identity. Clients install the route atomically when this
     * response frame arrives, before following bootstrap events are dispatched.
     */
    readonly agentType: string
    readonly processId: string
    readonly pid: u32 | null
    readonly modes: JsonUtf8 | null
    readonly configOptions: readonly JsonUtf8[]
    readonly agentCapabilities: JsonUtf8 | null
    readonly agentInfo: JsonUtf8 | null
}

export function readAcpSessionCreatedResponse(bc: bare.ByteCursor): AcpSessionCreatedResponse {
    return {
        sessionId: bare.readString(bc),
        agentType: bare.readString(bc),
        processId: bare.readString(bc),
        pid: read10(bc),
        modes: read7(bc),
        configOptions: read11(bc),
        agentCapabilities: read7(bc),
        agentInfo: read7(bc),
    }
}

export function writeAcpSessionCreatedResponse(bc: bare.ByteCursor, x: AcpSessionCreatedResponse): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    bare.writeString(bc, x.processId)
    write10(bc, x.pid)
    write7(bc, x.modes)
    write11(bc, x.configOptions)
    write7(bc, x.agentCapabilities)
    write7(bc, x.agentInfo)
}

export type AcpSessionRpcResponse = {
    readonly sessionId: string
    readonly response: JsonUtf8
    /**
     * Present for session/prompt. The sidecar accumulates agent_message_chunk
     * notifications while still streaming them as live session events.
     */
    readonly text: string | null
}

export function readAcpSessionRpcResponse(bc: bare.ByteCursor): AcpSessionRpcResponse {
    return {
        sessionId: bare.readString(bc),
        response: readJsonUtf8(bc),
        text: read1(bc),
    }
}

export function writeAcpSessionRpcResponse(bc: bare.ByteCursor, x: AcpSessionRpcResponse): void {
    bare.writeString(bc, x.sessionId)
    writeJsonUtf8(bc, x.response)
    write1(bc, x.text)
}

export type AcpSessionStateResponse = {
    readonly sessionId: string
    readonly agentType: string
    readonly processId: string
    readonly pid: u32 | null
    readonly closed: boolean
    readonly exitCode: i32 | null
    readonly modes: JsonUtf8 | null
    readonly configOptions: readonly JsonUtf8[]
    readonly agentCapabilities: JsonUtf8 | null
    readonly agentInfo: JsonUtf8 | null
}

export function readAcpSessionStateResponse(bc: bare.ByteCursor): AcpSessionStateResponse {
    return {
        sessionId: bare.readString(bc),
        agentType: bare.readString(bc),
        processId: bare.readString(bc),
        pid: read10(bc),
        closed: bare.readBool(bc),
        exitCode: read6(bc),
        modes: read7(bc),
        configOptions: read11(bc),
        agentCapabilities: read7(bc),
        agentInfo: read7(bc),
    }
}

export function writeAcpSessionStateResponse(bc: bare.ByteCursor, x: AcpSessionStateResponse): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    bare.writeString(bc, x.processId)
    write10(bc, x.pid)
    bare.writeBool(bc, x.closed)
    write6(bc, x.exitCode)
    write7(bc, x.modes)
    write11(bc, x.configOptions)
    write7(bc, x.agentCapabilities)
    write7(bc, x.agentInfo)
}

export type AcpSessionEntry = {
    readonly sessionId: string
    readonly agentType: string
}

export function readAcpSessionEntry(bc: bare.ByteCursor): AcpSessionEntry {
    return {
        sessionId: bare.readString(bc),
        agentType: bare.readString(bc),
    }
}

export function writeAcpSessionEntry(bc: bare.ByteCursor, x: AcpSessionEntry): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
}

function read12(bc: bare.ByteCursor): readonly AcpSessionEntry[] {
    const len = bare.readUintSafe(bc)
    if (len === 0) {
        return []
    }
    const result = [readAcpSessionEntry(bc)]
    for (let i = 1; i < len; i++) {
        result[i] = readAcpSessionEntry(bc)
    }
    return result
}

function write12(bc: bare.ByteCursor, x: readonly AcpSessionEntry[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeAcpSessionEntry(bc, x[i])
    }
}

export type AcpListSessionsResponse = {
    readonly sessions: readonly AcpSessionEntry[]
}

export function readAcpListSessionsResponse(bc: bare.ByteCursor): AcpListSessionsResponse {
    return {
        sessions: read12(bc),
    }
}

export function writeAcpListSessionsResponse(bc: bare.ByteCursor, x: AcpListSessionsResponse): void {
    write12(bc, x.sessions)
}

export type AcpSessionClosedResponse = {
    readonly sessionId: string
}

export function readAcpSessionClosedResponse(bc: bare.ByteCursor): AcpSessionClosedResponse {
    return {
        sessionId: bare.readString(bc),
    }
}

export function writeAcpSessionClosedResponse(bc: bare.ByteCursor, x: AcpSessionClosedResponse): void {
    bare.writeString(bc, x.sessionId)
}

export type AcpAgentStderrDeliveredResponse = {
    readonly processId: string
}

export function readAcpAgentStderrDeliveredResponse(bc: bare.ByteCursor): AcpAgentStderrDeliveredResponse {
    return {
        processId: bare.readString(bc),
    }
}

export function writeAcpAgentStderrDeliveredResponse(bc: bare.ByteCursor, x: AcpAgentStderrDeliveredResponse): void {
    bare.writeString(bc, x.processId)
}

/**
 * Result of AcpResumeSessionRequest. `sessionId` is the live ACP session id after
 * resume: equal to the requested id for native loads, or the freshly assigned id
 * for the fallback tier (the caller remaps external -> live). `mode` is "native"
 * (session/load|resume succeeded) or "fallback" (a new session was created and the
 * transcript-continuation preamble was armed for the next prompt).
 */
export type AcpSessionResumedResponse = {
    readonly sessionId: string
    readonly mode: string
    /**
     * Complete host-route identity for the newly live adapter/session.
     */
    readonly agentType: string
    readonly processId: string
    readonly pid: u32 | null
}

export function readAcpSessionResumedResponse(bc: bare.ByteCursor): AcpSessionResumedResponse {
    return {
        sessionId: bare.readString(bc),
        mode: bare.readString(bc),
        agentType: bare.readString(bc),
        processId: bare.readString(bc),
        pid: read10(bc),
    }
}

export function writeAcpSessionResumedResponse(bc: bare.ByteCursor, x: AcpSessionResumedResponse): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.mode)
    bare.writeString(bc, x.agentType)
    bare.writeString(bc, x.processId)
    write10(bc, x.pid)
}

export type AcpErrorResponse = {
    readonly code: string
    readonly message: string
}

export function readAcpErrorResponse(bc: bare.ByteCursor): AcpErrorResponse {
    return {
        code: bare.readString(bc),
        message: bare.readString(bc),
    }
}

export function writeAcpErrorResponse(bc: bare.ByteCursor, x: AcpErrorResponse): void {
    bare.writeString(bc, x.code)
    bare.writeString(bc, x.message)
}

/**
 * Browser RESUMABLE path: the create_session / session/prompt request (and each
 * AcpDeliverAgentOutputRequest that has not yet completed the handshake) returns
 * this, carrying the `processId` handle the kernel worker drives the interaction
 * with. The real result (AcpSessionCreatedResponse / AcpSessionRpcResponse) is
 * delivered as the response to the AcpDeliverAgentOutputRequest that completes it.
 */
export type AcpPendingResponse = {
    readonly processId: string
    /**
     * Sidecar-owned deadline for the currently awaited ACP phase.
     */
    readonly timeoutMs: u32
    /**
     * Stable phase identity. Browser routing resets the deadline only when this
     * changes; partial lines and same-phase notifications cannot extend it.
     */
    readonly timeoutPhase: string
}

export function readAcpPendingResponse(bc: bare.ByteCursor): AcpPendingResponse {
    return {
        processId: bare.readString(bc),
        timeoutMs: bare.readU32(bc),
        timeoutPhase: bare.readString(bc),
    }
}

export function writeAcpPendingResponse(bc: bare.ByteCursor, x: AcpPendingResponse): void {
    bare.writeString(bc, x.processId)
    bare.writeU32(bc, x.timeoutMs)
    bare.writeString(bc, x.timeoutPhase)
}

export type AcpResponse =
    | { readonly tag: "AcpSessionCreatedResponse"; readonly val: AcpSessionCreatedResponse }
    | { readonly tag: "AcpSessionRpcResponse"; readonly val: AcpSessionRpcResponse }
    | { readonly tag: "AcpSessionStateResponse"; readonly val: AcpSessionStateResponse }
    | { readonly tag: "AcpListSessionsResponse"; readonly val: AcpListSessionsResponse }
    | { readonly tag: "AcpSessionClosedResponse"; readonly val: AcpSessionClosedResponse }
    | { readonly tag: "AcpAgentStderrDeliveredResponse"; readonly val: AcpAgentStderrDeliveredResponse }
    | { readonly tag: "AcpSessionResumedResponse"; readonly val: AcpSessionResumedResponse }
    | { readonly tag: "AcpErrorResponse"; readonly val: AcpErrorResponse }
    | { readonly tag: "AcpPendingResponse"; readonly val: AcpPendingResponse }
    | { readonly tag: "AcpListAgentsResponse"; readonly val: AcpListAgentsResponse }

export function readAcpResponse(bc: bare.ByteCursor): AcpResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AcpSessionCreatedResponse", val: readAcpSessionCreatedResponse(bc) }
        case 1:
            return { tag: "AcpSessionRpcResponse", val: readAcpSessionRpcResponse(bc) }
        case 2:
            return { tag: "AcpSessionStateResponse", val: readAcpSessionStateResponse(bc) }
        case 3:
            return { tag: "AcpListSessionsResponse", val: readAcpListSessionsResponse(bc) }
        case 4:
            return { tag: "AcpSessionClosedResponse", val: readAcpSessionClosedResponse(bc) }
        case 5:
            return { tag: "AcpAgentStderrDeliveredResponse", val: readAcpAgentStderrDeliveredResponse(bc) }
        case 6:
            return { tag: "AcpSessionResumedResponse", val: readAcpSessionResumedResponse(bc) }
        case 7:
            return { tag: "AcpErrorResponse", val: readAcpErrorResponse(bc) }
        case 8:
            return { tag: "AcpPendingResponse", val: readAcpPendingResponse(bc) }
        case 9:
            return { tag: "AcpListAgentsResponse", val: readAcpListAgentsResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeAcpResponse(bc: bare.ByteCursor, x: AcpResponse): void {
    switch (x.tag) {
        case "AcpSessionCreatedResponse": {
            bare.writeU8(bc, 0)
            writeAcpSessionCreatedResponse(bc, x.val)
            break
        }
        case "AcpSessionRpcResponse": {
            bare.writeU8(bc, 1)
            writeAcpSessionRpcResponse(bc, x.val)
            break
        }
        case "AcpSessionStateResponse": {
            bare.writeU8(bc, 2)
            writeAcpSessionStateResponse(bc, x.val)
            break
        }
        case "AcpListSessionsResponse": {
            bare.writeU8(bc, 3)
            writeAcpListSessionsResponse(bc, x.val)
            break
        }
        case "AcpSessionClosedResponse": {
            bare.writeU8(bc, 4)
            writeAcpSessionClosedResponse(bc, x.val)
            break
        }
        case "AcpAgentStderrDeliveredResponse": {
            bare.writeU8(bc, 5)
            writeAcpAgentStderrDeliveredResponse(bc, x.val)
            break
        }
        case "AcpSessionResumedResponse": {
            bare.writeU8(bc, 6)
            writeAcpSessionResumedResponse(bc, x.val)
            break
        }
        case "AcpErrorResponse": {
            bare.writeU8(bc, 7)
            writeAcpErrorResponse(bc, x.val)
            break
        }
        case "AcpPendingResponse": {
            bare.writeU8(bc, 8)
            writeAcpPendingResponse(bc, x.val)
            break
        }
        case "AcpListAgentsResponse": {
            bare.writeU8(bc, 9)
            writeAcpListAgentsResponse(bc, x.val)
            break
        }
    }
}

export function encodeAcpResponse(x: AcpResponse, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeAcpResponse(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeAcpResponse(bytes: Uint8Array): AcpResponse {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readAcpResponse(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

export type AcpSessionEvent = {
    readonly sessionId: string
    readonly notification: JsonUtf8
}

export function readAcpSessionEvent(bc: bare.ByteCursor): AcpSessionEvent {
    return {
        sessionId: bare.readString(bc),
        notification: readJsonUtf8(bc),
    }
}

export function writeAcpSessionEvent(bc: bare.ByteCursor, x: AcpSessionEvent): void {
    bare.writeString(bc, x.sessionId)
    writeJsonUtf8(bc, x.notification)
}

export type AcpAgentStderrEvent = {
    readonly sessionId: string
    readonly agentType: string
    readonly processId: string
    readonly chunk: ArrayBuffer
}

export function readAcpAgentStderrEvent(bc: bare.ByteCursor): AcpAgentStderrEvent {
    return {
        sessionId: bare.readString(bc),
        agentType: bare.readString(bc),
        processId: bare.readString(bc),
        chunk: bare.readData(bc),
    }
}

export function writeAcpAgentStderrEvent(bc: bare.ByteCursor, x: AcpAgentStderrEvent): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    bare.writeString(bc, x.processId)
    bare.writeData(bc, x.chunk)
}

/**
 * Emitted when the ACP adapter process exits without an explicit close_session —
 * a crash from the host's perspective (any spontaneous exit, including code 0).
 * `restart` reports the sidecar's bounded auto-restart outcome:
 *   "restarted"   — adapter respawned and the session was natively re-attached
 *                   (session/load | session/resume) under the same sessionId;
 *                   the session stays live.
 *   "unsupported" — the respawned adapter does not advertise a native resume
 *                   capability; the session record was evicted.
 *   "failed"      — the respawn or the native re-attach errored; evicted.
 *   "exhausted"   — maxRestarts was already spent for this session; evicted.
 * `exitCode` is absent when the exit was observed indirectly (e.g. a write to
 * the adapter's stdin failed because the process was already gone).
 */
export type AcpAgentExitedEvent = {
    readonly sessionId: string
    readonly agentType: string
    readonly processId: string
    readonly exitCode: i32 | null
    readonly restart: string
    readonly restartCount: u32
    readonly maxRestarts: u32
}

export function readAcpAgentExitedEvent(bc: bare.ByteCursor): AcpAgentExitedEvent {
    return {
        sessionId: bare.readString(bc),
        agentType: bare.readString(bc),
        processId: bare.readString(bc),
        exitCode: read6(bc),
        restart: bare.readString(bc),
        restartCount: bare.readU32(bc),
        maxRestarts: bare.readU32(bc),
    }
}

export function writeAcpAgentExitedEvent(bc: bare.ByteCursor, x: AcpAgentExitedEvent): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    bare.writeString(bc, x.processId)
    write6(bc, x.exitCode)
    bare.writeString(bc, x.restart)
    bare.writeU32(bc, x.restartCount)
    bare.writeU32(bc, x.maxRestarts)
}

export type AcpEvent =
    | { readonly tag: "AcpSessionEvent"; readonly val: AcpSessionEvent }
    | { readonly tag: "AcpAgentStderrEvent"; readonly val: AcpAgentStderrEvent }
    | { readonly tag: "AcpAgentExitedEvent"; readonly val: AcpAgentExitedEvent }

export function readAcpEvent(bc: bare.ByteCursor): AcpEvent {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AcpSessionEvent", val: readAcpSessionEvent(bc) }
        case 1:
            return { tag: "AcpAgentStderrEvent", val: readAcpAgentStderrEvent(bc) }
        case 2:
            return { tag: "AcpAgentExitedEvent", val: readAcpAgentExitedEvent(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeAcpEvent(bc: bare.ByteCursor, x: AcpEvent): void {
    switch (x.tag) {
        case "AcpSessionEvent": {
            bare.writeU8(bc, 0)
            writeAcpSessionEvent(bc, x.val)
            break
        }
        case "AcpAgentStderrEvent": {
            bare.writeU8(bc, 1)
            writeAcpAgentStderrEvent(bc, x.val)
            break
        }
        case "AcpAgentExitedEvent": {
            bare.writeU8(bc, 2)
            writeAcpAgentExitedEvent(bc, x.val)
            break
        }
    }
}

export function encodeAcpEvent(x: AcpEvent, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeAcpEvent(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeAcpEvent(bytes: Uint8Array): AcpEvent {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readAcpEvent(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

export type AcpPermissionCallback = {
    readonly sessionId: string
    readonly permissionId: string
    readonly params: JsonUtf8
    /**
     * Sidecar-owned host bookkeeping deadline. This is strictly later than the
     * sidecar's authoritative permission decision timeout; clients must not use
     * it to choose the permission default.
     */
    readonly cleanupAfterMs: u64
}

export function readAcpPermissionCallback(bc: bare.ByteCursor): AcpPermissionCallback {
    return {
        sessionId: bare.readString(bc),
        permissionId: bare.readString(bc),
        params: readJsonUtf8(bc),
        cleanupAfterMs: bare.readU64(bc),
    }
}

export function writeAcpPermissionCallback(bc: bare.ByteCursor, x: AcpPermissionCallback): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.permissionId)
    writeJsonUtf8(bc, x.params)
    bare.writeU64(bc, x.cleanupAfterMs)
}

export type AcpCallback =
    | { readonly tag: "AcpPermissionCallback"; readonly val: AcpPermissionCallback }

export function readAcpCallback(bc: bare.ByteCursor): AcpCallback {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AcpPermissionCallback", val: readAcpPermissionCallback(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeAcpCallback(bc: bare.ByteCursor, x: AcpCallback): void {
    switch (x.tag) {
        case "AcpPermissionCallback": {
            bare.writeU8(bc, 0)
            writeAcpPermissionCallback(bc, x.val)
            break
        }
    }
}

export function encodeAcpCallback(x: AcpCallback, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeAcpCallback(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeAcpCallback(bytes: Uint8Array): AcpCallback {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readAcpCallback(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}

export type AcpPermissionCallbackResponse = {
    readonly permissionId: string
    /**
     * The client supplies only an explicit host answer. The ACP sidecar owns the
     * default behavior when the route is absent, times out, or fails.
     */
    readonly reply: string | null
}

export function readAcpPermissionCallbackResponse(bc: bare.ByteCursor): AcpPermissionCallbackResponse {
    return {
        permissionId: bare.readString(bc),
        reply: read1(bc),
    }
}

export function writeAcpPermissionCallbackResponse(bc: bare.ByteCursor, x: AcpPermissionCallbackResponse): void {
    bare.writeString(bc, x.permissionId)
    write1(bc, x.reply)
}

export type AcpCallbackResponse =
    | { readonly tag: "AcpPermissionCallbackResponse"; readonly val: AcpPermissionCallbackResponse }

export function readAcpCallbackResponse(bc: bare.ByteCursor): AcpCallbackResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AcpPermissionCallbackResponse", val: readAcpPermissionCallbackResponse(bc) }
        default: {
            bc.offset = offset
            throw new bare.BareError(offset, "invalid tag")
        }
    }
}

export function writeAcpCallbackResponse(bc: bare.ByteCursor, x: AcpCallbackResponse): void {
    switch (x.tag) {
        case "AcpPermissionCallbackResponse": {
            bare.writeU8(bc, 0)
            writeAcpPermissionCallbackResponse(bc, x.val)
            break
        }
    }
}

export function encodeAcpCallbackResponse(x: AcpCallbackResponse, config?: Partial<bare.Config>): Uint8Array {
    const fullConfig = config != null ? bare.Config(config) : DEFAULT_CONFIG
    const bc = new bare.ByteCursor(
        new Uint8Array(fullConfig.initialBufferLength),
        fullConfig,
    )
    writeAcpCallbackResponse(bc, x)
    return new Uint8Array(bc.view.buffer, bc.view.byteOffset, bc.offset)
}

export function decodeAcpCallbackResponse(bytes: Uint8Array): AcpCallbackResponse {
    const bc = new bare.ByteCursor(bytes, DEFAULT_CONFIG)
    const result = readAcpCallbackResponse(bc)
    if (bc.offset < bc.view.byteLength) {
        throw new bare.BareError(bc.offset, "remaining bytes")
    }
    return result
}
