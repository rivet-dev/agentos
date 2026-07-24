import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import { resolveRelativeNonFileUrl } from "../bridge-src/builtins/url-resolution.ts";

test("fallback URL resolution follows HTTP base URL semantics", () => {
	const base = {
		protocol: "http:",
		host: "example.test:8080",
		pathname: "/parent/entry",
		search: "?old=1",
	};
	const resolve = (input) => resolveRelativeNonFileUrl(input, base, path.posix);

	assert.equal(resolve("/hello?q=1"), "http://example.test:8080/hello?q=1");
	assert.equal(resolve("child"), "http://example.test:8080/parent/child");
	assert.equal(resolve("../child/"), "http://example.test:8080/child/");
	assert.equal(resolve("?next=1"), "http://example.test:8080/parent/entry?next=1");
	assert.equal(resolve("#result"), "http://example.test:8080/parent/entry?old=1#result");
	assert.equal(resolve(""), "http://example.test:8080/parent/entry?old=1");
	assert.equal(resolve("//cdn.test/file.js"), "http://cdn.test/file.js");
});
