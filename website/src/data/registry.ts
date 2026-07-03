import type { RegistryIconName } from "./registry-icons";

export interface RegistryEntryBase {
	slug: string;
	title: string;
	description: string;
	types: ("file-system" | "tool" | "agent" | "sandbox-extension" | "software")[];
	featured?: boolean;
	// Marks an entry as in beta — renders a "Beta" pill on the registry card
	// and detail page.
	beta?: boolean;
	// Lucide icon name, resolved via REGISTRY_ICONS. Used when no `image` is
	// provided. Must be a serializable string so it survives the Astro island
	// prop boundary.
	icon?: RegistryIconName;
	image?: string;
}

export interface RegistryEntryAvailable extends RegistryEntryBase {
	status: "available";
	package: string;
}

export interface RegistryEntryComingSoon extends RegistryEntryBase {
	status: "coming-soon";
}

// An entry that isn't a separately installable package — e.g. a built-in agent
// adapter. The registry page links straight to its documentation instead of
// rendering an install command.
export interface RegistryEntryDocs extends RegistryEntryBase {
	status: "docs";
	// Link to the docs page documenting this entry.
	docsHref: string;
	// For built-in agent adapters: the npm software package that provides the
	// adapter and the session agent id. Used to render a usage example on the
	// registry page before linking out to the docs.
	package?: string;
	agentId?: string;
}

// A built-in capability configured inline (e.g. a filesystem mount plugin),
// not a separately installable package. Renders a configuration snippet and a
// link to the relevant docs instead of an `npm install` command.
export interface RegistryEntryConfig extends RegistryEntryBase {
	status: "config";
	// Short label for how it's enabled, e.g. `mounts: [{ plugin: { id: "s3" } }]`.
	configId: string;
	// TypeScript snippet showing how to configure it.
	configExample: string;
	// Link to the docs page that documents this configuration.
	docsHref: string;
}

export type RegistryEntry =
	| RegistryEntryAvailable
	| RegistryEntryComingSoon
	| RegistryEntryDocs
	| RegistryEntryConfig;

export const registry: RegistryEntry[] = [
	// Agents
	{
		slug: "pi",
		title: "PI",
		status: "available",
		package: "@agentos-software/pi",
		description:
			"Run the PI coding agent with lightweight, fast execution.",
		types: ["agent"],
		featured: true,
		image: "/images/registry/pi.svg",
	},
	{
		slug: "claude-code",
		title: "Claude Code",
		status: "docs",
		beta: true,
		docsHref: "/docs/agents/claude",
		package: "@agentos-software/claude-code",
		agentId: "claude",
		description:
			"Run Claude Code as an agentOS agent with full tool access, file editing, and shell execution.",
		types: ["agent"],
		image: "/images/registry/claude-code.svg",
	},
	{
		slug: "codex",
		title: "Codex",
		status: "docs",
		beta: true,
		docsHref: "/docs/agents/codex",
		package: "@agentos-software/codex",
		agentId: "codex",
		description:
			"Run OpenAI's Codex coding agent inside agentOS with programmatic API access.",
		types: ["agent"],
		image: "/images/registry/codex.svg",
	},
	{
		slug: "opencode",
		title: "OpenCode",
		status: "docs",
		docsHref: "/docs/agents/opencode",
		package: "@agentos-software/opencode",
		agentId: "opencode",
		description:
			"Run OpenCode, an open-source coding agent, inside agentOS.",
		types: ["agent"],
		image: "/images/registry/opencode.svg",
	},

	// Software
	{
		slug: "common",
		title: "Common",
		status: "available",
		package: "@agentos-software/common",
		description:
			"Meta-package: coreutils + sed + grep + gawk + findutils + diffutils + tar + gzip.",
		types: ["software"],
	},
	{
		slug: "build-essential",
		title: "Build Essential",
		status: "available",
		package: "@agentos-software/build-essential",
		description:
			"Meta-package: common + git + curl.",
		types: ["software"],
	},
	{
		slug: "coreutils",
		title: "Coreutils",
		status: "available",
		package: "@agentos-software/coreutils",
		description:
			"sh, cat, ls, cp, mv, rm, sort, and 80+ essential POSIX commands.",
		types: ["software"],
	},
	{
		slug: "sed",
		title: "sed",
		status: "available",
		package: "@agentos-software/sed",
		description: "GNU stream editor for text transformation.",
		types: ["software"],
	},
	{
		slug: "grep",
		title: "grep",
		status: "available",
		package: "@agentos-software/grep",
		description: "GNU grep pattern matching (grep, egrep, fgrep).",
		types: ["software"],
	},
	{
		slug: "gawk",
		title: "gawk",
		status: "available",
		package: "@agentos-software/gawk",
		description: "GNU awk text processing and data extraction.",
		types: ["software"],
	},
	{
		slug: "findutils",
		title: "findutils",
		status: "available",
		package: "@agentos-software/findutils",
		description: "GNU find and xargs for file searching and batch execution.",
		types: ["software"],
	},
	{
		slug: "diffutils",
		title: "diffutils",
		status: "available",
		package: "@agentos-software/diffutils",
		description: "GNU diff for comparing files.",
		types: ["software"],
	},
	{
		slug: "tar",
		title: "tar",
		status: "available",
		package: "@agentos-software/tar",
		description: "GNU tar archiver.",
		types: ["software"],
	},
	{
		slug: "gzip",
		title: "gzip",
		status: "available",
		package: "@agentos-software/gzip",
		description: "GNU gzip compression (gzip, gunzip, zcat).",
		types: ["software"],
	},
	{
		slug: "zip",
		title: "zip",
		status: "available",
		package: "@agentos-software/zip",
		description: "Create zip archives.",
		types: ["software"],
	},
	{
		slug: "unzip",
		title: "unzip",
		status: "available",
		package: "@agentos-software/unzip",
		description: "Extract zip archives.",
		types: ["software"],
	},
	{
		slug: "jq",
		title: "jq",
		status: "available",
		package: "@agentos-software/jq",
		description: "Lightweight JSON processor.",
		types: ["software"],
	},
	{
		slug: "yq",
		title: "yq",
		status: "available",
		package: "@agentos-software/yq",
		description: "YAML/JSON processor.",
		types: ["software"],
	},
	{
		slug: "ripgrep",
		title: "ripgrep",
		status: "available",
		package: "@agentos-software/ripgrep",
		description: "Fast recursive search (rg).",
		types: ["software"],
		featured: true,
	},
	{
		slug: "fd",
		title: "fd",
		status: "available",
		package: "@agentos-software/fd",
		description: "Fast file finder.",
		types: ["software"],
	},
	{
		slug: "tree",
		title: "tree",
		status: "available",
		package: "@agentos-software/tree",
		description: "Display directory structure as a tree.",
		types: ["software"],
	},
	{
		slug: "file",
		title: "file",
		status: "available",
		package: "@agentos-software/file",
		description: "Detect file types.",
		types: ["software"],
	},
	{
		slug: "codex-wasm",
		title: "Codex CLI",
		status: "available",
		package: "@agentos-software/codex",
		description: "OpenAI Codex CLI integration.",
		types: ["software"],
	},

	// File Systems
	{
		slug: "host-dir",
		title: "Host Directory",
		status: "config",
		configId: 'plugin: { id: "host_dir" }',
		docsHref: "/docs/filesystem",
		description:
			"Project a real host directory into the VM, Docker-style. The guest sees only the mounted subtree, never the wider host filesystem.",
		types: ["file-system"],
		icon: "HardDrive",
		configExample: `import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    {
      path: "/home/agentos/repo",
      plugin: { id: "host_dir", config: { hostPath: "/path/to/repo" } },
      readOnly: true,
    },
  ],
});

export const registry = setup({ use: { vm } });`,
	},
	{
		slug: "s3",
		title: "S3",
		status: "config",
		configId: 'plugin: { id: "s3" }',
		docsHref: "/docs/filesystem",
		description:
			"Mount an S3-compatible bucket as a filesystem. File contents are chunked into S3 objects, keeping large files, partial reads/writes, and snapshots efficient.",
		types: ["file-system"],
		featured: true,
		image: "/images/registry/s3.svg",
		configExample: `import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    {
      path: "/home/agentos/data",
      plugin: {
        id: "s3",
        config: { bucket: "my-bucket", prefix: "agent-data/", region: "us-east-1" },
      },
    },
  ],
});

export const registry = setup({ use: { vm } });`,
	},
	{
		slug: "google-drive",
		title: "Google Drive",
		status: "config",
		configId: 'plugin: { id: "google_drive" }',
		docsHref: "/docs/filesystem",
		description:
			"Mount a Google Drive folder as a filesystem for reading and writing documents and files.",
		types: ["file-system"],
		featured: true,
		image: "/images/registry/google-drive.svg",
		configExample: `import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    {
      path: "/home/agentos/drive",
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

export const registry = setup({ use: { vm } });`,
	},
	{
		slug: "memory",
		title: "In-Memory",
		status: "config",
		configId: 'plugin: { id: "memory" }',
		docsHref: "/docs/filesystem",
		description:
			"Mount an ephemeral in-memory directory. Fast scratch space that is discarded when the VM is destroyed.",
		types: ["file-system"],
		icon: "HardDrive",
		configExample: `import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
  mounts: [
    { path: "/home/agentos/scratch", plugin: { id: "memory", config: {} } },
  ],
});

export const registry = setup({ use: { vm } });`,
	},
	// Tools
	{
		slug: "sandbox",
		title: "Sandbox",
		status: "available",
		package: "@rivet-dev/agentos-sandbox",
		description:
			"Mount a sandbox filesystem and expose process management tools. Works with any Sandbox Agent provider.",
		types: ["tool", "file-system"],
		icon: "Monitor",
	},
	{
		slug: "browserbase",
		title: "Browserbase",
		status: "coming-soon",
		description:
			"Cloud browser infrastructure for web scraping, testing, and automation tasks.",
		types: ["tool"],
		image: "/images/registry/browserbase.svg",
	},

	// Sandbox Mounting
	{
		slug: "local",
		title: "Local",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes directly on the local machine for development and testing.",
		beta: true,
		types: ["sandbox-extension"],
		icon: "Monitor",
	},
	{
		slug: "docker",
		title: "Docker",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes in Docker containers for isolated local execution.",
		beta: true,
		types: ["sandbox-extension"],
		image: "/images/registry/docker.svg",
	},
	{
		slug: "e2b",
		title: "E2B",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes on E2B's cloud infrastructure for secure, ephemeral environments.",
		beta: true,
		types: ["sandbox-extension"],
		featured: true,
		image: "/images/registry/e2b.svg",
	},
	{
		slug: "daytona",
		title: "Daytona",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes on Daytona's managed development environments.",
		beta: true,
		types: ["sandbox-extension"],
		image: "/images/registry/daytona.svg",
	},
	{
		slug: "modal",
		title: "Modal",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes on Modal's serverless cloud infrastructure.",
		beta: true,
		types: ["sandbox-extension"],
		featured: true,
		image: "/images/registry/modal.svg",
	},
	{
		slug: "vercel",
		title: "Vercel",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes on Vercel's edge and serverless platform.",
		beta: true,
		types: ["sandbox-extension"],
		image: "/images/registry/vercel.svg",
	},
	{
		slug: "computesdk",
		title: "ComputeSDK",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes using the ComputeSDK compute provider.",
		beta: true,
		types: ["sandbox-extension"],
		image: "/images/registry/computesdk.svg",
	},
	{
		slug: "sprites",
		title: "Sprites",
		status: "available",
		package: "sandbox-agent",
		description:
			"Run sandboxes on Sprites' cloud sandbox infrastructure.",
		beta: true,
		types: ["sandbox-extension"],
		image: "/images/registry/sprites.svg",
	},
];
