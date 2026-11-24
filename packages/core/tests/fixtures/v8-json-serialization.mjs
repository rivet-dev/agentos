import { createRequire } from "node:module";
import * as esmV8 from "node:v8";
const cjsV8 = createRequire(import.meta.url)("node:v8");
let accessorCalls = 0;
let toJsonCalls = 0;
const accessor = Object.defineProperty({}, "value", {
	enumerable: true,
	get() { accessorCalls += 1; return 7; },
});
const withToJson = { value: 7, toJSON() { toJsonCalls += 1; return "changed"; } };
const shared = {};
const circular = {};
circular.self = circular;
const extraArray = [1];
extraArray.extra = 2;
const symbolProperty = { [Symbol("value")]: 1 };
const unsupported = {
	map: new Map([["a", 1]]), date: new Date(0), bigint: 1n,
	undefinedField: { value: undefined }, topUndefined: undefined,
	negativeZero: -0, infinity: Infinity, typedArray: new Uint8Array([1]),
	shared: { a: shared, b: shared }, circular, sparse: [, 1], extraArray,
	accessor, withToJson, symbolProperty,
};
function inspect(api) {
	const rejected = {};
	for (const [name, value] of Object.entries(unsupported)) {
		for (const [method, run] of [
			["serialize", () => api.serialize(value)],
			["Serializer", () => {
				const writer = new api.Serializer();
				writer.writeHeader(); writer.writeValue(value); return writer.releaseBuffer();
			}],
		]) {
			try { run(); rejected[method + ":" + name] = "RETURNED"; }
			catch (error) { rejected[method + ":" + name] = error.code ?? error.name; }
		}
	}
	const plain = { s: "x\n\"", n: 1.5, b: true, z: null, list: [1, { nested: "y" }] };
	const values = [null, "", false, 0, 1.5, plain, Object.assign(Object.create(null), { value: 3 })];
	const roundTrips = values.map(value => api.deserialize(api.serialize(value)));
	const writer = new api.Serializer(); writer.writeHeader(); writer.writeValue(plain);
	const reader = new api.Deserializer(writer.releaseBuffer()); reader.readHeader();
	let invalid;
	try { api.deserialize(Buffer.from([0xff, 0x0f])); invalid = "RETURNED"; }
	catch (error) { invalid = error.code ?? error.name; }
	// Encoding the child must not truncate the already-captured parent data.
	const parent = [];
	const child = new Proxy({ value: 1 }, {
		getPrototypeOf(target) { parent.length = 1; return Reflect.getPrototypeOf(target); },
	});
	parent.push(child, { kept: 2 });
	let arrayMutationError;
	try { api.deserialize(api.serialize(parent)); arrayMutationError = "RETURNED"; }
	catch (error) { arrayMutationError = error.code ?? error.name; }
	return { rejected, roundTrips, classRoundTrip: reader.readValue(), invalid, arrayMutationError };
}
console.log(JSON.stringify({
	cjs: inspect(cjsV8), esm: inspect(esmV8), accessorCalls, toJsonCalls,
	exportNames: Object.keys(cjsV8).sort(),
	esmExportNames: Object.keys(esmV8).filter(name => name !== "default").sort(),
	sameDefault: esmV8.default === cjsV8,
	sameBindings: ["cachedDataVersionTag", "DefaultDeserializer", "DefaultSerializer", "Deserializer", "GCProfiler", "Serializer", "deserialize", "getCppHeapStatistics", "getHeapCodeStatistics", "getHeapSnapshot", "getHeapSpaceStatistics", "getHeapStatistics", "isStringOneByteRepresentation", "promiseHooks", "queryObjects", "serialize", "setFlagsFromString", "setHeapSnapshotNearHeapLimit", "startCpuProfile", "startupSnapshot", "stopCoverage", "takeCoverage", "writeHeapSnapshot"].every(name => cjsV8[name] === esmV8[name]),
}));
