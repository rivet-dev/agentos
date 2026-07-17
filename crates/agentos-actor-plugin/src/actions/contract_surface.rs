#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyShape {
    Unit,
    String,
    Bool,
    Number,
    Uint8Array,
    Array,
    NullableArray,
    Object(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct ActionContract {
    pub name: &'static str,
    pub reply_shape: ReplyShape,
    pub ts_signature: &'static str,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct EventContract {
    pub name: &'static str,
    pub payload_shape: ReplyShape,
    pub ts_signature: &'static str,
}

pub const GENERATED_ACTOR_ACTIONS_PATH: &str =
    "packages/agentos/src/generated/actor-actions.generated.ts";

pub const ACTION_CONTRACTS: &[ActionContract] = &[
    ActionContract {
        name: "readFile",
        reply_shape: ReplyShape::Uint8Array,
        ts_signature: "readFile: (c: Ctx, path: string) => Promise<Uint8Array>;",
    },
    ActionContract {
        name: "writeFile",
        reply_shape: ReplyShape::Unit,
        ts_signature:
            "writeFile: (c: Ctx, path: string, content: string | Uint8Array) => Promise<void>;",
    },
    ActionContract {
        name: "stat",
        reply_shape: ReplyShape::Object(&["atimeMs", "birthtimeMs", "blocks", "ctimeMs", "dev", "gid", "ino", "isDirectory", "isSymbolicLink", "mode", "mtimeMs", "nlink", "rdev", "size", "uid"]),
        ts_signature: "stat: (c: Ctx, path: string) => Promise<VirtualStat>;",
    },
    ActionContract {
        name: "mkdir",
        reply_shape: ReplyShape::Unit,
        ts_signature: "mkdir: (c: Ctx, path: string) => Promise<void>;",
    },
    ActionContract {
        name: "readdir",
        reply_shape: ReplyShape::Array,
        ts_signature: "readdir: (c: Ctx, path: string) => Promise<string[]>;",
    },
    ActionContract {
        name: "readdirEntries",
        reply_shape: ReplyShape::NullableArray,
        ts_signature: "readdirEntries: (c: Ctx, path: string) => Promise<ReaddirEntry[] | null>;",
    },
    ActionContract {
        name: "exists",
        reply_shape: ReplyShape::Bool,
        ts_signature: "exists: (c: Ctx, path: string) => Promise<boolean>;",
    },
    ActionContract {
        name: "move",
        reply_shape: ReplyShape::Unit,
        ts_signature: "move: (c: Ctx, from: string, to: string) => Promise<void>;",
    },
    ActionContract {
        name: "deleteFile",
        reply_shape: ReplyShape::Unit,
        ts_signature:
            "deleteFile: (c: Ctx, path: string, options?: { recursive?: boolean }) => Promise<void>;",
    },
    ActionContract {
        name: "writeFiles",
        reply_shape: ReplyShape::Array,
        ts_signature:
            "writeFiles: ( c: Ctx, entries: { path: string; content: string | Uint8Array }[], ) => Promise<WriteFileResult[]>;",
    },
    ActionContract {
        name: "readFiles",
        reply_shape: ReplyShape::Array,
        ts_signature: "readFiles: (c: Ctx, paths: string[]) => Promise<ReadFileResult[]>;",
    },
    ActionContract {
        name: "readdirRecursive",
        reply_shape: ReplyShape::Array,
        ts_signature: "readdirRecursive: (c: Ctx, path: string) => Promise<DirEntry[]>;",
    },
    ActionContract {
        name: "exec",
        reply_shape: ReplyShape::Object(&["exitCode", "stderr", "stdout"]),
        ts_signature: "exec: ( c: Ctx, command: string, options?: ExecActionOptions, ) => Promise<ExecResult>;",
    },
    ActionContract {
        name: "spawn",
        reply_shape: ReplyShape::Object(&["pid"]),
        ts_signature: "spawn: ( c: Ctx, command: string, args: string[], options?: SpawnActionOptions, ) => Promise<SpawnedProcess>;",
    },
    ActionContract {
        name: "waitProcess",
        reply_shape: ReplyShape::Number,
        ts_signature: "waitProcess: (c: Ctx, pid: number) => Promise<number>;",
    },
    ActionContract {
        name: "killProcess",
        reply_shape: ReplyShape::Unit,
        ts_signature: "killProcess: (c: Ctx, pid: number) => Promise<void>;",
    },
    ActionContract {
        name: "stopProcess",
        reply_shape: ReplyShape::Unit,
        ts_signature: "stopProcess: (c: Ctx, pid: number) => Promise<void>;",
    },
    ActionContract {
        name: "listProcesses",
        reply_shape: ReplyShape::Array,
        ts_signature: "listProcesses: (c: Ctx) => Promise<SpawnedProcessInfo[]>;",
    },
    ActionContract {
        name: "allProcesses",
        reply_shape: ReplyShape::Array,
        ts_signature: "allProcesses: (c: Ctx) => Promise<ProcessInfo[]>;",
    },
    ActionContract {
        name: "processTree",
        reply_shape: ReplyShape::Array,
        ts_signature: "processTree: (c: Ctx) => Promise<ProcessTreeNode[]>;",
    },
    ActionContract {
        name: "getProcess",
        reply_shape: ReplyShape::Object(&["args", "command", "exitCode", "pid", "running", "startedAt"]),
        ts_signature: "getProcess: (c: Ctx, pid: number) => Promise<SpawnedProcessInfo>;",
    },
    ActionContract {
        name: "writeProcessStdin",
        reply_shape: ReplyShape::Unit,
        ts_signature:
            "writeProcessStdin: (c: Ctx, pid: number, data: string | Uint8Array) => Promise<void>;",
    },
    ActionContract {
        name: "closeProcessStdin",
        reply_shape: ReplyShape::Unit,
        ts_signature: "closeProcessStdin: (c: Ctx, pid: number) => Promise<void>;",
    },
    ActionContract {
        name: "openShell",
        reply_shape: ReplyShape::Object(&["shellId"]),
        ts_signature: "openShell: (c: Ctx, options?: OpenShellActionOptions) => Promise<OpenShellResult>;",
    },
    ActionContract {
        name: "writeShell",
        reply_shape: ReplyShape::Unit,
        ts_signature:
            "writeShell: (c: Ctx, shellId: string, data: string | Uint8Array) => Promise<void>;",
    },
    ActionContract {
        name: "resizeShell",
        reply_shape: ReplyShape::Unit,
        ts_signature: "resizeShell: (c: Ctx, shellId: string, cols: number, rows: number) => Promise<void>;",
    },
    ActionContract {
        name: "closeShell",
        reply_shape: ReplyShape::Unit,
        ts_signature: "closeShell: (c: Ctx, shellId: string) => Promise<void>;",
    },
    ActionContract {
        name: "waitShell",
        reply_shape: ReplyShape::Number,
        ts_signature: "waitShell: (c: Ctx, shellId: string) => Promise<number>;",
    },
    ActionContract {
        name: "vmFetch",
        reply_shape: ReplyShape::Object(&["body", "headers", "status", "statusText"]),
        ts_signature: "vmFetch: ( c: Ctx, port: number, url: string, options?: VmFetchOptions, ) => Promise<VmFetchResponse>;",
    },
    ActionContract {
        name: "scheduleCron",
        reply_shape: ReplyShape::Object(&["id"]),
        ts_signature:
            "scheduleCron: (c: Ctx, options: SerializableCronJobOptions) => Promise<ScheduledCronJob>;",
    },
    ActionContract {
        name: "listCronJobs",
        reply_shape: ReplyShape::Array,
        ts_signature: "listCronJobs: (c: Ctx) => Promise<SerializableCronJobInfo[]>;",
    },
    ActionContract {
        name: "cancelCronJob",
        reply_shape: ReplyShape::Unit,
        ts_signature: "cancelCronJob: (c: Ctx, id: string) => Promise<void>;",
    },
    ActionContract {
        name: "createSession",
        reply_shape: ReplyShape::String,
        ts_signature:
            "createSession: (c: Ctx, agentType: string, options?: CreateSessionOptions) => Promise<string>;",
    },
    ActionContract {
        name: "sendPrompt",
        reply_shape: ReplyShape::Object(&["response", "text"]),
        ts_signature: "sendPrompt: (c: Ctx, sessionId: string, text: string) => Promise<PromptResult>;",
    },
    ActionContract {
        name: "closeSession",
        reply_shape: ReplyShape::Unit,
        ts_signature: "closeSession: (c: Ctx, sessionId: string) => Promise<void>;",
    },
    ActionContract {
        name: "listPersistedSessions",
        reply_shape: ReplyShape::Array,
        ts_signature: "listPersistedSessions: (c: Ctx) => Promise<PersistedSessionRecord[]>;",
    },
    ActionContract {
        name: "getSessionEvents",
        reply_shape: ReplyShape::Array,
        ts_signature:
            "getSessionEvents: (c: Ctx, sessionId: string) => Promise<PersistedSessionEvent[]>;",
    },
    ActionContract {
        name: "respondPermission",
        reply_shape: ReplyShape::Unit,
        ts_signature:
            "respondPermission: ( c: Ctx, sessionId: string, permissionId: string, reply: PermissionReply, ) => Promise<void>;",
    },
    ActionContract {
        name: "createSignedPreviewUrl",
        reply_shape: ReplyShape::Object(&["expiresAt", "path", "port", "token"]),
        ts_signature:
            "createSignedPreviewUrl: (c: Ctx, port: number, ttlSeconds: number) => Promise<SignedPreviewUrl>;",
    },
    ActionContract {
        name: "expireSignedPreviewUrl",
        reply_shape: ReplyShape::Unit,
        ts_signature: "expireSignedPreviewUrl: (c: Ctx, token: string) => Promise<void>;",
    },
    ActionContract {
        name: "listMounts",
        reply_shape: ReplyShape::Array,
        ts_signature: "listMounts: (c: Ctx) => Promise<MountInfo[]>;",
    },
    ActionContract {
        name: "listSoftware",
        reply_shape: ReplyShape::Array,
        ts_signature: "listSoftware: (c: Ctx) => Promise<SoftwareInfo[]>;",
    },
    // Observe-only actions (`actions::OBSERVE_ONLY`): dispatched without
    // booting a sleeping VM. See `dispatch_observe` in actions/mod.rs.
    ActionContract {
        name: "getRuntimeHealth",
        reply_shape: ReplyShape::Object(&[
            "agentExits",
            "booted",
            "sessions",
            "sidecar",
            "stderrTail",
            "warnings",
        ]),
        ts_signature: "getRuntimeHealth: (c: Ctx) => Promise<RuntimeHealth>;",
    },
    ActionContract {
        name: "listSessions",
        reply_shape: ReplyShape::Array,
        ts_signature: "listSessions: (c: Ctx) => Promise<LiveSessionInfo[]>;",
    },
    ActionContract {
        name: "cancelPrompt",
        reply_shape: ReplyShape::Unit,
        ts_signature: "cancelPrompt: (c: Ctx, sessionId: string) => Promise<void>;",
    },
    ActionContract {
        name: "listPendingPermissions",
        reply_shape: ReplyShape::Array,
        ts_signature:
            "listPendingPermissions: (c: Ctx) => Promise<PendingPermissionInfo[]>;",
    },
];

#[allow(dead_code)]
pub const EVENT_CONTRACTS: &[EventContract] = &[
    EventContract {
        name: "fsChanged",
        payload_shape: ReplyShape::Object(&["dirs", "overflow"]),
        ts_signature: "fsChanged: FsChangedPayload;",
    },
    EventContract {
        name: "sessionEvent",
        payload_shape: ReplyShape::Object(&["event", "sessionId"]),
        ts_signature: "sessionEvent: SessionEventPayload;",
    },
    EventContract {
        name: "permissionRequest",
        payload_shape: ReplyShape::Object(&["request", "sessionId"]),
        ts_signature: "permissionRequest: PermissionRequestPayload;",
    },
    EventContract {
        name: "permissionResolved",
        payload_shape: ReplyShape::Object(&["permissionId", "reply", "sessionId"]),
        ts_signature: "permissionResolved: PermissionResolvedPayload;",
    },
    EventContract {
        name: "agentCrashed",
        payload_shape: ReplyShape::Object(&["event", "sessionId"]),
        ts_signature: "agentCrashed: AgentCrashedPayload;",
    },
    EventContract {
        name: "vmBooted",
        payload_shape: ReplyShape::Object(&[]),
        ts_signature: "vmBooted: VmBootedPayload;",
    },
    EventContract {
        name: "vmShutdown",
        payload_shape: ReplyShape::Object(&["reason"]),
        ts_signature: "vmShutdown: VmShutdownPayload;",
    },
    EventContract {
        name: "processOutput",
        payload_shape: ReplyShape::Object(&["data", "pid", "stream"]),
        ts_signature: "processOutput: ProcessOutputPayload;",
    },
    EventContract {
        name: "processExit",
        payload_shape: ReplyShape::Object(&["exitCode", "pid"]),
        ts_signature: "processExit: ProcessExitPayload;",
    },
    EventContract {
        name: "shellData",
        payload_shape: ReplyShape::Object(&["data", "shellId"]),
        ts_signature: "shellData: ShellDataPayload;",
    },
    EventContract {
        name: "shellStderr",
        payload_shape: ReplyShape::Object(&["data", "shellId"]),
        ts_signature: "shellStderr: ShellDataPayload;",
    },
    EventContract {
        name: "shellExit",
        payload_shape: ReplyShape::Object(&["exitCode", "shellId"]),
        ts_signature: "shellExit: ShellExitPayload;",
    },
    EventContract {
        name: "cronEvent",
        payload_shape: ReplyShape::Object(&["event"]),
        ts_signature: "cronEvent: CronEventPayload;",
    },
];

pub fn render_actor_actions_ts() -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("// @generated by agentos-actor-plugin. Do not edit.\n");
    out.push_str("// This file is committed so package builds do not need to compile the native\n");
    out.push_str("// plugin just to regenerate TypeScript action types.\n");

    for import in TYPE_IMPORTS {
        render_import(&mut out, import);
    }

    out.push('\n');
    out.push_str("// The leading actor context arg; stripped from the client-facing method.\n");
    out.push_str("// biome-ignore lint/suspicious/noExplicitAny: ctx is server-side only and never reaches the typed client surface.\n");
    out.push_str("type Ctx = any;\n\n");

    for interface in DTO_INTERFACES {
        render_interface(&mut out, interface);
    }

    out.push_str("export type AgentOsActions = {\n");
    for action in ACTION_CONTRACTS {
        writeln!(&mut out, "\t{}", action.ts_signature)
            .expect("writing generated actor actions to string");
    }
    out.push_str("};\n");
    out
}

#[derive(Debug, Clone, Copy)]
struct TsImport {
    module: &'static str,
    names: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct TsInterface {
    name: &'static str,
    fields: &'static [TsField],
}

#[derive(Debug, Clone, Copy)]
struct TsField {
    name: &'static str,
    ty: &'static str,
    optional: bool,
}

const TYPE_IMPORTS: &[TsImport] = &[
    TsImport {
        module: "@rivet-dev/agentos-core",
        names: &[
            "ExecResult",
            "PermissionReply",
            "ProcessInfo",
            "ProcessTreeNode",
            "SpawnedProcessInfo",
            "VirtualStat",
        ],
    },
    TsImport {
        module: "../types.js",
        names: &[
            "PersistedSessionEvent",
            "PersistedSessionRecord",
            "PromptResult",
            "SerializableCronJobInfo",
            "SerializableCronJobOptions",
        ],
    },
];

const DTO_INTERFACES: &[TsInterface] = &[
    TsInterface {
        name: "DirEntry",
        fields: &[
            field("path", "string"),
            field("type", r#""file" | "directory" | "symlink""#),
            field("size", "number"),
        ],
    },
    TsInterface {
        name: "ReaddirEntry",
        fields: &[
            field("name", "string"),
            field("isDirectory", "boolean"),
            field("isSymbolicLink", "boolean"),
        ],
    },
    TsInterface {
        name: "SpawnedProcess",
        fields: &[field("pid", "number")],
    },
    TsInterface {
        name: "ExecActionOptions",
        fields: &[
            optional_field("env", "Record<string, string>"),
            optional_field("cwd", "string"),
        ],
    },
    TsInterface {
        name: "SpawnActionOptions",
        fields: &[
            optional_field("env", "Record<string, string>"),
            optional_field("cwd", "string"),
            optional_field("streamStdin", "boolean"),
        ],
    },
    TsInterface {
        name: "ScheduledCronJob",
        fields: &[field("id", "string")],
    },
    TsInterface {
        name: "VmFetchOptions",
        fields: &[
            optional_field("method", "string"),
            optional_field("headers", "Record<string, string>"),
            optional_field("body", "string | Uint8Array"),
        ],
    },
    TsInterface {
        name: "VmFetchResponse",
        fields: &[
            field("status", "number"),
            field("statusText", "string"),
            field("headers", "Record<string, string>"),
            field("body", "Uint8Array"),
        ],
    },
    TsInterface {
        name: "CreateSessionOptions",
        fields: &[
            optional_field("cwd", "string"),
            optional_field("env", "Record<string, string>"),
            optional_field("skipOsInstructions", "boolean"),
            optional_field("additionalInstructions", "string"),
        ],
    },
    TsInterface {
        name: "SignedPreviewUrl",
        fields: &[
            field("path", "string"),
            field("token", "string"),
            field("port", "number"),
            field("expiresAt", "number"),
        ],
    },
    TsInterface {
        name: "OpenShellActionOptions",
        fields: &[
            optional_field("command", "string"),
            optional_field("args", "string[]"),
            optional_field("env", "Record<string, string>"),
            optional_field("cwd", "string"),
            optional_field("cols", "number"),
            optional_field("rows", "number"),
        ],
    },
    TsInterface {
        name: "OpenShellResult",
        fields: &[field("shellId", "string")],
    },
    TsInterface {
        name: "WriteFileResult",
        fields: &[
            field("path", "string"),
            field("success", "boolean"),
            optional_field("error", "string"),
        ],
    },
    TsInterface {
        name: "ReadFileResult",
        fields: &[
            field("path", "string"),
            optional_field("content", "Uint8Array"),
            optional_field("error", "string"),
        ],
    },
    TsInterface {
        name: "MountInfo",
        fields: &[
            field("path", "string"),
            field(
                "kind",
                r#""host_dir" | "s3" | "google_drive" | "sandbox_agent""#,
            ),
            field("config", "unknown"),
            field("readOnly", "boolean"),
        ],
    },
    TsInterface {
        name: "SoftwareInfo",
        fields: &[
            field("package", "string"),
            field("kind", r#""wasm-commands" | "agent" | "tool""#),
            field("version", "string | null"),
            field("commands", "string[]"),
        ],
    },
    TsInterface {
        name: "LiveSessionInfo",
        fields: &[field("sessionId", "string"), field("agentType", "string")],
    },
    TsInterface {
        name: "PendingPermissionInfo",
        fields: &[
            field("sessionId", "string"),
            field("permissionId", "string"),
            optional_field("description", "string"),
            field("params", "Record<string, unknown>"),
            field("requestedAt", "number"),
        ],
    },
    TsInterface {
        name: "RuntimeLimitWarning",
        fields: &[
            field("ts", "number"),
            field("limit", "string"),
            field("category", "string"),
            field("observed", "number"),
            field("capacity", "number"),
            field("fillPercent", "number"),
        ],
    },
    TsInterface {
        name: "RuntimeAgentExit",
        fields: &[
            field("ts", "number"),
            field("sessionId", "string"),
            field("agentType", "string"),
            field("exitCode", "number | null"),
            field("restart", "string"),
            field("restartCount", "number"),
        ],
    },
    TsInterface {
        name: "RuntimeStderrLine",
        fields: &[field("ts", "number"), field("line", "string")],
    },
    TsInterface {
        name: "RuntimeSidecarInfo",
        fields: &[field("state", "string"), field("activeVmCount", "number")],
    },
    TsInterface {
        name: "RuntimeHealth",
        fields: &[
            field("booted", "boolean"),
            field("sessions", "number | null"),
            field("sidecar", "RuntimeSidecarInfo | null"),
            field("warnings", "RuntimeLimitWarning[]"),
            field("agentExits", "RuntimeAgentExit[]"),
            field("stderrTail", "RuntimeStderrLine[]"),
        ],
    },
];

const fn field(name: &'static str, ty: &'static str) -> TsField {
    TsField {
        name,
        ty,
        optional: false,
    }
}

const fn optional_field(name: &'static str, ty: &'static str) -> TsField {
    TsField {
        name,
        ty,
        optional: true,
    }
}

fn render_import(out: &mut String, import: &TsImport) {
    use std::fmt::Write as _;

    out.push_str("import type {\n");
    for name in import.names {
        writeln!(out, "\t{name},").expect("writing generated import to string");
    }
    writeln!(out, "}} from \"{}\";", import.module).expect("writing generated import to string");
}

fn render_interface(out: &mut String, interface: &TsInterface) {
    use std::fmt::Write as _;

    writeln!(out, "export interface {} {{", interface.name)
        .expect("writing generated interface to string");
    for field in interface.fields {
        let optional = if field.optional { "?" } else { "" };
        writeln!(out, "\t{}{}: {};", field.name, optional, field.ty)
            .expect("writing generated interface field to string");
    }
    out.push_str("}\n\n");
}
