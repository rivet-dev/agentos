#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const abiPath = resolve(repoRoot, "docs-internal/node-runtime-wasm-abi/agentos_posix_v1.json");
const outputPath = resolve(repoRoot, "docs-internal/node-runtime-wasm-abi/agentos-posix-contract.json");
const patchRoot = resolve(repoRoot, "registry/native/patches/wasi-libc");
const check = process.argv.slice(2).includes("--check");
if (process.argv.slice(2).some((argument) => argument !== "--check")) {
	throw new Error("usage: generate-node-runtime-wasm-posix-contract.mjs [--check]");
}

function sha256(path) {
	return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function filesRecursively(directory) {
	return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
		const path = resolve(directory, entry.name);
		return entry.isDirectory() ? filesRecursively(path) : [path];
	});
}

const patchFiles = filesRecursively(patchRoot).filter((path) => statSync(path).isFile());

const manualPages = {
	chmod: ["man2", "chmod.2"],
	clock_time_get: ["man2", "clock_gettime.2"],
	environ_get: ["man7", "environ.7"],
	environ_sizes_get: ["man7", "environ.7"],
	fchmod: ["man2", "chmod.2"],
	fd_close: ["man2", "close.2"],
	fd_dup_min: ["man2", "fcntl.2"],
	fd_fdstat_get: ["man2", "fcntl.2"],
	fd_fdstat_set_flags: ["man2", "fcntl.2"],
	fd_filestat_get: ["man2", "fstat.2"],
	fd_filestat_set_size: ["man2", "truncate.2"],
	fd_filestat_set_times: ["man2", "utimensat.2"],
	fd_mode: ["man2", "fstat.2"],
	fd_pipe: ["man2", "pipe.2"],
	fd_pread: ["man2", "pread.2"],
	fd_prestat_dir_name: ["man2", "openat.2"],
	fd_prestat_get: ["man2", "openat.2"],
	fd_pwrite: ["man2", "pwrite.2"],
	fd_read: ["man2", "read.2"],
	fd_readdir: ["man2", "getdents.2"],
	fd_seek: ["man2", "lseek.2"],
	fd_size: ["man2", "fstat.2"],
	fd_sync: ["man2", "fsync.2"],
	fd_write: ["man2", "write.2"],
	ftruncate: ["man2", "truncate.2"],
	getegid: ["man2", "getegid.2"],
	geteuid: ["man2", "geteuid.2"],
	getgid: ["man2", "getgid.2"],
	getpwuid: ["man3", "getpwuid.3"],
	getuid: ["man2", "getuid.2"],
	isatty: ["man3", "isatty.3"],
	net_accept: ["man2", "accept.2"],
	net_bind: ["man2", "bind.2"],
	net_connect: ["man2", "connect.2"],
	net_getaddrinfo: ["man3", "getaddrinfo.3"],
	net_getpeername: ["man2", "getpeername.2"],
	net_getsockname: ["man2", "getsockname.2"],
	net_listen: ["man2", "listen.2"],
	net_poll: ["man2", "poll.2"],
	net_recv: ["man2", "recv.2"],
	net_recvfrom: ["man2", "recvfrom.2"],
	net_send: ["man2", "send.2"],
	net_sendto: ["man2", "sendto.2"],
	net_setsockopt: ["man2", "setsockopt.2"],
	net_socket: ["man2", "socket.2"],
	path_create_directory: ["man2", "mkdirat.2"],
	path_filestat_get: ["man2", "stat.2"],
	path_link: ["man2", "link.2"],
	path_mode: ["man2", "stat.2"],
	path_open: ["man2", "openat.2"],
	path_readlink: ["man2", "readlink.2"],
	path_remove_directory: ["man2", "unlink.2"],
	path_rename: ["man2", "rename.2"],
	path_size: ["man2", "stat.2"],
	path_symlink: ["man2", "symlink.2"],
	path_unlink_file: ["man2", "unlink.2"],
	poll_oneoff: ["man2", "poll.2"],
	proc_exit: ["man2", "_exit.2"],
	proc_getpid: ["man2", "getpid.2"],
	proc_getppid: ["man2", "getppid.2"],
	proc_kill: ["man2", "kill.2"],
	proc_sigaction: ["man2", "sigaction.2"],
	proc_spawn: ["man3", "posix_spawn.3"],
	proc_waitpid: ["man2", "wait.2"],
	random_get: ["man2", "getrandom.2"],
	sched_yield: ["man2", "sched_yield.2"],
	sock_shutdown: ["man2", "shutdown.2"],
	"thread-spawn": ["man3", "pthread_create.3"],
};

function authority(name) {
	const [section, page] = manualPages[name] ?? [];
	if (!section) throw new Error(`missing authoritative Linux/POSIX reference for ${name}`);
	return `https://man7.org/linux/man-pages/${section}/${page}.html`;
}

function policy(name) {
	if (name === "path_open") return "fs.open-derived-rights-and-flags";
	if (name === "fd_fdstat_set_flags") return "fs.open-fd-capability";
	if (/^(fd_read|fd_pread|fd_readdir|fd_filestat_get|fd_mode|fd_size|path_filestat_get|path_mode|path_size|path_readlink|fd_prestat)/.test(name)) return "fs.read";
	if (/^(fd_write|fd_pwrite|fd_sync|fd_filestat_set|fchmod|ftruncate|chmod|path_create|path_link|path_open|path_remove|path_rename|path_symlink|path_unlink)/.test(name)) return "fs.write-or-metadata";
	if (name.startsWith("net_") || name === "sock_shutdown") return "network.socket-policy";
	if (name === "proc_spawn") return "child_process.spawn";
	if (/^proc_(kill|sigaction)$/.test(name)) return "process.signal";
	if (name === "proc_waitpid") return "process.wait-owned-child";
	if (name === "thread-spawn") return "runtime.thread-limit";
	if (/^(get|proc_get)/.test(name)) return "process.virtual-identity";
	return "vm-runtime-intrinsic";
}

function accounting(name) {
	if (/^(fd_(read|write|pread|pwrite|readdir)|path_readlink|net_(send|recv|sendto|recvfrom|getaddrinfo))$/.test(name)) return ["pending-syscalls", "transfer-bytes"];
	if (name === "poll_oneoff" || name === "net_poll") return ["pending-syscalls", "poll-events"];
	if (name === "thread-spawn") return ["runtime-threads", "aggregate-memory", "cpu"];
	if (name === "proc_spawn") return ["pending-syscalls", "processes", "transfer-bytes"];
	if (name === "fd_pipe") return ["pending-syscalls", "fds", "pipe-buffer-bytes"];
	if (name === "net_socket" || name === "net_accept") return ["pending-syscalls", "fds", "sockets"];
	return ["pending-syscalls"];
}

function bounds(name) {
	const result = ["limits.nodeRuntime.maxPendingSyscalls"];
	if (accounting(name).includes("transfer-bytes")) result.push("limits.nodeRuntime.maxTransferBytes");
	if (accounting(name).includes("poll-events")) result.push("limits.nodeRuntime.maxPollEvents");
	if (accounting(name).includes("runtime-threads")) result.push("limits.nodeRuntime.maxThreads");
	if (accounting(name).includes("fds")) result.push("limits.maxOpenFiles");
	if (accounting(name).includes("sockets")) result.push("limits.maxSockets");
	if (accounting(name).includes("processes")) result.push("limits.maxProcesses");
	return result;
}

function declarations(name) {
	const needle = `"${name}"`;
	const matches = patchFiles
		.filter((path) => readFileSync(path, "utf8").includes(needle))
		.map((path) => relative(repoRoot, path));
	return matches.length > 0
		? matches
		: ["https://github.com/WebAssembly/wasi-libc/blob/wasi-sdk-25/libc-bottom-half/headers/public/wasi/api.h"];
}

const abi = JSON.parse(readFileSync(abiPath, "utf8"));
const entries = abi.entries.map((entry) => ({
	module: "agentos_posix_v1",
	name: entry.name,
	signature: entry.signature,
	resultClassification: entry.resultClassification,
	libcEntryPoint: entry.name.replaceAll("-", "_"),
	sourceDeclarations: declarations(entry.name),
	authoritativeReference: authority(entry.name),
	authorizationRule: policy(entry.name),
	errnoMapping: entry.resultClassification === "wasi-errno" ? "wasi-preview1-errno" : entry.resultClassification,
	accountingClasses: accounting(entry.name),
	bounds: bounds(entry.name),
	testId: `posix:agentos_posix_v1:${entry.name}`,
	status: "required-shared-provider-surface",
}));

const output = `${JSON.stringify({
	schema: 1,
	namespace: "agentos_posix_v1",
	sourceAbi: "docs-internal/node-runtime-wasm-abi/agentos_posix_v1.json",
	sourceAbiSha256: sha256(abiPath),
	entryCount: entries.length,
	entries,
}, null, 2)}\n`;

if (check) {
	if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== output) {
		throw new Error(`generated POSIX contract is stale: ${outputPath}`);
	}
} else {
	writeFileSync(outputPath, output);
}
process.stdout.write(`Node runtime WASM POSIX contract ${check ? "verified" : "generated"}: ${entries.length} imports\n`);
