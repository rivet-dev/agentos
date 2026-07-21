import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const runtime = await JavaScriptRuntime.create({
	files: { "/workspace/seed.json": JSON.stringify({ ok: true }) },
});

try {
	await runtime.vm.filesystem.writeFile("/workspace/note.txt", "written from the host\n");
	const result = await runtime.evaluate<{ ok: boolean; note: string }>(`
		(async () => {
			const { readFile } = await import("node:fs/promises");
			const seed = JSON.parse(await readFile("/workspace/seed.json", "utf8"));
			const note = (await readFile("/workspace/note.txt", "utf8")).trim();
			return { ok: seed.ok, note };
		})()
	`);
	console.log(result);

	const bytes = await runtime.vm.filesystem.readFile("/workspace/seed.json");
	console.log(new TextDecoder().decode(bytes));
} finally {
	await runtime.dispose();
}
