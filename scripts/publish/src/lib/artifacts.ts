export type ArtifactEnv = Record<string, string | undefined>;

export interface ReleaseArtifactRef {
	ref: string;
	name: string;
}

export function releaseArtifactNamespace(env: ArtifactEnv = process.env): string {
	const namespace = env.RELEASE_ARTIFACT_NAMESPACE?.trim() || "agent-os";
	if (!/^[a-z0-9][a-z0-9-]*$/.test(namespace)) {
		throw new Error(`invalid RELEASE_ARTIFACT_NAMESPACE: ${namespace}`);
	}
	return namespace;
}

export function releaseArtifactPrefix(
	artifact: ReleaseArtifactRef,
	env: ArtifactEnv = process.env,
): string {
	for (const field of ["ref", "name"] as const) {
		if (!/^[A-Za-z0-9][A-Za-z0-9._+-]*$/.test(artifact[field])) {
			throw new Error(`Invalid release artifact ${field}`);
		}
	}
	return `${releaseArtifactNamespace(env)}/${artifact.ref}/${artifact.name}/`;
}

export function releaseUserAgent(env: ArtifactEnv = process.env): string {
	const namespace = releaseArtifactNamespace(env);
	const repositoryUrl =
		env.RELEASE_REPOSITORY_URL ?? `https://github.com/rivet-dev/${namespace}`;
	return `${namespace}-release-publisher (${repositoryUrl})`;
}
