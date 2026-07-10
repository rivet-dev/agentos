#!/usr/bin/env node
import {
	existsSync,
	mkdirSync,
	readFileSync,
	readdirSync,
	writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputPath = join(root, "docs-internal/node-runtime-wasm-porting-delta.json");
const roots = {
	edgeNative: join(root, "crates/node-runtime-wasm/vendor/edgejs/src"),
	nodeNative: join(root, "crates/node-runtime-wasm/vendor/node/src"),
	edgeJavaScript: join(root, "crates/node-runtime-wasm/vendor/edgejs/lib"),
	nodeJavaScript: join(root, "crates/node-stdlib/vendor/lib"),
};

function slash(path) {
	return path.split(sep).join("/");
}

function sha256(bytes) {
	return createHash("sha256").update(bytes).digest("hex");
}

function collect(path, base, files) {
	if (!existsSync(path)) throw new Error(`required vendor path is missing: ${slash(relative(root, path))}`);
	for (const entry of readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
		const child = join(path, entry.name);
		if (entry.isDirectory()) collect(child, base, files);
		else if (entry.isFile()) {
			const bytes = readFileSync(child);
			files[slash(relative(base, child))] = { sha256: sha256(bytes), bytes: bytes.length };
		}
	}
}

function inventory(path) {
	const files = {};
	collect(path, path, files);
	return files;
}

function compare(edgeFiles, nodeFiles) {
	const rows = {};
	const counts = { exact: 0, modified: 0, edgeOnly: 0, nodeOnly: 0 };
	const paths = [...new Set([...Object.keys(edgeFiles), ...Object.keys(nodeFiles)])].sort((a, b) => a.localeCompare(b));
	for (const path of paths) {
		const edge = edgeFiles[path];
		const node = nodeFiles[path];
		let status;
		if (!edge) status = "node-only";
		else if (!node) status = "edge-only";
		else if (edge.sha256 === node.sha256) status = "exact";
		else status = "modified";
		counts[status.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())]++;
		rows[path] = { status, edge: edge ?? null, node: node ?? null };
	}
	return { counts, rows };
}

const edgeVersion = readFileSync(join(roots.edgeNative, "node_version.h"), "utf8");
const nodeVersion = readFileSync(join(roots.nodeNative, "node_version.h"), "utf8");
function version(source) {
	const value = (name) => Number(source.match(new RegExp(`#define ${name} (\\d+)`))?.[1]);
	return `${value("NODE_MAJOR_VERSION")}.${value("NODE_MINOR_VERSION")}.${value("NODE_PATCH_VERSION")}`;
}

const native = compare(inventory(roots.edgeNative), inventory(roots.nodeNative));
const javaScript = compare(inventory(roots.edgeJavaScript), inventory(roots.nodeJavaScript));
const report = {
	schema: 1,
	sourcePins: {
		edgejs: "b1feaa2c2b36f443ee5d527161dd93f3ac1544d6",
		edgejsNodeBase: version(edgeVersion),
		targetNode: "848430679556aed0bd073f2bc263331ad84fa119",
		targetNodeVersion: version(nodeVersion),
	},
	policy: {
		edgejsRole: "source-porting-reference-only",
		edgejsArchitectureAccepted: false,
		targetArchitecture: "host -> existing native V8 isolate -> node-runtime.wasm and user JavaScript",
		osCapabilitySurface: "generated Linux/POSIX imports from the AgentOS-owned sysroot only",
	},
	comparisons: { native, javaScript },
};

const rendered = `${JSON.stringify(report, null, 2)}\n`;
if (process.argv.includes("--check")) {
	if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== rendered) {
		throw new Error("node-runtime-wasm porting delta is stale; regenerate it without --check");
	}
} else {
	mkdirSync(dirname(outputPath), { recursive: true });
	writeFileSync(outputPath, rendered);
}

process.stdout.write(
	`porting-delta: ${process.argv.includes("--check") ? "verified" : "generated"}; ` +
		`native ${JSON.stringify(native.counts)}, JavaScript ${JSON.stringify(javaScript.counts)}\n`,
);
