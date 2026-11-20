import { afterEach, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

let vm: AgentOs | undefined;

afterEach(async () => {
	await vm?.dispose();
	vm = undefined;
});

const decode = (bytes: Uint8Array | string | undefined) =>
	typeof bytes === "string" ? bytes : Buffer.from(bytes ?? []).toString();

// Next.js and its bundled `compression` middleware use these OutgoingMessage
// APIs on every response.
const serverSource = `
const http = require("node:http");
http.createServer((req, res) => {
	res.setHeader("set-cookie", "a=1");
	const chained = res.appendHeader("set-cookie", ["b=2", "c=3"]);
	res.appendHeader("x-list", "1").appendHeader("x-list", "2");
	res.setHeader("x-chained", String(chained === res));
	res.setHeader("x-outgoing", String(res instanceof http.OutgoingMessage));
	const end = res.end;
	res.end = function (chunk) {
		this._implicitHeader();
		let lateAppend = "accepted";
		try { this.appendHeader("x-late", "1"); } catch (error) { lateAppend = error.code; }
		return end.call(this, chunk + ":" + lateAppend);
	};
	res.statusCode = 201;
	res.end("body");
}).listen(18884, "127.0.0.1", () => console.log("ready"));
`;

test("a guest ServerResponse appends headers and flushes them through _implicitHeader", async () => {
	vm = await AgentOs.create({ defaultSoftware: false });
	const ready = Promise.withResolvers<void>();
	let stderr = "";
	await vm.writeFile("/tmp/server.cjs", serverSource);
	await vm.process.spawn("node", ["/tmp/server.cjs"], {
		onStdout: (chunk) => {
			if (decode(chunk).includes("ready")) ready.resolve();
		},
		onStderr: (chunk) => {
			stderr += decode(chunk);
		},
	});
	await ready.promise;

	const response = await vm.fetch(18884, new Request("http://localhost:18884/"));

	expect(response.status).toBe(201);
	expect(response.headers.getSetCookie()).toEqual(["a=1", "b=2", "c=3"]);
	expect(response.headers.get("x-list")).toBe("1, 2");
	expect(response.headers.get("x-chained")).toBe("true");
	expect(response.headers.get("x-outgoing")).toBe("true");
	expect(response.headers.get("x-late")).toBeNull();
	expect(await response.text()).toBe("body:ERR_HTTP_HEADERS_SENT");
	expect(stderr).toBe("");
});
