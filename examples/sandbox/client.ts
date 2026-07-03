import { createSandboxVm, disposeSandboxVm } from "./server";

const handle = await createSandboxVm();

try {
	await handle.vm.writeFile(
		"/home/agentos/sandbox/hello.txt",
		"Hello from a fresh sandbox",
	);
	const content = await handle.vm.readFile("/home/agentos/sandbox/hello.txt");
	console.log(new TextDecoder().decode(content));
} finally {
	await disposeSandboxVm(handle);
}
