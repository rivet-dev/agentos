import { vm } from "./client.js";

const preview = await vm.network.preview.create({
	port: 3000,
	ttlMs: 5 * 60 * 1_000,
});
console.log("Preview path:", preview.path);
console.log("Expires at:", new Date(Number(preview.expiresAtMs)));

await vm.network.preview.expire({ token: preview.token });
