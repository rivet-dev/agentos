import { vm } from "./client.js";

// Write a simple Node HTTP server and run it inside the VM. It binds a loopback
// port (3000) exactly like any normal Node process.
await vm.filesystem.writeFile({
	path: "/home/agentos/server.js",
	content: `const http = require("http");
http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "text/plain" });
  res.end("Hello from inside the VM");
}).listen(3000, () => console.log("listening on http://127.0.0.1:3000"));`,
});
const process = await vm.process.spawn({
	command: "node",
	args: ["/home/agentos/server.js"],
	options: { env: {} },
});
console.log("server pid:", process.pid);
