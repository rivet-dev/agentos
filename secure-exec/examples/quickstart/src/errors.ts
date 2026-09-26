import { createVm, evaluate, SidecarRejectedError } from "secure-exec";

// docs:start returned
// What the guest code does wrong is returned as a result, never thrown.
const failed = await evaluate(`null.length`, { output: { capture: "stderr" } });
console.log(failed.outcome); // failed
if (failed.outcome !== "succeeded") {
	console.log(failed.error.code); // execution_failed
	console.log(failed.stderr?.split("\n")[0]); // TypeError: Cannot read properties of null ...
}
// docs:end returned

// docs:start thrown
// What the host asks for wrong is thrown, as a typed error with a stable code.
const vm = await createVm();
const context = await vm.createContext();
const slow = context.evaluate(
	"new Promise((resolve) => setTimeout(resolve, 500))",
);
try {
	await context.evaluate("1 + 1"); // The context is still busy with `slow`.
} catch (error) {
	if (error instanceof SidecarRejectedError) {
		console.log(error.detail.code); // execution_busy
	}
}
await slow;
await vm.dispose();
// docs:end thrown
