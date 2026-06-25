import { describe, expect, test } from "vitest";
import { createInMemoryFileSystem } from "../src/index.js";

const decoder = new TextDecoder();

describe("InMemoryFileSystem", () => {
	test("copies caller-provided write buffers and returned read buffers", async () => {
		const fs = createInMemoryFileSystem();
		const input = new Uint8Array([97, 98, 99]);

		await fs.writeFile("/data.txt", input);
		input[0] = 120;

		const firstRead = await fs.readFile("/data.txt");
		expect(decoder.decode(firstRead)).toBe("abc");

		firstRead[1] = 121;
		expect(decoder.decode(await fs.readFile("/data.txt"))).toBe("abc");
	});

	test("removeFile on a symlink removes the link without deleting the target", async () => {
		const fs = createInMemoryFileSystem();

		await fs.writeFile("/target.txt", "target content");
		await fs.symlink("/target.txt", "/link.txt");

		await fs.removeFile("/link.txt");

		expect(await fs.exists("/link.txt")).toBe(false);
		expect(decoder.decode(await fs.readFile("/target.txt"))).toBe(
			"target content",
		);
	});
});
