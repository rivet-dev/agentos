"use strict";

const { WebSocket, WebSocketServer } = require("ws");

async function main() {
	const serverEvents = [];
	const clientEvents = [];
	const serverCloseEvents = [];

	// Start server on random port
	const wss = new WebSocketServer({ port: 0 });

	wss.on("connection", (ws) => {
		serverEvents.push("connection");
		serverCloseEvents.push(
			new Promise((resolve) => {
				ws.once("close", resolve);
			}),
		);

		ws.on("message", (data, isBinary) => {
			serverEvents.push(isBinary ? "binary-message" : "text-message");
			// Echo back
			ws.send(data, { binary: isBinary });
		});

		ws.on("close", () => {
			serverEvents.push("close");
		});
	});

	await new Promise((resolve) => wss.on("listening", resolve));
	const port = wss.address().port;

	try {
		const textEcho = await new Promise((resolve, reject) => {
			const ws = new WebSocket(`ws://127.0.0.1:${port}`);
			let echo;

			ws.on("open", () => {
				clientEvents.push("open");
				ws.send("hello-ws");
			});

			ws.on("message", (data) => {
				clientEvents.push("text-message");
				echo = data.toString();
				ws.close();
			});

			ws.on("close", () => {
				clientEvents.push("text-close");
				if (echo === undefined) {
					reject(new Error("text socket closed before echo"));
					return;
				}
				resolve(echo);
			});

			ws.on("error", reject);
		});

		await serverCloseEvents[0];

		const binaryEcho = await new Promise((resolve, reject) => {
			const ws = new WebSocket(`ws://127.0.0.1:${port}`);
			let echo;

			ws.on("open", () => {
				clientEvents.push("binary-open");
				ws.send(Buffer.from([0xde, 0xad, 0xbe, 0xef]));
			});

			ws.on("message", (data, isBinary) => {
				clientEvents.push("binary-message");
				echo = {
					isBinary,
					hex: Buffer.from(data).toString("hex"),
				};
				ws.close();
			});

			ws.on("close", () => {
				clientEvents.push("binary-close");
				if (echo === undefined) {
					reject(new Error("binary socket closed before echo"));
					return;
				}
				resolve(echo);
			});

			ws.on("error", reject);
		});

		await serverCloseEvents[1];

		const result = {
			textEcho,
			binaryEcho,
			serverEvents: serverEvents.sort(),
			clientEvents: clientEvents.sort(),
		};

		console.log(JSON.stringify(result));
	} finally {
		await new Promise((resolve) => wss.close(resolve));
	}
}

main().catch((err) => {
	console.error(err.message);
	process.exit(1);
});
