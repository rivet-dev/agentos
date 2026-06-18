// Dependency-free code "highlighter" for the marketing hero code tabs.
//
// The original Rivet site rendered these snippets with Shiki at build time and
// styled the resulting `.shiki` / `.line` markup. Shiki is not a dependency of
// this site, so we emit the same `.shiki` > `.line` structure with the source
// HTML-escaped. The hero code block styles the container with a monospace font
// and muted foreground, so plain (unhighlighted) code still reads cleanly.
//
// Kept async + same signature as the prior `highlightCodeHtml` so callers
// (Astro pages) do not change.

function escapeHtml(code: string): string {
	return code
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;");
}

export async function highlightCodeHtml(
	code: string,
	_lang = "ts",
	_theme?: string,
): Promise<string> {
	const lines = code
		.split("\n")
		.map((line) => `<span class="line">${escapeHtml(line) || " "}</span>`)
		.join("\n");
	return `<pre class="shiki"><code>${lines}</code></pre>`;
}
