import { execute } from "secure-exec";

const source = `
	console.log("progress: 1/2");
	console.error("warning: slow path");
	console.log("progress: 2/2");
`;

// docs:start capture
// Output is not retained unless you ask for it: "stderr" for diagnostics, "all"
// for both streams.
const captured = await execute(source, { output: { capture: "all" } });
console.log(captured.stdout); // progress: 1/2\nprogress: 2/2\n
console.log(captured.stderr); // warning: slow path\n
// docs:end capture

// docs:start stream
// `onStdout` and `onStderr` stream chunks live and work with or without capture.
const decoder = new TextDecoder();
await execute(source, {
	onStdout: (chunk) => process.stdout.write(decoder.decode(chunk)),
	onStderr: (chunk) => process.stderr.write(decoder.decode(chunk)),
});
// docs:end stream
