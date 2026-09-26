import { AgentOs } from "@rivet-dev/agentos-core";

// External and host-backed mounts are available only to trusted Core callers.
export const vm = await AgentOs.create({
	mounts: [
		{
			path: "/mnt/drive",
			plugin: {
				id: "google_drive",
				config: {
					credentials: {
						clientEmail: process.env.GOOGLE_DRIVE_CLIENT_EMAIL!,
						privateKey: process.env.GOOGLE_DRIVE_PRIVATE_KEY!,
					},
					folderId: process.env.GOOGLE_DRIVE_FOLDER_ID!,
				},
			},
		},
	],
});
