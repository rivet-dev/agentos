import assert from "node:assert/strict";
import { test } from "node:test";
import { agentOsQueryKey, agentOsSource } from "../src/inspector-tabs/lib/source.ts";

test("filesystem refresh key matches every directory query for the same actor", () => {
	const prefix = agentOsQueryKey("actor-a", "dir");
	const nested = agentOsSource.listDirQueryOptions("actor-a", "/workspace").queryKey;
	const other = agentOsSource.listDirQueryOptions("actor-b", "/workspace").queryKey;
	assert.deepEqual(nested.slice(0, prefix.length), prefix);
	assert.notDeepEqual(other.slice(0, prefix.length), prefix);
});
