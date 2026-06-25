// Dependency-free syntax highlighter for the marketing hero code tabs.
//
// The original Rivet site rendered these snippets with Shiki at build time and
// styled the resulting `.shiki` / `.line` markup. Shiki is not resolvable from
// this package, so we tokenize the (TS/JS) source ourselves and emit the same
// `.shiki` > `.line` structure with inline-colored token spans. The hero code
// block styles the container (`[&_.shiki]:!bg-transparent`, muted base text), so
// only the token foregrounds matter here.
//
// Kept async + same signature as the prior `highlightCodeHtml` so callers
// (Astro pages) do not change.

function escapeHtml(code: string): string {
	return code
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;");
}

// Light-theme palette: purple keywords, orange strings, gray-italic comments,
// blue function calls. Tuned to read on the light (`bg-zinc-50`) code block.
const STYLES: Record<string, string> = {
	comment: "color:#8b949e;font-style:italic",
	keyword: "color:#8250df",
	string: "color:#b45309",
	number: "color:#0550ae",
	fn: "color:#0550ae",
};

const KEYWORDS = new Set([
	"import", "export", "from", "const", "let", "var", "await", "async",
	"function", "return", "new", "class", "extends", "implements", "interface",
	"type", "enum", "if", "else", "for", "while", "do", "switch", "case",
	"break", "continue", "default", "try", "catch", "finally", "throw",
	"typeof", "instanceof", "in", "of", "void", "yield", "this", "super",
	"static", "public", "private", "protected", "readonly", "as", "satisfies",
	"namespace", "declare", "true", "false", "null", "undefined",
]);

interface Token {
	type: keyof typeof STYLES | "text";
	value: string;
}

function tokenize(code: string): Token[] {
	const tokens: Token[] = [];
	const n = code.length;
	let i = 0;
	let text = "";

	const flushText = () => {
		if (text) {
			tokens.push({ type: "text", value: text });
			text = "";
		}
	};

	const isIdentStart = (c: string) => /[A-Za-z_$]/.test(c);
	const isIdent = (c: string) => /[\w$]/.test(c);

	while (i < n) {
		const c = code[i];
		const next = code[i + 1];

		// Line comment
		if (c === "/" && next === "/") {
			flushText();
			let j = i + 2;
			while (j < n && code[j] !== "\n") j++;
			tokens.push({ type: "comment", value: code.slice(i, j) });
			i = j;
			continue;
		}

		// Block comment
		if (c === "/" && next === "*") {
			flushText();
			let j = i + 2;
			while (j < n && !(code[j] === "*" && code[j + 1] === "/")) j++;
			j = Math.min(n, j + 2);
			tokens.push({ type: "comment", value: code.slice(i, j) });
			i = j;
			continue;
		}

		// Strings and template literals
		if (c === '"' || c === "'" || c === "`") {
			flushText();
			const quote = c;
			let j = i + 1;
			while (j < n) {
				if (code[j] === "\\") {
					j += 2;
					continue;
				}
				if (code[j] === quote) {
					j++;
					break;
				}
				j++;
			}
			tokens.push({ type: "string", value: code.slice(i, j) });
			i = j;
			continue;
		}

		// Numbers
		if (/[0-9]/.test(c) && !(text && isIdent(text[text.length - 1]))) {
			flushText();
			let j = i;
			while (j < n && /[0-9._a-fxA-FXn]/.test(code[j])) j++;
			tokens.push({ type: "number", value: code.slice(i, j) });
			i = j;
			continue;
		}

		// Identifiers / keywords / function calls
		if (isIdentStart(c)) {
			flushText();
			let j = i + 1;
			while (j < n && isIdent(code[j])) j++;
			const word = code.slice(i, j);
			if (KEYWORDS.has(word)) {
				tokens.push({ type: "keyword", value: word });
			} else {
				// Function call if the next non-space char is "("
				let k = j;
				while (k < n && (code[k] === " " || code[k] === "\t")) k++;
				tokens.push({ type: code[k] === "(" ? "fn" : "text", value: word });
			}
			i = j;
			continue;
		}

		text += c;
		i++;
	}

	flushText();
	return tokens;
}

function renderTokens(tokens: Token[]): string {
	// Build per-line markup, splitting token values that span newlines so each
	// `.line` stays self-contained (the container styles `.line` for wrapping).
	const lines: string[][] = [[]];

	for (const token of tokens) {
		const parts = token.value.split("\n");
		parts.forEach((part, idx) => {
			if (idx > 0) lines.push([]);
			if (part === "") return;
			const escaped = escapeHtml(part);
			const style = token.type === "text" ? undefined : STYLES[token.type];
			lines[lines.length - 1].push(
				style ? `<span style="${style}">${escaped}</span>` : escaped,
			);
		});
	}

	return lines
		.map((parts) => `<span class="line">${parts.join("") || " "}</span>`)
		.join("\n");
}

export async function highlightCodeHtml(
	code: string,
	_lang = "ts",
	_theme?: string,
): Promise<string> {
	const html = renderTokens(tokenize(code));
	return `<pre class="shiki"><code>${html}</code></pre>`;
}
