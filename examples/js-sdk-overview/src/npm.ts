import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const javascript = await JavaScriptRuntime.create();
try {
	await javascript.installNpmPackages({ frozen: true });
	const build = await javascript.executeNpmScript("build");
	if (build.outcome !== "succeeded") throw new Error(build.error.message);

	const formatter = await javascript.executeNpmPackage("prettier", {
		args: ["--check", "."],
	});
	if (formatter.outcome !== "succeeded") {
		throw new Error(formatter.error.message);
	}
} finally {
	await javascript.dispose();
}
