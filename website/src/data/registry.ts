import type { RegistryIconName } from "./registry-icons";

export interface RegistryEntryBase {
	slug: string;
	title: string;
	description: string;
	types: ("file-system" | "tool" | "agent" | "sandbox-extension" | "software")[];
	featured?: boolean;
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

export type RegistryEntry = RegistryEntryAvailable | RegistryEntryComingSoon;

export const registry: RegistryEntry[] = [
	// Agents
	{
		slug: "pi",
		title: "PI",
		status: "available",
		package: "@agent-os/pi",
		description:
			"Run the PI coding agent with lightweight, fast execution.",
		types: ["agent"],
		featured: true,
		image: "/images/registry/pi.svg",
	},
	{
		slug: "claude-code",
		title: "Claude Code",
		status: "coming-soon",
		description:
			"Run Claude Code as an agentOS agent with full tool access, file editing, and shell execution.",
		types: ["agent"],
		image: "/images/registry/claude-code.svg",
	},
	{
		slug: "codex",
		title: "Codex",
		status: "coming-soon",
		description:
			"Run OpenAI's Codex coding agent inside agentOS with programmatic API access.",
		types: ["agent"],
		image: "/images/registry/codex.svg",
	},
	{
		slug: "amp",
		title: "Amp",
		status: "coming-soon",
		description:
			"Run Sourcegraph's Amp coding agent inside agentOS.",
		types: ["agent"],
		image: "/images/registry/amp.svg",
	},
	{
		slug: "opencode",
		title: "OpenCode",
		status: "coming-soon",
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
		package: "@agent-os/common",
		description:
			"Meta-package: coreutils + sed + grep + gawk + findutils + diffutils + tar + gzip.",
		types: ["software"],
	},
	{
		slug: "build-essential",
		title: "Build Essential",
		status: "available",
		package: "@agent-os/build-essential",
		description:
			"Meta-package: common + make + git + curl.",
		types: ["software"],
	},
	{
		slug: "coreutils",
		title: "Coreutils",
		status: "available",
		package: "@agent-os/coreutils",
		description:
			"sh, cat, ls, cp, mv, rm, sort, and 80+ essential POSIX commands.",
		types: ["software"],
	},
	{
		slug: "sed",
		title: "sed",
		status: "available",
		package: "@agent-os/sed",
		description: "GNU stream editor for text transformation.",
		types: ["software"],
	},
	{
		slug: "grep",
		title: "grep",
		status: "available",
		package: "@agent-os/grep",
		description: "GNU grep pattern matching (grep, egrep, fgrep).",
		types: ["software"],
	},
	{
		slug: "gawk",
		title: "gawk",
		status: "available",
		package: "@agent-os/gawk",
		description: "GNU awk text processing and data extraction.",
		types: ["software"],
	},
	{
		slug: "findutils",
		title: "findutils",
		status: "available",
		package: "@agent-os/findutils",
		description: "GNU find and xargs for file searching and batch execution.",
		types: ["software"],
	},
	{
		slug: "diffutils",
		title: "diffutils",
		status: "available",
		package: "@agent-os/diffutils",
		description: "GNU diff for comparing files.",
		types: ["software"],
	},
	{
		slug: "tar",
		title: "tar",
		status: "available",
		package: "@agent-os/tar",
		description: "GNU tar archiver.",
		types: ["software"],
	},
	{
		slug: "gzip",
		title: "gzip",
		status: "available",
		package: "@agent-os/gzip",
		description: "GNU gzip compression (gzip, gunzip, zcat).",
		types: ["software"],
	},
	{
		slug: "zip",
		title: "zip",
		status: "available",
		package: "@agent-os/zip",
		description: "Create zip archives.",
		types: ["software"],
	},
	{
		slug: "unzip",
		title: "unzip",
		status: "available",
		package: "@agent-os/unzip",
		description: "Extract zip archives.",
		types: ["software"],
	},
	{
		slug: "jq",
		title: "jq",
		status: "available",
		package: "@agent-os/jq",
		description: "Lightweight JSON processor.",
		types: ["software"],
	},
	{
		slug: "yq",
		title: "yq",
		status: "available",
		package: "@agent-os/yq",
		description: "YAML/JSON processor.",
		types: ["software"],
	},
	{
		slug: "ripgrep",
		title: "ripgrep",
		status: "available",
		package: "@agent-os/ripgrep",
		description: "Fast recursive search (rg).",
		types: ["software"],
		featured: true,
	},
	{
		slug: "fd",
		title: "fd",
		status: "available",
		package: "@agent-os/fd",
		description: "Fast file finder.",
		types: ["software"],
	},
	{
		slug: "tree",
		title: "tree",
		status: "available",
		package: "@agent-os/tree",
		description: "Display directory structure as a tree.",
		types: ["software"],
	},
	{
		slug: "file",
		title: "file",
		status: "available",
		package: "@agent-os/file",
		description: "Detect file types.",
		types: ["software"],
	},
	{
		slug: "codex-wasm",
		title: "Codex CLI",
		status: "available",
		package: "@agent-os/codex",
		description: "OpenAI Codex CLI integration.",
		types: ["software"],
	},

	// File Systems
	{
		slug: "filesystem",
		title: "Filesystem",
		status: "available",
		package: "@agent-os/core",
		description:
			"Mount and manage virtual filesystems with support for S3, local, and overlay drivers.",
		types: ["file-system"],
		icon: "HardDrive",
	},
	{
		slug: "s3",
		title: "S3",
		status: "available",
		package: "@agent-os/s3",
		description:
			"Mount S3-compatible object storage as a filesystem inside the VM.",
		types: ["file-system"],
		featured: true,
		image: "/images/registry/s3.svg",
	},
	{
		slug: "sqlite",
		title: "SQLite",
		status: "coming-soon",
		description:
			"Mount a SQLite-backed virtual filesystem for persistent, queryable storage.",
		types: ["file-system"],
		icon: "Database",
	},
	{
		slug: "postgres",
		title: "Postgres",
		status: "coming-soon",
		description:
			"Mount a Postgres-backed filesystem for shared, durable storage across agents.",
		types: ["file-system"],
		icon: "Database",
	},
	{
		slug: "google-drive",
		title: "Google Drive",
		status: "available",
		package: "@agent-os/google-drive",
		description:
			"Mount Google Drive as a filesystem for reading and writing documents and files.",
		types: ["file-system"],
		featured: true,
		image: "/images/registry/google-drive.svg",
	},

	// Tools
	{
		slug: "sandbox",
		title: "Sandbox",
		status: "available",
		package: "@agent-os/sandbox",
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
		types: ["sandbox-extension"],
		image: "/images/registry/sprites.svg",
	},
];
