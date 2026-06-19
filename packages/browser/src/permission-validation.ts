/**
 * Validate permission callback source strings before revival via new Function().
 *
 * Permission callbacks are serialized with fn.toString() on the host and revived
 * in the Web Worker. Because revival uses new Function(), the source must be
 * validated to prevent code injection.
 */

/**
 * Dangerous patterns that should never appear in a permission callback.
 * These could be used to escape the sandbox or access host resources.
 */
const BLOCKED_PATTERNS: RegExp[] = [
	// Code execution / eval
	/\beval\s*\(/,
	/\bFunction\s*\(/,
	/\bnew\s+Function\b/,

	// Module loading
	/\bimport\s*\(/,
	/\bimportScripts\s*\(/,
	/\brequire\s*\(/,

	// Global object access
	/\bglobalThis\b/,
	/\bself\b/,
	/\bwindow\b/,

	// Process/system access
	/\bprocess\s*\.\s*(?:exit|kill|binding|_linkedBinding|env)\b/,

	// Network / IO escape
	/\bXMLHttpRequest\b/,
	/\bWebSocket\b/,
	/\bfetch\s*\(/,

	// Prototype pollution / constructor abuse. `constructor` is never needed in
	// a permission callback; blocking the bare identifier catches both the
	// bracketed (`constructor[`) and the dot-chained
	// (`.constructor.constructor(...)`) Function-escape forms.
	/\bconstructor\b/,
	/\b__proto__\b/,
	/Object\s*\.\s*(?:defineProperty|setPrototypeOf|assign)\b/,

	// Dynamic property access on dangerous objects
	/\bpostMessage\b/,

	// `this` is never needed in a permission callback and is a common pivot to
	// reconstruct dangerous globals (e.g. `this['fet'+'ch']`). Block it.
	/\bthis\b/,

	// Computed/bracket member access to a dangerous identifier. This catches
	// forms that dodge the dotted patterns above, e.g. `process['exit']`,
	// `process['env']`, `req['constructor']`, `['constructor']['constructor']`,
	// `obj["prototype"]`, `x['__proto__']`. Legitimate permission callbacks
	// only read plain `req.*` properties and never bracket-index these names.
	/\[\s*(['"`])(?:exit|kill|binding|_linkedBinding|env|constructor|prototype|__proto__|eval|fetch|importScripts|require|globalThis|self|window|postMessage|process)\1/,

	// String-literal concatenation. Used to reconstruct a blocked identifier at
	// runtime so it never appears as a literal token (e.g. `'fet' + 'ch'`).
	// Permission callbacks have no legitimate need to concatenate string
	// literals, so reject any `'...' + '...'`.
	/(['"`])[^'"`]*\1\s*\+\s*(['"`])/,
];

/**
 * Validate that a permission callback source string is safe to revive.
 *
 * Returns true if the source appears to be a safe function expression.
 * Returns false if the source contains blocked patterns that could indicate
 * code injection.
 */
export function validatePermissionSource(source: string): boolean {
	if (!source || typeof source !== "string") return false;

	const trimmed = source.trim();

	// Must look like a function expression (arrow function or function keyword)
	const startsLikeFunction =
		trimmed.startsWith("function") ||
		trimmed.startsWith("(") ||
		// Single-param arrow functions: x => ...
		/^[a-zA-Z_$][a-zA-Z0-9_$]*\s*=>/.test(trimmed);

	if (!startsLikeFunction) return false;

	// Check for blocked patterns
	for (const pattern of BLOCKED_PATTERNS) {
		if (pattern.test(source)) return false;
	}

	return true;
}
