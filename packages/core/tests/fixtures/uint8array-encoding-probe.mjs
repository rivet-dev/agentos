// Runs in a guest VM. Expected values come from the tc39/proposal-arraybuffer-base64
// spec text. Native methods in Node 25 do not pass every case: for example, they
// fail "setFromBase64 ignores garbage after a full target", as they do in test262.
import vm from "node:vm";

const RealUint8Array = Uint8Array;
const bytes = (...values) => new Uint8Array(values);
const detached = () => {
	const view = bytes(1, 2);
	view.buffer.transfer();
	return view;
};
const outOfBounds = () => {
	const buffer = new ArrayBuffer(4, { maxByteLength: 8 });
	const view = new Uint8Array(buffer, 2, 2);
	buffer.resize(1);
	return view;
};
const detachOnOptionRead = (run) => {
	const view = bytes(1, 2);
	return run(view, {
		get alphabet() {
			view.buffer.transfer();
			return "base64";
		},
	});
};
const withPatched = (target, key, descriptor, run) => {
	const original = Object.getOwnPropertyDescriptor(target, key);
	Object.defineProperty(target, key, { configurable: true, ...descriptor });
	try {
		return run();
	} finally {
		if (original) Object.defineProperty(target, key, original);
		else delete target[key];
	}
};
const setResult = (size, run) => {
	const target = new Uint8Array(size);
	const { read, written } = run(target);
	return [read, written, [...target]];
};
const setError = (size, run) => {
	const target = new Uint8Array(size);
	try {
		run(target);
		return "no error";
	} catch (error) {
		return [error.name, [...target]];
	}
};
class LyingLength extends Uint8Array {
	get length() {
		return 1;
	}
}
const ownLength = () => {
	const view = bytes(1, 2);
	Object.defineProperty(view, "length", { value: 0 });
	return view;
};
const large = new Uint8Array(100_000).map((_, index) => index % 251);
const sharedBytes = new Uint8Array(new SharedArrayBuffer(2));
sharedBytes.set([1, 255]);
const sameBytes = (left, right) =>
	left.length === right.length && left.every((value, index) => value === right[index]);
const stop = { lastChunkHandling: "stop-before-partial" };
const strict = { lastChunkHandling: "strict" };
const url = { alphabet: "base64url" };
const proto = Uint8Array.prototype;

// Runs every path through the methods, so a patched intrinsic that any path
// still uses shows up as a different result.
const outcome = (run) => {
	try {
		const value = run();
		return value instanceof RealUint8Array ? [...value] : value;
	} catch (error) {
		return error.name;
	}
};
const realBytes = (...values) => new RealUint8Array(values);
const builtinResults = () => [
	outcome(() => realBytes(1, 2).toHex()),
	outcome(() => realBytes(1, 2).toBase64()),
	outcome(() => realBytes(1).toBase64({ omitPadding: true })),
	outcome(() => RealUint8Array.fromHex("0aFf")),
	outcome(() => RealUint8Array.fromHex("zz")),
	outcome(() => RealUint8Array.fromHex("abc")),
	outcome(() => RealUint8Array.fromHex(1)),
	outcome(() => RealUint8Array.fromBase64("AQID")),
	outcome(() => RealUint8Array.fromBase64("AQI")),
	outcome(() => RealUint8Array.fromBase64(" A Q = = ")),
	outcome(() => RealUint8Array.fromBase64("%%%%")),
	outcome(() => RealUint8Array.fromBase64("AQ", strict)),
	outcome(() => RealUint8Array.fromBase64(1)),
	outcome(() => realBytes(0).toBase64({ alphabet: "hex" })),
	outcome(() => {
		const { read, written } = new RealUint8Array(2).setFromHex("01ff");
		return [read, written];
	}),
	outcome(() => {
		const { read, written } = new RealUint8Array(3).setFromBase64(" A Q I D ");
		return [read, written];
	}),
];
const expectedBuiltinResults = [
	"0102",
	"AQI=",
	"AQ",
	[10, 255],
	"SyntaxError",
	"SyntaxError",
	"TypeError",
	[1, 2, 3],
	[1, 2],
	[1],
	"SyntaxError",
	"SyntaxError",
	"TypeError",
	"TypeError",
	[4, 2],
	[8, 3],
];

const cases = [
	// toHex
	["toHex lowercase", () => bytes(1, 2, 255).toHex(), "0102ff"],
	["toHex empty", () => bytes().toHex(), ""],
	["toHex subarray", () => bytes(0, 1, 2, 3).subarray(1, 3).toHex(), "0102"],
	["toHex SharedArrayBuffer", () => sharedBytes.toHex(), "01ff"],
	["toHex Buffer", () => Buffer.from([171]).toHex(), "ab"],
	["toHex subclass length", () => new LyingLength([1, 2]).toHex(), "0102"],
	["toHex own length", () => ownLength().toHex(), "0102"],
	["toHex Int8Array", () => proto.toHex.call(new Int8Array(1)), "TypeError"],
	["toHex plain object", () => proto.toHex.call({}), "TypeError"],
	["toHex detached", () => detached().toHex(), "TypeError"],
	["toHex out of bounds", () => outOfBounds().toHex(), "TypeError"],

	// fromHex
	["fromHex mixed case", () => Uint8Array.fromHex("0aFf"), [10, 255]],
	["fromHex empty", () => Uint8Array.fromHex(""), []],
	["fromHex odd length", () => Uint8Array.fromHex("abc"), "SyntaxError"],
	["fromHex bad digit", () => Uint8Array.fromHex("zz"), "SyntaxError"],
	["fromHex partly valid pair", () => Uint8Array.fromHex("1g"), "SyntaxError"],
	["fromHex signed pair", () => Uint8Array.fromHex("+1"), "SyntaxError"],
	["fromHex whitespace", () => Uint8Array.fromHex("0 "), "SyntaxError"],
	["fromHex non-ascii digits", () => Uint8Array.fromHex("٠١"), "SyntaxError"],
	["fromHex number", () => Uint8Array.fromHex(12), "TypeError"],
	["fromHex String object", () => Uint8Array.fromHex(new String("00")), "TypeError"],
	["fromHex no argument", () => Uint8Array.fromHex(), "TypeError"],

	// toBase64
	["toBase64 default", () => bytes(251, 255).toBase64(), "+/8="],
	["toBase64 one byte", () => bytes(1).toBase64(), "AQ=="],
	["toBase64 three bytes", () => bytes(1, 2, 3).toBase64(), "AQID"],
	["toBase64 empty", () => bytes().toBase64(), ""],
	["toBase64 subarray", () => bytes(0, 1, 2, 3).subarray(1, 3).toBase64(), "AQI="],
	["toBase64 SharedArrayBuffer", () => sharedBytes.toBase64(), "Af8="],
	["toBase64 subclass length", () => new LyingLength([1, 2]).toBase64(), "AQI="],
	["toBase64 base64url", () => bytes(251, 255).toBase64(url), "-_8="],
	["toBase64 base64url one byte", () => bytes(255).toBase64(url), "_w=="],
	["toBase64 base64url two bytes", () => bytes(255, 255).toBase64(url), "__8="],
	["toBase64 reads options in order", () => {
		const reads = [];
		bytes(1).toBase64({
			get alphabet() {
				reads.push("alphabet");
			},
			get omitPadding() {
				reads.push("omitPadding");
			},
		});
		return reads;
	}, ["alphabet", "omitPadding"]],
	["toBase64 reads length after options", () => {
		const buffer = new ArrayBuffer(4, { maxByteLength: 4 });
		const view = new Uint8Array(buffer);
		view.set([1, 2, 3, 4]);
		return view.toBase64({
			get alphabet() {
				buffer.resize(1);
				return "base64";
			},
		});
	}, "AQ=="],
	["toBase64 omitPadding", () => bytes(251, 255).toBase64({ omitPadding: true }), "+/8"],
	["toBase64 omitPadding coerces", () => bytes(1).toBase64({ omitPadding: 1 }), "AQ"],
	["toBase64 keeps padding on falsy", () => bytes(1).toBase64({ omitPadding: 0 }), "AQ=="],
	["toBase64 function options", () => bytes(1).toBase64(() => {}), "AQ=="],
	["toBase64 unknown alphabet", () => bytes(1).toBase64({ alphabet: "hex" }), "TypeError"],
	["toBase64 null alphabet", () => bytes(1).toBase64({ alphabet: null }), "TypeError"],
	["toBase64 null options", () => bytes(1).toBase64(null), "TypeError"],
	["toBase64 number options", () => bytes(1).toBase64(1), "TypeError"],
	["toBase64 Int8Array", () => proto.toBase64.call(new Int8Array(1)), "TypeError"],
	["toBase64 detached", () => detached().toBase64(), "TypeError"],
	["toBase64 out of bounds", () => outOfBounds().toBase64(), "TypeError"],
	["toBase64 detached by option getter", () => detachOnOptionRead((view, options) => view.toBase64(options)), "TypeError"],

	// fromBase64: alphabets and malformed input
	["fromBase64 default", () => Uint8Array.fromBase64("+/8="), [251, 255]],
	["fromBase64 unpadded", () => Uint8Array.fromBase64("+/8"), [251, 255]],
	["fromBase64 empty", () => Uint8Array.fromBase64(""), []],
	["fromBase64 only whitespace", () => Uint8Array.fromBase64(" \n"), []],
	["fromBase64 base64url", () => Uint8Array.fromBase64("-_8=", url), [251, 255]],
	["fromBase64 url chars in base64", () => Uint8Array.fromBase64("-_8="), "SyntaxError"],
	["fromBase64 base64 chars in url", () => Uint8Array.fromBase64("+/8=", url), "SyntaxError"],
	["fromBase64 ascii whitespace", () => Uint8Array.fromBase64(" A\tQ\n=\f=\r"), [1]],
	["fromBase64 vertical tab", () => Uint8Array.fromBase64("A\vQ=="), "SyntaxError"],
	["fromBase64 no-break space", () => Uint8Array.fromBase64("A Q=="), "SyntaxError"],
	["fromBase64 non-ascii", () => Uint8Array.fromBase64("éQ=="), "SyntaxError"],
	["fromBase64 single char", () => Uint8Array.fromBase64("A"), "SyntaxError"],
	["fromBase64 single trailing char", () => Uint8Array.fromBase64("AAAAA"), "SyntaxError"],
	["fromBase64 only padding", () => Uint8Array.fromBase64("=="), "SyntaxError"],
	["fromBase64 partial padding", () => Uint8Array.fromBase64("AQ="), "SyntaxError"],
	["fromBase64 excess padding", () => Uint8Array.fromBase64("AQ==="), "SyntaxError"],
	["fromBase64 data after padding", () => Uint8Array.fromBase64("AQ==AQ=="), "SyntaxError"],
	["fromBase64 data inside padding", () => Uint8Array.fromBase64("AA=A"), "SyntaxError"],

	// fromBase64: lastChunkHandling
	["loose ignores extra bits", () => Uint8Array.fromBase64("AR=="), [1]],
	["loose ignores extra bits, no padding", () => Uint8Array.fromBase64("AAF"), [0, 1]],
	["strict padded", () => Uint8Array.fromBase64("AAE=", strict), [0, 1]],
	["strict requires padding", () => Uint8Array.fromBase64("AQ", strict), "SyntaxError"],
	["strict rejects extra bits", () => Uint8Array.fromBase64("AR==", strict), "SyntaxError"],
	["strict rejects extra bits in a two byte chunk", () => Uint8Array.fromBase64("AAF=", strict), "SyntaxError"],
	["stop-before-partial full chunks", () => Uint8Array.fromBase64("AQID", stop), [1, 2, 3]],
	["stop-before-partial drops one char", () => Uint8Array.fromBase64("AQIDA", stop), [1, 2, 3]],
	["stop-before-partial drops two chars", () => Uint8Array.fromBase64("AQIDAQ", stop), [1, 2, 3]],
	["stop-before-partial drops partial padding", () => Uint8Array.fromBase64("AQIDAQ=", stop), [1, 2, 3]],
	["stop-before-partial keeps padded chunk", () => Uint8Array.fromBase64("AQIDAQ==", stop), [1, 2, 3, 1]],
	["stop-before-partial ignores extra bits", () => Uint8Array.fromBase64("AR==", stop), [1]],
	["stop-before-partial bad char", () => Uint8Array.fromBase64("AQ%D", stop), "SyntaxError"],

	// fromBase64: arguments
	["fromBase64 number", () => Uint8Array.fromBase64(1), "TypeError"],
	["fromBase64 String object", () => Uint8Array.fromBase64(new String("AQ==")), "TypeError"],
	["fromBase64 null options", () => Uint8Array.fromBase64("AQ==", null), "TypeError"],
	["fromBase64 unknown alphabet", () => Uint8Array.fromBase64("AQ==", { alphabet: "base32" }), "TypeError"],
	["fromBase64 unknown lastChunkHandling", () => Uint8Array.fromBase64("AQ==", { lastChunkHandling: "lenient" }), "TypeError"],
	["fromBase64 reads options in order", () => {
		const reads = [];
		Uint8Array.fromBase64("", {
			get alphabet() {
				reads.push("alphabet");
			},
			get lastChunkHandling() {
				reads.push("lastChunkHandling");
			},
		});
		return reads;
	}, ["alphabet", "lastChunkHandling"]],
	["fromBase64 interior whitespace trims the result", () => Uint8Array.fromBase64("AQ\nID\nBA==").buffer.byteLength, 4],
	["fromBase64 padding before data", () => Uint8Array.fromBase64("AQ==AQIDBAUG"), "SyntaxError"],
	["fromBase64 checks string before options", () => Uint8Array.fromBase64(1, { get alphabet() { throw new RangeError("read"); } }), "TypeError"],
	["fromBase64 option getter errors propagate", () => Uint8Array.fromBase64("AQ==", { get alphabet() { throw new RangeError("read"); } }), "RangeError"],

	// setFromHex
	["setFromHex fills target", () => setResult(2, (target) => target.setFromHex("01ff")), [4, 2, [1, 255]]],
	["setFromHex short input", () => setResult(3, (target) => target.setFromHex("01")), [2, 1, [1, 0, 0]]],
	["setFromHex stops when full", () => setResult(1, (target) => target.setFromHex("0102")), [2, 1, [1]]],
	["setFromHex stops before bad digit when full", () => setResult(1, (target) => target.setFromHex("01zz")), [2, 1, [1]]],
	["setFromHex empty target", () => setResult(0, (target) => target.setFromHex("zz")), [0, 0, []]],
	["setFromHex keeps bytes before an error", () => setError(2, (target) => target.setFromHex("01zz")), ["SyntaxError", [1, 0]]],
	["setFromHex odd length writes nothing", () => setError(2, (target) => target.setFromHex("010")), ["SyntaxError", [0, 0]]],
	["setFromHex number", () => bytes(0).setFromHex(1), "TypeError"],
	["setFromHex Int8Array", () => proto.setFromHex.call(new Int8Array(1), "00"), "TypeError"],
	["setFromHex detached", () => detached().setFromHex("00"), "TypeError"],
	["setFromHex out of bounds", () => outOfBounds().setFromHex("00"), "TypeError"],
	["setFromHex checks bounds before odd length", () => outOfBounds().setFromHex("0"), "TypeError"],

	// setFromBase64
	["setFromBase64 fills target", () => setResult(3, (target) => target.setFromBase64("AQID")), [4, 3, [1, 2, 3]]],
	["setFromBase64 final chunk", () => setResult(4, (target) => target.setFromBase64("AQ==")), [4, 1, [1, 0, 0, 0]]],
	["setFromBase64 chunk does not fit", () => setResult(2, (target) => target.setFromBase64("AQID")), [0, 0, [0, 0]]],
	["setFromBase64 stops at one byte left", () => setResult(1, (target) => target.setFromBase64("AQI=")), [0, 0, [0]]],
	["setFromBase64 stops at one byte left after a chunk", () => setResult(4, (target) => target.setFromBase64("AQIDBAU=")), [4, 3, [1, 2, 3, 0]]],
	["setFromBase64 unpadded final chunk", () => setResult(4, (target) => target.setFromBase64("AQI")), [3, 2, [1, 2, 0, 0]]],
	["setFromBase64 ignores garbage after a full target", () => setResult(3, (target) => target.setFromBase64("AQID%")), [4, 3, [1, 2, 3]]],
	["setFromBase64 reads length after options", () => {
		const buffer = new ArrayBuffer(4, { maxByteLength: 4 });
		const view = new Uint8Array(buffer);
		const { read, written } = view.setFromBase64("AQID", {
			get alphabet() {
				buffer.resize(1);
				return "base64";
			},
		});
		return [read, written, [...view]];
	}, [0, 0, [0]]],
	["setFromBase64 final chunk fits", () => setResult(2, (target) => target.setFromBase64("AQI=")), [4, 2, [1, 2]]],
	["setFromBase64 stops when full", () => setResult(3, (target) => target.setFromBase64("AQIDBAUG")), [4, 3, [1, 2, 3]]],
	["setFromBase64 reads whitespace in chunk", () => setResult(3, (target) => target.setFromBase64(" A Q I D ")), [8, 3, [1, 2, 3]]],
	["setFromBase64 stop-before-partial", () => setResult(6, (target) => target.setFromBase64("AQIDAQ", stop)), [4, 3, [1, 2, 3, 0, 0, 0]]],
	["setFromBase64 base64url", () => setResult(2, (target) => target.setFromBase64("-_8=", url)), [4, 2, [251, 255]]],
	["setFromBase64 empty target", () => setResult(0, (target) => target.setFromBase64("%%%")), [0, 0, []]],
	["setFromBase64 keeps chunks before an error", () => setError(6, (target) => target.setFromBase64("AQID%%%%")), ["SyntaxError", [1, 2, 3, 0, 0, 0]]],
	["setFromBase64 number", () => bytes(0).setFromBase64(1), "TypeError"],
	["setFromBase64 null options", () => bytes(0).setFromBase64("", null), "TypeError"],
	["setFromBase64 Int8Array", () => proto.setFromBase64.call(new Int8Array(1), ""), "TypeError"],
	["setFromBase64 detached", () => detached().setFromBase64("AQ=="), "TypeError"],
	["setFromBase64 out of bounds", () => outOfBounds().setFromBase64("AQ=="), "TypeError"],
	["setFromBase64 detached by option getter", () => detachOnOptionRead((view, options) => view.setFromBase64("AQ==", options)), "TypeError"],

	// Guest patches must not change a built-in's result
	["patched species", () => withPatched(Uint8Array, Symbol.species, { value: Array }, () => Uint8Array.fromBase64("A Q==") instanceof Uint8Array), true],
	["patched slice", () => withPatched(proto.__proto__, "slice", { value: () => "patched" }, () => Uint8Array.fromBase64("A Q==")), [1]],
	["patched subarray", () => withPatched(proto.__proto__, "subarray", { value: () => "patched" }, () => bytes(1).toBase64()), "AQ=="],
	["patched byteLength", () => withPatched(proto.__proto__, "byteLength", { get: () => 0 }, () => Uint8Array.fromHex("0102")), [1, 2]],
	["patched byteLength, bad hex", () => withPatched(proto.__proto__, "byteLength", { get: () => 0 }, () => Uint8Array.fromHex("zzzz")), "SyntaxError"],
	...[
		["String.prototype.charCodeAt", String.prototype, "charCodeAt", { value: () => { throw new Error("patched"); } }],
		["global ArrayBuffer", globalThis, "ArrayBuffer", { value: class {} }],
		["global Number", globalThis, "Number", { value: { POSITIVE_INFINITY: 0 } }],
		["global SyntaxError", globalThis, "SyntaxError", { value: class { name = "patched"; } }],
		["global TypeError", globalThis, "TypeError", { value: class { name = "patched"; } }],
		["global Uint8Array", globalThis, "Uint8Array", { value: class {} }],
	].map(([name, target, key, descriptor]) => [`guest patch of ${name} changes nothing`, () => withPatched(target, key, descriptor, builtinResults), expectedBuiltinResults]),

	// Round trips across chunk boundaries, and result shape
	["hex round trip", () => sameBytes(Uint8Array.fromHex(large.toHex()), large), true],
	["base64 round trip", () => sameBytes(Uint8Array.fromBase64(large.toBase64()), large), true],
	["base64url round trip", () => sameBytes(Uint8Array.fromBase64(large.toBase64(url), url), large), true],
	["unpadded round trip", () => sameBytes(Uint8Array.fromBase64(large.subarray(1).toBase64({ omitPadding: true })), large.subarray(1)), true],
	["every size around a chunk round trips", () => {
		for (let size = 0; size < 8200; size++) {
			if (size > 40 && (size + 8) % 512 > 16) continue;
			const view = large.subarray(0, size);
			if (!sameBytes(Uint8Array.fromHex(view.toHex()), view)) return `hex ${size}`;
			if (!sameBytes(Uint8Array.fromBase64(view.toBase64()), view)) return `base64 ${size}`;
			if (!sameBytes(Uint8Array.fromBase64(view.toBase64({ omitPadding: true })), view)) return `unpadded ${size}`;
		}
		return "all sizes";
	}, "all sizes"],
	["fromHex result owns its buffer", () => Uint8Array.fromHex("00ff").buffer.byteLength, 2],
	["fromBase64 result owns its buffer", () => Uint8Array.fromBase64("AQ==").buffer.byteLength, 1],
	["fromBase64 returns Uint8Array from Buffer", () => Buffer.fromBase64("AQ==").constructor === Uint8Array, true],
	["for-in sees only indexes", () => { const keys = []; for (const key in bytes(1)) keys.push(key); return keys.join(); }, "0"],
	["methods have spec lengths", () => [proto.toHex.length, proto.toBase64.length, proto.setFromHex.length, proto.setFromBase64.length, Uint8Array.fromHex.length, Uint8Array.fromBase64.length].join(), "0,0,1,1,1,1"],
	["methods are not constructors", () => new Uint8Array.fromHex("00"), "TypeError"],
	["methods work in a new vm context", () => vm.runInNewContext('new Uint8Array([1, 255]).toHex() + Uint8Array.fromBase64("AQ==")[0]'), "01ff1"],
	["encoding near the stack limit is correct or a RangeError", () => {
		const view = new Uint8Array(4096).fill(255);
		const expected = "ff".repeat(4096);
		const outcomes = new Set();
		const descend = () => {
			try {
				descend();
			} catch {}
			try {
				outcomes.add(view.toHex() === expected ? "correct" : "wrong");
			} catch (error) {
				outcomes.add(error.name);
			}
		};
		descend();
		return [...outcomes].filter((outcome) => outcome !== "correct" && outcome !== "RangeError");
	}, []],

	// Encoding must stay inside the VM's default 128 MiB heap.
	["16 MiB toHex fits the heap", () => {
		const encoded = new Uint8Array(16 * 1024 * 1024).fill(171).toHex();
		return [encoded.length, encoded.at(0), encoded.at(-1)];
	}, [32 * 1024 * 1024, "a", "b"]],
	["16 MiB toBase64 fits the heap", () => {
		const encoded = new Uint8Array(16 * 1024 * 1024).fill(255).toBase64();
		return [encoded.length, encoded.at(0), encoded.at(-1)];
	}, [22369624, "/", "="]],
	// A second buffer for this result would exceed the VM's 128 MiB buffer budget.
	["64 MiB padded base64 needs one buffer", () => Uint8Array.fromBase64(`${"AAAA".repeat(22369621)}AA==`).length, 64 * 1024 * 1024],
];

const failures = [];
for (const [label, run, expected] of cases) {
	let actual;
	try {
		const value = run();
		actual = value instanceof Uint8Array ? [...value] : value;
	} catch (error) {
		actual = error.name;
	}
	if (JSON.stringify(actual) !== JSON.stringify(expected)) {
		failures.push({ label, expected, actual });
	}
}
console.log(JSON.stringify({ total: cases.length, failures }));
