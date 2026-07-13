import type * as protocol from "./generated-protocol.js";
import { stringifyJsonUtf8 } from "./json.js";

export type LiveSidecarPlacement =
	| { kind: "shared"; pool?: string | null }
	| { kind: "explicit"; sidecar_id: string };

export type MountConfigJsonPrimitive = string | number | boolean | null;
export type MountConfigJsonValue =
	| MountConfigJsonPrimitive
	| MountConfigJsonObject
	| MountConfigJsonValue[];

export interface MountConfigJsonObject {
	[key: string]: MountConfigJsonValue | undefined;
}

export interface NativeMountPluginDescriptor<
	TConfig extends MountConfigJsonObject = MountConfigJsonObject,
> {
	id: string;
	config?: TConfig;
}

export interface S3MountCredentialsConfig {
	accessKeyId: string;
	secretAccessKey: string;
}

export interface ObjectS3MountConfig {
	bucket: string;
	prefix?: string;
	region?: string;
	credentials?: S3MountCredentialsConfig;
	endpoint?: string;
	uid?: number;
	gid?: number;
	fileMode?: number;
	dirMode?: number;
}

export interface ChunkedS3MountConfig {
	bucket: string;
	prefix?: string;
	region?: string;
	credentials?: S3MountCredentialsConfig;
	endpoint?: string;
	metadataBackend?: "sqlite" | "local" | "callback";
	metadataPath?: string;
	mountId?: string;
	chunkSize?: number;
	inlineThreshold?: number;
	uid?: number;
	gid?: number;
	fileMode?: number;
	dirMode?: number;
	metadataCacheEntries?: number;
}

export interface ChunkedLocalMountConfig {
	metadataPath: string;
	blockRoot: string;
	chunkSize?: number;
	inlineThreshold?: number;
	uid?: number;
	gid?: number;
	fileMode?: number;
	dirMode?: number;
	metadataCacheEntries?: number;
}

export function objectS3MountPlugin(
	config: ObjectS3MountConfig,
): NativeMountPluginDescriptor {
	return {
		id: "object_s3",
		config: config as unknown as MountConfigJsonObject,
	};
}

export function chunkedS3MountPlugin(
	config: ChunkedS3MountConfig,
): NativeMountPluginDescriptor {
	return {
		id: "chunked_s3",
		config: config as unknown as MountConfigJsonObject,
	};
}

export function chunkedLocalMountPlugin(
	config: ChunkedLocalMountConfig,
): NativeMountPluginDescriptor {
	return {
		id: "chunked_local",
		config: config as unknown as MountConfigJsonObject,
	};
}

export interface LiveMountDescriptor {
	guest_path: string;
	read_only?: boolean;
	plugin: NativeMountPluginDescriptor;
}

export interface LivePackageDescriptor {
	path: string;
}

export function toGeneratedSidecarPlacement(
	placement: LiveSidecarPlacement,
): protocol.SidecarPlacement {
	switch (placement.kind) {
		case "shared":
			return {
				tag: "SidecarPlacementShared",
				val: { pool: placement.pool ?? null },
			};
		case "explicit":
			return {
				tag: "SidecarPlacementExplicit",
				val: { sidecarId: placement.sidecar_id },
			};
	}
}

export function toGeneratedMountDescriptor(
	descriptor: LiveMountDescriptor,
): protocol.MountDescriptor {
	return {
		guestPath: descriptor.guest_path,
		readOnly: descriptor.read_only ?? null,
		plugin: {
			id: descriptor.plugin.id,
			config:
				descriptor.plugin.config === undefined
					? null
					: stringifyJsonUtf8(descriptor.plugin.config, "mount plugin config"),
		},
	};
}

export function toGeneratedPackageDescriptor(
	descriptor: LivePackageDescriptor,
): protocol.PackageDescriptor {
	return {
		path: descriptor.path,
	};
}
