/**
 * agentOS docs configuration — the only non-content surface consumed by
 * @rivet-dev/docs-theme. Everything visual (theme, header chrome, sidebar
 * icons, code blocks) lives in the package; this file maps agentOS's product
 * identity, navigation, and pages onto it.
 *
 * Sidebar structure + icons mirror the Rivet sitemap (rivet.dev) agentOS
 * section: a static "Agent" group containing a collapsible "Agents" sub-group.
 * Icons attach via each item's attrs.data-icon (shared theme catalog).
 *
 * @type {import('@rivet-dev/docs-theme').SiteConfig}
 */
export const siteConfig = {
	product: "agentOS",
	productLogo: "/images/agent-os/agentos-hero-logo.svg",
	productHome: "/",
	favicon: "/favicon.svg",
	repo: "rivet-dev/agent-os",
	editPath: "website/",

	topNav: [
		{ label: "Use Cases", href: "/use-cases" },
		{ label: "Pricing", href: "/pricing" },
		{ label: "Registry", href: "/registry" },
		{ label: "Docs", href: "/docs", match: "/docs" },
	],
	cta: { label: "Get Started", href: "/docs/quickstart" },
	social: { discord: "https://rivet.dev/discord" },

	analytics: { posthogKey: "phc_6kfTNEAVw7rn1LA51cO3D69FefbKupSWFaM7OUgEpEo" },

	landing: {
		title: "Documentation",
		subtitle:
			"agentOS runs coding agents inside isolated VMs with full filesystem, process, and network control — a lightweight VM in your own process with bindings, permissions, and orchestration built in.",
		cards: [
			{ title: "Quickstart", href: "/docs/quickstart", icon: "rocket", description: "Boot a VM and run your first coding agent." },
			{ title: "Crash Course", href: "/docs/crash-course", icon: "lightbulb", description: "Learn the core agentOS concepts." },
			{ title: "Agents", href: "/docs/agents/pi", icon: "bot", description: "Run Pi, Claude Code, Codex, and OpenCode." },
		],
	},

	sidebarGroupIcons: { Agents: "bot" },

	sidebar: [
		{ slug: "docs", label: "Introduction", attrs: { "data-icon": "info" } },
		{
			label: "General",
			items: [
				{ slug: "docs/quickstart", attrs: { "data-icon": "fastForward" } },
				{ slug: "docs/crash-course", label: "Crash Course", attrs: { "data-icon": "lightbulb" } },
				{ slug: "docs/versus-sandbox", label: "agentOS vs Sandbox", attrs: { "data-icon": "scaleBalanced" } },
			],
		},
		{
			label: "Agent",
			items: [
				{
					label: "Agents",
					items: [
						{ slug: "docs/agents/pi", label: "Pi", attrs: { "data-icon-src": "/images/registry/pi.svg" } },
						{ slug: "docs/agents/claude", label: "ClaudeCode", attrs: { "data-icon-src": "/images/registry/claude-code.svg" } },
						{ slug: "docs/agents/codex", label: "Codex", attrs: { "data-icon-src": "/images/registry/codex.svg" } },
						{ slug: "docs/agents/opencode", label: "OpenCode", attrs: { "data-icon-src": "/images/registry/opencode.svg" } },
						{ slug: "docs/agents/custom", label: "Custom Agents", attrs: { "data-icon": "wrench" } },
					],
				},
				{ slug: "docs/sessions", label: "Sessions & Transcripts", attrs: { "data-icon": "messages" } },
				{ slug: "docs/approvals", label: "Approvals", attrs: { "data-icon": "check" } },
				{ slug: "docs/llm-credentials", label: "LLM Credentials", attrs: { "data-icon": "key" } },
				{ slug: "docs/llm-gateway", label: "LLM Gateway", badge: { text: "Coming Soon", variant: "caution" }, attrs: { "data-icon": "cloud" } },
			],
		},
		{
			label: "Operating System",
			items: [
				{ slug: "docs/software", attrs: { "data-icon": "download" } },
				{ slug: "docs/filesystem", attrs: { "data-icon": "floppyDisk" } },
				{ slug: "docs/bindings", label: "Bindings", attrs: { "data-icon": "wrench" } },
				{ slug: "docs/processes", label: "Processes & Shell", attrs: { "data-icon": "terminal" } },
				{ slug: "docs/networking", label: "Networking & Previews", attrs: { "data-icon": "globe" } },
				{ slug: "docs/cron", label: "Cron Jobs", attrs: { "data-icon": "clock" } },
				{ slug: "docs/sandbox", label: "Sandbox Mounting", attrs: { "data-icon": "hardDrive" } },
				{ slug: "docs/js-runtime", label: "JavaScript Runtime", attrs: { "data-icon": "nodejs" } },
				{ slug: "docs/permissions", attrs: { "data-icon": "key" } },
				{ slug: "docs/resource-limits", label: "Resource Limits", attrs: { "data-icon": "gauge" } },
			],
		},
		{
			label: "Orchestration",
			items: [
				{ slug: "docs/authentication", attrs: { "data-icon": "key" } },
				{ slug: "docs/webhooks", attrs: { "data-icon": "link" } },
				{ slug: "docs/multiplayer", label: "Multiplayer & Realtime", attrs: { "data-icon": "towerBroadcast" } },
				{ slug: "docs/agent-to-agent", label: "Agent-to-Agent", attrs: { "data-icon": "arrowsLeftRight" } },
				{ slug: "docs/workflows", attrs: { "data-icon": "diagramNext" } },
			],
		},
		{
			label: "Reference",
			items: [
				{ label: "API Reference", link: "/api", attrs: { target: "_blank" } },
				{ slug: "docs/deployment", label: "Deploy" },
				{
					label: "Custom Software",
					items: [
						{ slug: "docs/custom-software/definition", label: "Definition" },
						{ slug: "docs/custom-software/building-wasm", label: "Building Binaries" },
						{ label: "Request Software", link: "https://github.com/rivet-dev/agent-os/issues/new/choose", attrs: { target: "_blank" } },
					],
				},
				{
					label: "Architecture",
					items: [
						{ slug: "docs/architecture", label: "Overview" },
						{ slug: "docs/security-model", label: "Security Model" },
						{ slug: "docs/limitations" },
						{
							label: "Advanced",
							items: [
								{ slug: "docs/architecture/agent-sessions", label: "Agent Sessions" },
								{ slug: "docs/architecture/agent-sdk-snapshots", label: "Agent SDK Snapshots" },
								{ slug: "docs/architecture/sessions-persistence", label: "Sessions & Persistence" },
								{ slug: "docs/architecture/processes", label: "Processes" },
								{ slug: "docs/architecture/filesystem", label: "Filesystem" },
								{ slug: "docs/architecture/networking", label: "Networking" },
								{ slug: "docs/architecture/posix-syscalls", label: "POSIX Syscalls" },
								{ slug: "docs/architecture/compiler-toolchain", label: "Compiler Toolchain" },
								{ slug: "docs/architecture/limits-and-observability", label: "Limits & Observability" },
								{ slug: "docs/system-prompt", label: "System Prompt" },
								{ slug: "docs/persistence", label: "Persistence & Sleep" },
							],
						},
					],
				},
				{
					label: "More",
					items: [
						{ slug: "docs/core", label: "Core SDK" },
						{ slug: "docs/debugging", label: "Debugging", attrs: { "data-icon": "bug" } },
						{ slug: "docs/benchmarks" },
						{ slug: "docs/cost-evaluation", label: "Cost Evaluation" },
					],
				},
			],
		},
	],
};

export default siteConfig;
