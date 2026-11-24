import assert from "node:assert/strict";
import test from "node:test";
import {
	releaseArtifactNamespace,
	releaseArtifactPrefix,
	releaseUserAgent,
} from "./artifacts.js";

test("defaults release artifact paths to agent-os", () => {
	assert.equal(releaseArtifactNamespace({}), "agent-os");
	assert.equal(
		releaseArtifactPrefix({ ref: "abc1234", name: "sidecar" }, {}),
		"agent-os/abc1234/sidecar/",
	);
	assert.equal(
		releaseUserAgent({}),
		"agent-os-release-publisher (https://github.com/rivet-dev/agent-os)",
	);
});

test("rejects invalid release artifact namespaces", () => {
	assert.throws(
		() => releaseArtifactNamespace({ RELEASE_ARTIFACT_NAMESPACE: "../agent-os" }),
		/invalid RELEASE_ARTIFACT_NAMESPACE/,
	);
});

test("artifact coordinates each occupy exactly one key segment", () => {
	assert.equal(releaseArtifactPrefix({ ref: "1.2.3+build", name: "sidecar" }, {}), "agent-os/1.2.3+build/sidecar/");
	assert.throws(() => releaseArtifactPrefix({ ref: "", name: "sidecar" }, {}), /artifact ref/);
	assert.throws(() => releaseArtifactPrefix({ ref: "1.2.3", name: "sidecar/linux" }, {}), /artifact name/);
});
