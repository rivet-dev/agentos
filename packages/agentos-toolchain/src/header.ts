/**
 * Executable header detection — the same binfmt rules the runtime uses.
 *
 * Runtime is decided by the file's leading bytes, never its name or extension:
 * a `#!` shebang, the `\0asm` WebAssembly magic, or a native object-file magic
 * (ELF / Mach-O / PE) which agentOS cannot execute (no native-arch handler).
 */

export type ExecutableKind =
	| "shebang"
	| "wasm"
	| "native-elf"
	| "native-macho"
	| "native-pe"
	| "unknown";

const WASM_MAGIC = Buffer.from([0x00, 0x61, 0x73, 0x6d]); // "\0asm"
const ELF_MAGIC = Buffer.from([0x7f, 0x45, 0x4c, 0x46]); // "\x7fELF"

// Mach-O thin (LE/BE, 32/64) + fat/universal magics.
const MACHO_MAGICS = [
	0xfeedface, 0xcefaedfe, 0xfeedfacf, 0xcffaedfe, 0xcafebabe, 0xbebafeca,
];

export function detectExecutableKind(head: Buffer): ExecutableKind {
	if (head.length >= 2 && head[0] === 0x23 && head[1] === 0x21) {
		return "shebang"; // "#!"
	}
	if (head.length >= 4 && head.subarray(0, 4).equals(WASM_MAGIC)) {
		return "wasm";
	}
	if (head.length >= 4 && head.subarray(0, 4).equals(ELF_MAGIC)) {
		return "native-elf";
	}
	if (head.length >= 4) {
		const be = head.readUInt32BE(0);
		// `cafebabe` collides with Java `.class`; disambiguate via the next field
		// (Mach-O fat has a small arch count; a class file's major version is >= 45).
		if (be === 0xcafebabe && head.length >= 8) {
			const next = head.readUInt32BE(4);
			if (next >= 45) return "unknown"; // Java class, not Mach-O
		}
		if (MACHO_MAGICS.includes(be)) return "native-macho";
	}
	// PE: "MZ" with a PE header pointer; "MZ" alone is enough to flag as native.
	if (head.length >= 2 && head[0] === 0x4d && head[1] === 0x5a) {
		return "native-pe"; // "MZ"
	}
	return "unknown";
}

export function isNativeKind(kind: ExecutableKind): boolean {
	return kind === "native-elf" || kind === "native-macho" || kind === "native-pe";
}

/** The interpreter named on a shebang line, taken literally (no PATH search). */
export function parseShebangInterpreter(head: Buffer): string | null {
	if (!(head.length >= 2 && head[0] === 0x23 && head[1] === 0x21)) return null;
	const nl = head.indexOf(0x0a);
	const line = head.subarray(2, nl < 0 ? head.length : nl).toString("utf8");
	const trimmed = line.replace(/^\s+/, "");
	const interp = trimmed.split(/\s+/, 1)[0] ?? "";
	return interp.length > 0 ? interp : null;
}
