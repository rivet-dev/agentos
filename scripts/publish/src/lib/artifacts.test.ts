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

test("supports secure-exec release artifact namespace", () => {
	const env = {
		RELEASE_ARTIFACT_NAMESPACE: "secure-exec",
		RELEASE_REPOSITORY_URL: "https://github.com/rivet-dev/secure-exec",
	};

	assert.equal(releaseArtifactNamespace(env), "secure-exec");
	assert.equal(
		releaseArtifactPrefix({ ref: "0.3.0", name: "sidecar" }, env),
		"secure-exec/0.3.0/sidecar/",
	);
	assert.equal(
		releaseUserAgent(env),
		"secure-exec-release-publisher (https://github.com/rivet-dev/secure-exec)",
	);
});

test("rejects invalid release artifact namespaces", () => {
	assert.throws(
		() => releaseArtifactNamespace({ RELEASE_ARTIFACT_NAMESPACE: "../agent-os" }),
		/invalid RELEASE_ARTIFACT_NAMESPACE/,
	);
});
