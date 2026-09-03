import { vm } from "./client.js";

const response = await vm.network.fetch({
	request: {
		port: 3000,
		path: "/api/data",
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ key: "value" }),
	},
});

console.log("Status:", response.status, response.statusText);
console.log("Headers:", response.headers);
console.log("Body:", new TextDecoder().decode(response.body));
