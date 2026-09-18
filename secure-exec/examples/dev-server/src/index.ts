// docs:start spawn
import { createVm } from "secure-exec";

// The server is user-authored code, so it runs in a VM. The guest may listen on
// its virtual network only when the network is allowed.
const vm = await createVm({ permissions: { network: "allow" } });

// `spawn` returns as soon as the process starts, instead of waiting for the
// code to finish. The server prints a line once it is listening, so the host
// knows when it can send requests.
const ready = Promise.withResolvers<void>();
const decoder = new TextDecoder();
const server = await vm.javascript.spawn(
	`
	import { createServer } from "node:http";

	createServer((request, response) => {
		response.setHeader("content-type", "application/json");
		response.end(JSON.stringify({ path: request.url, pid: process.pid }));
	}).listen(3000, () => console.log("listening"));
	`,
	{
		onStdout: (chunk) => {
			if (decoder.decode(chunk).includes("listening")) ready.resolve();
		},
	},
);
console.log("server pid:", server.pid);
await ready.promise;
// docs:end spawn

// docs:start request
// Send requests from the host to the port inside the VM. Nothing is exposed on
// the host's own network.
const response = await vm.network.httpRequest({ port: 3000, path: "/hello" });
console.log(new TextDecoder().decode(response.body)); // {"path":"/hello","pid":...}
// docs:end request

// docs:start stop
// Stop the server, then dispose the VM.
await vm.process.kill(server.pid);
const exit = await vm.process.wait(server.pid);
console.log("server exited:", exit);
await vm.dispose();
// docs:end stop
