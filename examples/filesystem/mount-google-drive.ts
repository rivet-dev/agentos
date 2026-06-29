import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
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

export const registry = setup({ use: { vm } });
registry.start();
