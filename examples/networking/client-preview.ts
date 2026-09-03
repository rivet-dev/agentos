import { vm } from "./client.js";

// Start a web app in the VM
await vm.process.spawn({
	command: "node",
	args: ["/home/agentos/app.js"],
	options: { env: {} },
});

// Create a preview URL for port 3000, valid for 1 hour
const preview = await vm.network.preview.create({
	port: 3000,
	ttlMs: 60 * 60 * 1_000,
});
console.log("Preview path:", preview.path);
console.log("Token:", preview.token);
console.log("Expires at:", new Date(Number(preview.expiresAtMs)));

// Create a preview URL with a shorter expiration
const shortPreview = await vm.network.preview.create({
	port: 3000,
	ttlMs: 5 * 60 * 1_000,
});
console.log("Short-lived preview:", shortPreview.path);
