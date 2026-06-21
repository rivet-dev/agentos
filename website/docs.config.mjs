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
		{ label: "Documentation", href: "/docs", match: "/docs" },
		{ label: "Changelog", href: "https://github.com/rivet-dev/agent-os/releases" },
	],
	cta: { label: "Get Started", href: "/docs/quickstart" },
	social: { discord: "https://rivet.dev/discord" },

	analytics: { posthogKey: "phc_6kfTNEAVw7rn1LA51cO3D69FefbKupSWFaM7OUgEpEo" },

	landing: {
		title: "Documentation",
		subtitle:
			"agentOS runs coding agents inside isolated VMs with full filesystem, process, and network control — a lightweight VM in your own process with host tools, permissions, and orchestration built in.",
		cards: [
			{ title: "Quickstart", href: "/docs/quickstart", icon: "rocket", description: "Boot a VM and run your first coding agent." },
			{ title: "Crash Course", href: "/docs/crash-course", icon: "lightbulb", description: "Learn the core agentOS concepts." },
			{ title: "Agents", href: "/docs/agents/pi", icon: "bot", description: "Run Pi, Claude Code, Codex, Amp, and OpenCode." },
			{ title: "Operating System", href: "/docs/software", icon: "cpu", description: "Software, filesystem, processes, and networking." },
			{ title: "Orchestration", href: "/docs/workflows", icon: "diagramNext", description: "Webhooks, workflows, queues, and agent-to-agent." },
			{ title: "Reference", href: "/docs/core", icon: "book", description: "Core API, configuration, events, and deployment." },
		],
	},

	sidebar: [
		{
			label: "General",
			items: [
				{ slug: "docs", label: "Introduction", attrs: { "data-icon": "info" } },
				{ slug: "docs/quickstart", attrs: { "data-icon": "rocket" } },
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
						{ slug: "docs/agents/pi", label: "Pi" },
						{ slug: "docs/agents/claude", label: "ClaudeCode", badge: { text: "Coming Soon", variant: "caution" } },
						{ slug: "docs/agents/codex", label: "Codex", badge: { text: "Coming Soon", variant: "caution" } },
						{ slug: "docs/agents/amp", label: "Amp", badge: { text: "Coming Soon", variant: "caution" } },
						{ slug: "docs/agents/opencode", label: "OpenCode", badge: { text: "Coming Soon", variant: "caution" } },
					],
				},
				{ slug: "docs/sessions", label: "Sessions & Transcripts", attrs: { "data-icon": "messages" } },
				{ slug: "docs/permissions", attrs: { "data-icon": "key" } },
				{ slug: "docs/tools", attrs: { "data-icon": "wrench" } },
				{ slug: "docs/llm-credentials", label: "LLM Credentials", attrs: { "data-icon": "key" } },
				{ slug: "docs/llm-gateway", label: "LLM Gateway", badge: { text: "Coming Soon", variant: "caution" }, attrs: { "data-icon": "cloud" } },
			],
		},
		{
			label: "Operating System",
			items: [
				{ slug: "docs/software", attrs: { "data-icon": "download" } },
				{ slug: "docs/filesystem", attrs: { "data-icon": "floppyDisk" } },
				{ slug: "docs/processes", label: "Processes & Shell", attrs: { "data-icon": "terminal" } },
				{ slug: "docs/networking", label: "Networking & Previews", attrs: { "data-icon": "globe" } },
				{ slug: "docs/cron", label: "Cron Jobs", attrs: { "data-icon": "clock" } },
				{ slug: "docs/sandbox", label: "Sandbox Mounting", attrs: { "data-icon": "hardDrive" } },
				{ slug: "docs/security", label: "Security & Auth", attrs: { "data-icon": "lock" } },
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
				{ slug: "docs/queues", attrs: { "data-icon": "mailbox" } },
				{ slug: "docs/sqlite", label: "SQLite", attrs: { "data-icon": "database" } },
			],
		},
		{
			label: "Reference",
			items: [
				{ slug: "docs/core", label: "agentOS Core", attrs: { "data-icon": "box" } },
				{ slug: "docs/configuration", attrs: { "data-icon": "blocks" } },
				{ slug: "docs/events", attrs: { "data-icon": "scroll" } },
				{ slug: "docs/deployment", attrs: { "data-icon": "cloud" } },
				{ slug: "docs/limitations", attrs: { "data-icon": "shield" } },
				{
					label: "Internals",
					items: [
						{ slug: "docs/security-model", label: "Security Model", attrs: { "data-icon": "lock" } },
						{ slug: "docs/persistence", label: "Persistence & Sleep", attrs: { "data-icon": "hardDrive" } },
						{ slug: "docs/system-prompt", label: "System Prompt", attrs: { "data-icon": "fileCode" } },
						{ slug: "docs/benchmarks", attrs: { "data-icon": "gauge" } },
					],
				},
			],
		},
	],
};

export default siteConfig;
