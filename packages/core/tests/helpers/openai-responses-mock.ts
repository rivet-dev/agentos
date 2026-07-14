import {
	createServer,
	type IncomingMessage,
	type ServerResponse,
} from "node:http";

export type ResponsesRequestBody = Record<string, unknown>;

export type ResponsesFixture = {
	name: string;
	predicate: (body: ResponsesRequestBody) => boolean;
	response: Record<string, unknown>;
	delayMs?: number;
};

export type RunningResponsesMock = {
	url: string;
	port: number;
	requests: ResponsesRequestBody[];
	stop: () => Promise<void>;
};

async function readJsonBody(
	req: IncomingMessage,
): Promise<ResponsesRequestBody> {
	const chunks: Buffer[] = [];
	for await (const chunk of req) {
		chunks.push(Buffer.from(chunk));
	}
	const text = Buffer.concat(chunks).toString("utf8");
	return JSON.parse(text) as ResponsesRequestBody;
}

function writeJson(
	res: ServerResponse,
	statusCode: number,
	body: Record<string, unknown>,
): void {
	const payload = JSON.stringify(body);
	res.statusCode = statusCode;
	res.setHeader("content-type", "application/json");
	res.setHeader("content-length", Buffer.byteLength(payload));
	res.end(payload);
}

function writeSse(res: ServerResponse, response: Record<string, unknown>): void {
	res.statusCode = 200;
	res.setHeader("content-type", "text/event-stream");
	res.setHeader("cache-control", "no-cache");
	res.setHeader("connection", "keep-alive");

	const send = (event: Record<string, unknown>) => {
		res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
	};
	const responseId =
		typeof response.id === "string" ? response.id : "resp_mock";
	const output = Array.isArray(response.output)
		? (response.output as Array<Record<string, unknown>>)
		: [];

	send({ type: "response.created", response: { id: responseId } });
	for (const item of output) {
		if (item.type === "function_call" || item.type === "local_shell_call") {
			send({ type: "response.output_item.done", item });
			continue;
		}
		const content = Array.isArray(item.content)
			? (item.content as Array<Record<string, unknown>>)
			: [];
		for (const part of content) {
			if (part.type === "output_text" && typeof part.text === "string") {
				send({ type: "response.output_text.delta", delta: part.text });
			}
		}
		send({ type: "response.output_item.done", item });
	}
	send({
		type: "response.completed",
		response: {
			id: responseId,
			usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 },
		},
	});
	res.end();
}

export async function startResponsesMock(
	fixtures: ResponsesFixture[],
): Promise<RunningResponsesMock> {
	const requests: ResponsesRequestBody[] = [];
	const server = createServer(async (req, res) => {
		if (req.method !== "POST" || req.url !== "/v1/responses") {
			writeJson(res, 404, { error: "not_found" });
			return;
		}

		try {
			const body = await readJsonBody(req);
			requests.push(body);

			const fixture = fixtures.find((candidate) => candidate.predicate(body));
			if (!fixture) {
				writeJson(res, 500, {
					error: "no_matching_fixture",
					request: body,
				});
				return;
			}

			if (fixture.delayMs) {
				await new Promise((resolve) => setTimeout(resolve, fixture.delayMs));
			}
			writeSse(res, fixture.response);
		} catch (error) {
			writeJson(res, 500, {
				error: "invalid_request",
				message: error instanceof Error ? error.message : String(error),
			});
		}
	});

	await new Promise<void>((resolve) => {
		server.listen(0, "127.0.0.1", () => resolve());
	});
	server.unref();

	const address = server.address();
	if (!address || typeof address === "string") {
		throw new Error("mock server did not expose a TCP port");
	}

	return {
		url: `http://127.0.0.1:${address.port}`,
		port: address.port,
		requests,
		stop: async () => {
			await new Promise<void>((resolve, reject) => {
				server.close((error) => {
					if (error) reject(error);
					else resolve();
				});
			});
		},
	};
}
