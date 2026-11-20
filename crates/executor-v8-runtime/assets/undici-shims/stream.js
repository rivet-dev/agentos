"use strict";

import streamDefault, * as streamNs from "agentos-stream-stdlib";
import { Buffer } from "node:buffer";

const baseStreamModule = streamNs.default ?? streamDefault ?? {};
const baseFinished = streamNs.finished ?? baseStreamModule.finished;
const baseReadable = streamNs.Readable ?? baseStreamModule.Readable;
const baseWritable = streamNs.Writable ?? baseStreamModule.Writable;

// readable-stream's Node adapter loads native internal/webstreams helpers.
// Use the runtime's Web Stream readers and writers at this owning boundary.
baseReadable.fromWeb = (stream, options = {}) => {
	if (!stream || typeof stream.getReader !== "function") throw new TypeError("Expected a ReadableStream");
	const reader = stream.getReader();
	let reading = false, released = false;
	const release = () => { if (!released) { released = true; reader.releaseLock(); } };
	return new baseReadable({
		...options,
		read() {
			if (reading || released) return;
			reading = true;
			void reader.read().then(({ value, done }) => {
				reading = false;
				if (this.destroyed) return;
				if (done) { release(); this.push(null); }
				else this.push(value);
			}, error => { reading = false; this.destroy(error); });
		},
		destroy(error, callback) {
			if (released) { callback(error); return; }
			void reader.cancel(error).then(() => { release(); callback(error); }, failure => { release(); callback(error ?? failure); });
		},
	});
};
baseWritable.fromWeb = (stream, options = {}) => {
	if (!stream || typeof stream.getWriter !== "function") throw new TypeError("Expected a WritableStream");
	const writer = stream.getWriter(); let released = false;
	const release = () => { if (!released) { released = true; writer.releaseLock(); } };
	return new baseWritable({
		...options,
		write(chunk, _encoding, callback) { void writer.write(chunk).then(() => callback(), callback); },
		final(callback) { void writer.close().then(() => { release(); callback(); }, callback); },
		destroy(error, callback) {
			if (released) { callback(error); return; }
			void writer.abort(error).then(() => { release(); callback(error); }, failure => { release(); callback(error ?? failure); });
		},
	});
};

if (
	typeof baseReadable?.prototype?.push === "function" &&
	!baseReadable.prototype.push.__agentOSUint8ArrayPatched
) {
	const originalPush = baseReadable.prototype.push;
	baseReadable.prototype.push = function pushNodeCompatibleChunk(chunk, encoding) {
		if (
			!this?._readableState?.objectMode &&
			chunk instanceof Uint8Array &&
			!Buffer.isBuffer(chunk)
		) {
			chunk = Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength);
		}
		return originalPush.call(this, chunk, encoding);
	};
	baseReadable.prototype.push.__agentOSUint8ArrayPatched = true;
}

const isWebReadableStream = (stream) =>
	Boolean(stream) &&
	typeof stream.getReader === "function" &&
	typeof stream.cancel === "function";

const isWebWritableStream = (stream) =>
	Boolean(stream) &&
	typeof stream.getWriter === "function" &&
	typeof stream.abort === "function";

const normalizeStreamError = (value) => {
	if (value instanceof Error) {
		return value;
	}
	if (value == null) {
		return new Error("stream errored");
	}
	return new Error(String(value));
};

// Web streams expose no way to observe close or error without locking the
// stream. Node's eos waits on an internal promise that does not keep the event
// loop alive, so watch web-streams-polyfill `_state` transitions. A polling
// timer here held the guest process open while a fetch body stayed unread.
const webStreamSettleListeners = new WeakMap();

const watchWebStreamState = (stream) => {
	const listeners = new Set();
	let state = stream._state;
	Object.defineProperty(stream, "_state", {
		configurable: true,
		enumerable: true,
		get: () => state,
		set: (next) => {
			state = next;
			if (next === "closed" || next === "errored") {
				for (const listener of listeners) listener();
			}
		},
	});
	webStreamSettleListeners.set(stream, listeners);
	return listeners;
};

const onWebStreamSettled = (stream, listener) => {
	const listeners =
		webStreamSettleListeners.get(stream) ?? watchWebStreamState(stream);
	listeners.add(listener);
	return () => listeners.delete(listener);
};

export const finished = (stream, options, callback) => {
	let normalizedOptions = options;
	let normalizedCallback = callback;
	if (typeof normalizedOptions === "function") {
		normalizedCallback = normalizedOptions;
		normalizedOptions = {};
	}
	if (
		!isWebReadableStream(stream) &&
		!isWebWritableStream(stream) &&
		typeof baseFinished === "function"
	) {
		return baseFinished(stream, normalizedOptions, normalizedCallback);
	}

	const done =
		typeof normalizedCallback === "function" ? normalizedCallback : () => {};
	const readableEnabled = normalizedOptions?.readable !== false;
	const writableEnabled = normalizedOptions?.writable !== false;
	let cancelled = false;
	let unsubscribe = null;

	const cleanup = () => {
		cancelled = true;
		unsubscribe?.();
		unsubscribe = null;
	};

	const complete = (error = undefined) => {
		if (cancelled) {
			return;
		}
		cleanup();
		queueMicrotask(() => done(error));
	};

	const check = () => {
		if (cancelled) {
			return true;
		}
		const state = stream?._state;
		if (state === "errored") {
			complete(normalizeStreamError(stream?._storedError));
			return true;
		}
		if (
			state === "closed" ||
			(isWebReadableStream(stream) && !readableEnabled) ||
			(isWebWritableStream(stream) && !writableEnabled)
		) {
			complete();
			return true;
		}
		return false;
	};

	if (!check()) {
		// The polyfill assigns `_storedError` after `_state`, so read it once the
		// transition has finished.
		unsubscribe = onWebStreamSettled(stream, () => queueMicrotask(check));
	}
	return cleanup;
};

export const isReadable = (stream) => {
	if (isWebReadableStream(stream)) {
		return stream._state === "readable";
	}
	return Boolean(stream) && stream.readable !== false && stream.destroyed !== true;
};

export const isErrored = (stream) => {
	if (isWebReadableStream(stream) || isWebWritableStream(stream)) {
		return stream?._state === "errored";
	}
	return stream?.errored != null;
};

export const isDisturbed = (stream) => {
	return Boolean(
		stream?.locked ||
			stream?.disturbed === true ||
			stream?._disturbed === true ||
			stream?.readableDidRead === true,
	);
};

export * from "agentos-stream-stdlib";

export default {
	...baseStreamModule,
	finished,
	isReadable,
	isErrored,
	isDisturbed,
};
