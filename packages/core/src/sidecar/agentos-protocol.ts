// @generated - run pnpm --dir packages/core build:agentos-protocol
import * as bare from "@rivetkit/bare-ts"

const DEFAULT_CONFIG = /* @__PURE__ */ bare.Config({})

export type i32 = number
export type u32 = number

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

function read0(bc: bare.ByteCursor): readonly string[] {
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

function write0(bc: bare.ByteCursor, x: readonly string[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        bare.writeString(bc, x[i])
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

function read2(bc: bare.ByteCursor): string | null {
    return bare.readBool(bc) ? bare.readString(bc) : null
}

function write2(bc: bare.ByteCursor, x: string | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeString(bc, x)
    }
}

export type AcpCreateSessionRequest = {
    readonly agentType: string
    readonly runtime: AcpRuntimeKind
    readonly cwd: string
    readonly args: readonly string[]
    readonly env: ReadonlyMap<string, string>
    readonly protocolVersion: i32
    readonly clientCapabilities: JsonUtf8
    readonly mcpServers: JsonUtf8
    readonly skipOsInstructions: boolean
    readonly additionalInstructions: string | null
}

export function readAcpCreateSessionRequest(bc: bare.ByteCursor): AcpCreateSessionRequest {
    return {
        agentType: bare.readString(bc),
        runtime: readAcpRuntimeKind(bc),
        cwd: bare.readString(bc),
        args: read0(bc),
        env: read1(bc),
        protocolVersion: bare.readI32(bc),
        clientCapabilities: readJsonUtf8(bc),
        mcpServers: readJsonUtf8(bc),
        skipOsInstructions: bare.readBool(bc),
        additionalInstructions: read2(bc),
    }
}

export function writeAcpCreateSessionRequest(bc: bare.ByteCursor, x: AcpCreateSessionRequest): void {
    bare.writeString(bc, x.agentType)
    writeAcpRuntimeKind(bc, x.runtime)
    bare.writeString(bc, x.cwd)
    write0(bc, x.args)
    write1(bc, x.env)
    bare.writeI32(bc, x.protocolVersion)
    writeJsonUtf8(bc, x.clientCapabilities)
    writeJsonUtf8(bc, x.mcpServers)
    bare.writeBool(bc, x.skipOsInstructions)
    write2(bc, x.additionalInstructions)
}

function read3(bc: bare.ByteCursor): JsonUtf8 | null {
    return bare.readBool(bc) ? readJsonUtf8(bc) : null
}

function write3(bc: bare.ByteCursor, x: JsonUtf8 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        writeJsonUtf8(bc, x)
    }
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
        params: read3(bc),
    }
}

export function writeAcpSessionRequest(bc: bare.ByteCursor, x: AcpSessionRequest): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.method)
    write3(bc, x.params)
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

function read4(bc: bare.ByteCursor): readonly AcpAgentEntry[] {
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

function write4(bc: bare.ByteCursor, x: readonly AcpAgentEntry[]): void {
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
        agents: read4(bc),
    }
}

export function writeAcpListAgentsResponse(bc: bare.ByteCursor, x: AcpListAgentsResponse): void {
    write4(bc, x.agents)
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
    readonly cwd: string
    readonly env: ReadonlyMap<string, string>
}

export function readAcpResumeSessionRequest(bc: bare.ByteCursor): AcpResumeSessionRequest {
    return {
        sessionId: bare.readString(bc),
        agentType: bare.readString(bc),
        transcriptPath: read2(bc),
        cwd: bare.readString(bc),
        env: read1(bc),
    }
}

export function writeAcpResumeSessionRequest(bc: bare.ByteCursor, x: AcpResumeSessionRequest): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    write2(bc, x.transcriptPath)
    bare.writeString(bc, x.cwd)
    write1(bc, x.env)
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

export type AcpRequest =
    | { readonly tag: "AcpCreateSessionRequest"; readonly val: AcpCreateSessionRequest }
    | { readonly tag: "AcpSessionRequest"; readonly val: AcpSessionRequest }
    | { readonly tag: "AcpGetSessionStateRequest"; readonly val: AcpGetSessionStateRequest }
    | { readonly tag: "AcpCloseSessionRequest"; readonly val: AcpCloseSessionRequest }
    | { readonly tag: "AcpResumeSessionRequest"; readonly val: AcpResumeSessionRequest }
    | { readonly tag: "AcpDeliverAgentOutputRequest"; readonly val: AcpDeliverAgentOutputRequest }
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
            return { tag: "AcpGetSessionStateRequest", val: readAcpGetSessionStateRequest(bc) }
        case 3:
            return { tag: "AcpCloseSessionRequest", val: readAcpCloseSessionRequest(bc) }
        case 4:
            return { tag: "AcpResumeSessionRequest", val: readAcpResumeSessionRequest(bc) }
        case 5:
            return { tag: "AcpDeliverAgentOutputRequest", val: readAcpDeliverAgentOutputRequest(bc) }
        case 6:
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
        case "AcpGetSessionStateRequest": {
            bare.writeU8(bc, 2)
            writeAcpGetSessionStateRequest(bc, x.val)
            break
        }
        case "AcpCloseSessionRequest": {
            bare.writeU8(bc, 3)
            writeAcpCloseSessionRequest(bc, x.val)
            break
        }
        case "AcpResumeSessionRequest": {
            bare.writeU8(bc, 4)
            writeAcpResumeSessionRequest(bc, x.val)
            break
        }
        case "AcpDeliverAgentOutputRequest": {
            bare.writeU8(bc, 5)
            writeAcpDeliverAgentOutputRequest(bc, x.val)
            break
        }
        case "AcpListAgentsRequest": {
            bare.writeU8(bc, 6)
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

function read5(bc: bare.ByteCursor): u32 | null {
    return bare.readBool(bc) ? bare.readU32(bc) : null
}

function write5(bc: bare.ByteCursor, x: u32 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeU32(bc, x)
    }
}

function read6(bc: bare.ByteCursor): readonly JsonUtf8[] {
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

function write6(bc: bare.ByteCursor, x: readonly JsonUtf8[]): void {
    bare.writeUintSafe(bc, x.length)
    for (let i = 0; i < x.length; i++) {
        writeJsonUtf8(bc, x[i])
    }
}

export type AcpSessionCreatedResponse = {
    readonly sessionId: string
    readonly pid: u32 | null
    readonly modes: JsonUtf8 | null
    readonly configOptions: readonly JsonUtf8[]
    readonly agentCapabilities: JsonUtf8 | null
    readonly agentInfo: JsonUtf8 | null
}

export function readAcpSessionCreatedResponse(bc: bare.ByteCursor): AcpSessionCreatedResponse {
    return {
        sessionId: bare.readString(bc),
        pid: read5(bc),
        modes: read3(bc),
        configOptions: read6(bc),
        agentCapabilities: read3(bc),
        agentInfo: read3(bc),
    }
}

export function writeAcpSessionCreatedResponse(bc: bare.ByteCursor, x: AcpSessionCreatedResponse): void {
    bare.writeString(bc, x.sessionId)
    write5(bc, x.pid)
    write3(bc, x.modes)
    write6(bc, x.configOptions)
    write3(bc, x.agentCapabilities)
    write3(bc, x.agentInfo)
}

export type AcpSessionRpcResponse = {
    readonly sessionId: string
    readonly response: JsonUtf8
}

export function readAcpSessionRpcResponse(bc: bare.ByteCursor): AcpSessionRpcResponse {
    return {
        sessionId: bare.readString(bc),
        response: readJsonUtf8(bc),
    }
}

export function writeAcpSessionRpcResponse(bc: bare.ByteCursor, x: AcpSessionRpcResponse): void {
    bare.writeString(bc, x.sessionId)
    writeJsonUtf8(bc, x.response)
}

function read7(bc: bare.ByteCursor): i32 | null {
    return bare.readBool(bc) ? bare.readI32(bc) : null
}

function write7(bc: bare.ByteCursor, x: i32 | null): void {
    bare.writeBool(bc, x != null)
    if (x != null) {
        bare.writeI32(bc, x)
    }
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
        pid: read5(bc),
        closed: bare.readBool(bc),
        exitCode: read7(bc),
        modes: read3(bc),
        configOptions: read6(bc),
        agentCapabilities: read3(bc),
        agentInfo: read3(bc),
    }
}

export function writeAcpSessionStateResponse(bc: bare.ByteCursor, x: AcpSessionStateResponse): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    bare.writeString(bc, x.processId)
    write5(bc, x.pid)
    bare.writeBool(bc, x.closed)
    write7(bc, x.exitCode)
    write3(bc, x.modes)
    write6(bc, x.configOptions)
    write3(bc, x.agentCapabilities)
    write3(bc, x.agentInfo)
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
}

export function readAcpSessionResumedResponse(bc: bare.ByteCursor): AcpSessionResumedResponse {
    return {
        sessionId: bare.readString(bc),
        mode: bare.readString(bc),
    }
}

export function writeAcpSessionResumedResponse(bc: bare.ByteCursor, x: AcpSessionResumedResponse): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.mode)
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
}

export function readAcpPendingResponse(bc: bare.ByteCursor): AcpPendingResponse {
    return {
        processId: bare.readString(bc),
    }
}

export function writeAcpPendingResponse(bc: bare.ByteCursor, x: AcpPendingResponse): void {
    bare.writeString(bc, x.processId)
}

export type AcpResponse =
    | { readonly tag: "AcpSessionCreatedResponse"; readonly val: AcpSessionCreatedResponse }
    | { readonly tag: "AcpSessionRpcResponse"; readonly val: AcpSessionRpcResponse }
    | { readonly tag: "AcpSessionStateResponse"; readonly val: AcpSessionStateResponse }
    | { readonly tag: "AcpSessionClosedResponse"; readonly val: AcpSessionClosedResponse }
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
            return { tag: "AcpSessionClosedResponse", val: readAcpSessionClosedResponse(bc) }
        case 4:
            return { tag: "AcpSessionResumedResponse", val: readAcpSessionResumedResponse(bc) }
        case 5:
            return { tag: "AcpErrorResponse", val: readAcpErrorResponse(bc) }
        case 6:
            return { tag: "AcpPendingResponse", val: readAcpPendingResponse(bc) }
        case 7:
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
        case "AcpSessionClosedResponse": {
            bare.writeU8(bc, 3)
            writeAcpSessionClosedResponse(bc, x.val)
            break
        }
        case "AcpSessionResumedResponse": {
            bare.writeU8(bc, 4)
            writeAcpSessionResumedResponse(bc, x.val)
            break
        }
        case "AcpErrorResponse": {
            bare.writeU8(bc, 5)
            writeAcpErrorResponse(bc, x.val)
            break
        }
        case "AcpPendingResponse": {
            bare.writeU8(bc, 6)
            writeAcpPendingResponse(bc, x.val)
            break
        }
        case "AcpListAgentsResponse": {
            bare.writeU8(bc, 7)
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
        exitCode: read7(bc),
        restart: bare.readString(bc),
        restartCount: bare.readU32(bc),
        maxRestarts: bare.readU32(bc),
    }
}

export function writeAcpAgentExitedEvent(bc: bare.ByteCursor, x: AcpAgentExitedEvent): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.agentType)
    bare.writeString(bc, x.processId)
    write7(bc, x.exitCode)
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
}

export function readAcpPermissionCallback(bc: bare.ByteCursor): AcpPermissionCallback {
    return {
        sessionId: bare.readString(bc),
        permissionId: bare.readString(bc),
        params: readJsonUtf8(bc),
    }
}

export function writeAcpPermissionCallback(bc: bare.ByteCursor, x: AcpPermissionCallback): void {
    bare.writeString(bc, x.sessionId)
    bare.writeString(bc, x.permissionId)
    writeJsonUtf8(bc, x.params)
}

export type AcpHostRequestCallback = {
    readonly sessionId: string
    readonly request: JsonUtf8
}

export function readAcpHostRequestCallback(bc: bare.ByteCursor): AcpHostRequestCallback {
    return {
        sessionId: bare.readString(bc),
        request: readJsonUtf8(bc),
    }
}

export function writeAcpHostRequestCallback(bc: bare.ByteCursor, x: AcpHostRequestCallback): void {
    bare.writeString(bc, x.sessionId)
    writeJsonUtf8(bc, x.request)
}

export type AcpCallback =
    | { readonly tag: "AcpPermissionCallback"; readonly val: AcpPermissionCallback }
    | { readonly tag: "AcpHostRequestCallback"; readonly val: AcpHostRequestCallback }

export function readAcpCallback(bc: bare.ByteCursor): AcpCallback {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AcpPermissionCallback", val: readAcpPermissionCallback(bc) }
        case 1:
            return { tag: "AcpHostRequestCallback", val: readAcpHostRequestCallback(bc) }
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
        case "AcpHostRequestCallback": {
            bare.writeU8(bc, 1)
            writeAcpHostRequestCallback(bc, x.val)
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
    readonly reply: string
}

export function readAcpPermissionCallbackResponse(bc: bare.ByteCursor): AcpPermissionCallbackResponse {
    return {
        permissionId: bare.readString(bc),
        reply: bare.readString(bc),
    }
}

export function writeAcpPermissionCallbackResponse(bc: bare.ByteCursor, x: AcpPermissionCallbackResponse): void {
    bare.writeString(bc, x.permissionId)
    bare.writeString(bc, x.reply)
}

export type AcpHostRequestCallbackResponse = {
    readonly response: JsonUtf8 | null
}

export function readAcpHostRequestCallbackResponse(bc: bare.ByteCursor): AcpHostRequestCallbackResponse {
    return {
        response: read3(bc),
    }
}

export function writeAcpHostRequestCallbackResponse(bc: bare.ByteCursor, x: AcpHostRequestCallbackResponse): void {
    write3(bc, x.response)
}

export type AcpCallbackResponse =
    | { readonly tag: "AcpPermissionCallbackResponse"; readonly val: AcpPermissionCallbackResponse }
    | { readonly tag: "AcpHostRequestCallbackResponse"; readonly val: AcpHostRequestCallbackResponse }

export function readAcpCallbackResponse(bc: bare.ByteCursor): AcpCallbackResponse {
    const offset = bc.offset
    const tag = bare.readU8(bc)
    switch (tag) {
        case 0:
            return { tag: "AcpPermissionCallbackResponse", val: readAcpPermissionCallbackResponse(bc) }
        case 1:
            return { tag: "AcpHostRequestCallbackResponse", val: readAcpHostRequestCallbackResponse(bc) }
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
        case "AcpHostRequestCallbackResponse": {
            bare.writeU8(bc, 1)
            writeAcpHostRequestCallbackResponse(bc, x.val)
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
