// Capture intrinsic slots before user code can shadow a genuine view's public
// metadata. These getters also accept genuine receivers from another realm.
const apply = Reflect.apply;
const ByteView = Uint8Array;
const isView = ArrayBuffer.isView;
const arrayBufferLength = Object.getOwnPropertyDescriptor(
	ArrayBuffer.prototype,
	"byteLength",
)!.get!;
const typedArray = Object.getPrototypeOf(Uint8Array.prototype);
const typedBuffer = Object.getOwnPropertyDescriptor(typedArray, "buffer")!.get!;
const typedOffset = Object.getOwnPropertyDescriptor(typedArray, "byteOffset")!
	.get!;
const typedLength = Object.getOwnPropertyDescriptor(typedArray, "byteLength")!
	.get!;
const dataBuffer = Object.getOwnPropertyDescriptor(
	DataView.prototype,
	"buffer",
)!.get!;
const dataOffset = Object.getOwnPropertyDescriptor(
	DataView.prototype,
	"byteOffset",
)!.get!;
const dataLength = Object.getOwnPropertyDescriptor(
	DataView.prototype,
	"byteLength",
)!.get!;

export function intrinsicArrayBufferBytes(value: unknown): Uint8Array | null {
	try {
		apply(arrayBufferLength, value, []);
	} catch {
		return null;
	}
	return new ByteView(value as ArrayBuffer);
}

export function intrinsicArrayBufferViewBytes(
	value: unknown,
): Uint8Array | null {
	if (!isView(value)) return null;
	let buffer: ArrayBufferLike;
	try {
		buffer = apply(typedBuffer, value, []);
	} catch {
		return new ByteView(
			apply(dataBuffer, value, []),
			apply(dataOffset, value, []),
			apply(dataLength, value, []),
		);
	}
	return new ByteView(
		buffer,
		apply(typedOffset, value, []),
		apply(typedLength, value, []),
	);
}
