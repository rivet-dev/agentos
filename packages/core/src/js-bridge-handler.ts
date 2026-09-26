import { posix as posixPath } from "node:path";
import type { VirtualFileSystem } from "./runtime-compat.js";
import type {
	SidecarRequestFrame,
	SidecarResponsePayload,
} from "./sidecar/rpc-client.js";

interface JsBridgeContext {
	filesystem: VirtualFileSystem;
}

function bridgeErrorMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function toBridgeArgs(value: unknown): Record<string, unknown> {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		throw new Error("js_bridge args must be an object");
	}
	return value as Record<string, unknown>;
}

function bridgePath(mountId: string, value: unknown): string {
	if (!mountId.startsWith("/")) {
		throw new Error(`Unsupported js_bridge mount id: ${mountId}`);
	}
	if (typeof value !== "string") {
		throw new Error("js_bridge path argument must be a string");
	}
	return posixPath.normalize(posixPath.join(mountId, value));
}

function requireBridgeNumber(value: unknown, field: string): number {
	if (typeof value !== "number" || !Number.isFinite(value)) {
		throw new Error(`js_bridge args.${field} must be a number`);
	}
	return value;
}

function decodeBridgeBytes(value: unknown, field: string): Uint8Array {
	if (typeof value === "string") {
		return new Uint8Array(Buffer.from(value, "base64"));
	}
	if (
		Array.isArray(value) &&
		value.every(
			(entry) => Number.isInteger(entry) && entry >= 0 && entry <= 255,
		)
	) {
		return new Uint8Array(value);
	}
	throw new Error(`js_bridge args.${field} must be base64 bytes`);
}

export async function handleJsBridgeCall(
	request: Extract<SidecarRequestFrame["payload"], { type: "js_bridge_call" }>,
	context: JsBridgeContext,
): Promise<SidecarResponsePayload> {
	try {
		const args = toBridgeArgs(request.args);
		const fs = context.filesystem;
		const path = () => bridgePath(request.mount_id, args.path);
		let result: unknown;

		switch (request.operation) {
			case "readFile":
				result = Buffer.from(await fs.readFile(path())).toString("base64");
				break;
			case "readDir":
				result = await fs.readDir(path());
				break;
			case "readDirWithTypes":
				result = await fs.readDirWithTypes(path());
				break;
			case "writeFile":
				await fs.writeFile(path(), decodeBridgeBytes(args.content, "content"));
				break;
			case "createDir":
				await fs.createDir(path());
				break;
			case "mkdir":
				await fs.mkdir(path(), { recursive: args.recursive !== false });
				break;
			case "exists":
				result = await fs.exists(path());
				break;
			case "stat":
				result = await fs.stat(path());
				break;
			case "removeFile":
				await fs.removeFile(path());
				break;
			case "removeDir":
				await fs.removeDir(path());
				break;
			case "rename":
				await fs.rename(
					bridgePath(request.mount_id, args.oldPath),
					bridgePath(request.mount_id, args.newPath),
				);
				break;
			case "realpath":
				result = await fs.realpath(path());
				break;
			case "symlink": {
				if (typeof args.target !== "string") {
					throw new Error("js_bridge args.target must be a string");
				}
				await fs.symlink(
					args.target,
					bridgePath(request.mount_id, args.linkPath),
				);
				break;
			}
			case "readlink":
				result = await fs.readlink(path());
				break;
			case "lstat":
				result = await fs.lstat(path());
				break;
			case "link":
				await fs.link(
					bridgePath(request.mount_id, args.oldPath),
					bridgePath(request.mount_id, args.newPath),
				);
				break;
			case "chmod":
				await fs.chmod(path(), requireBridgeNumber(args.mode, "mode"));
				break;
			case "chown":
				await fs.chown(
					path(),
					requireBridgeNumber(args.uid, "uid"),
					requireBridgeNumber(args.gid, "gid"),
					{ followSymlinks: args.followSymlinks !== false },
				);
				break;
			case "utimes":
				await fs.utimes(
					path(),
					requireBridgeNumber(args.atimeMs, "atimeMs"),
					requireBridgeNumber(args.mtimeMs, "mtimeMs"),
				);
				break;
			case "truncate":
				await fs.truncate(path(), requireBridgeNumber(args.length, "length"));
				break;
			case "pread":
				result = Buffer.from(
					await fs.pread(
						path(),
						requireBridgeNumber(args.offset, "offset"),
						requireBridgeNumber(args.length, "length"),
					),
				).toString("base64");
				break;
			case "pwrite":
				await fs.pwrite(
					path(),
					requireBridgeNumber(args.offset, "offset"),
					decodeBridgeBytes(args.content, "content"),
				);
				break;
			default:
				throw new Error(
					`Unsupported js_bridge operation: ${request.operation}`,
				);
		}

		return {
			type: "js_bridge_result",
			call_id: request.call_id,
			...(result === undefined ? {} : { result }),
		};
	} catch (error) {
		return {
			type: "js_bridge_result",
			call_id: request.call_id,
			error: bridgeErrorMessage(error),
		};
	}
}
