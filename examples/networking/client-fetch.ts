import { vm } from "./client.js";

// Fetch from the VM service started above.
const response = await vm.network.fetch({
	request: { port: 3000, path: "/", method: "GET", headers: {} },
});
console.log("Status:", response.status);
console.log("Body:", new TextDecoder().decode(response.body));
