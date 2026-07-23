/**
 * agentOS docs configuration for @rivet-dev/docs-theme (the de-Starlighted,
 * rivet-1:1 framework). Maps agentOS identity/nav onto the theme's SiteConfig.
 *
 * `sitemap` is the docs navigation tree: SiteTab[] where each tab carries a
 * sidebar tree (pages + collapsible sections). Routes are /docs/* (file paths
 * under src/content/docs). Top-level sections are non-collapsible labels (rivet
 * style); only nested page-groups collapse. Page items carry FontAwesome
 * `IconDefinition`s for the sidebar icons.
 *
 * @type {import('@rivet-dev/docs-theme').SiteConfig}
 */
import {
	faCircleInfo,
	faForwardFast,
	faLightbulb,
	faScaleBalanced,
	faRobot,
	faToolbox,
	faWrench,
	faMessages,
	faCheck,
	faKey,
	faCloud,
	faDownload,
	faFloppyDisk,
	faTerminal,
	faGlobe,
	faClock,
	faHardDrive,
	faNodeJs,
	faGauge,
	faLink,
	faTowerBroadcast,
	faArrowsLeftRight,
	faDiagramNext,
	faWindowMaximize,
} from "@rivet-gg/icons";

export const siteConfig = {
	product: "agentOS",
	productLogo: "/images/agent-os/agentos-hero-logo.svg",
	productHome: "/",
	siteUrl: "https://agentos-sdk.dev",
	favicon: { svg: "/favicon.svg" },
	repo: "rivet-dev/agentos",
	editPath: "website/",

	// Keep in sync with the marketing nav (src/components/Navigation.tsx):
	// same links in the same order across docs + marketing.
	topNav: [
		{ label: "Documentation", href: "/docs", match: "/docs" },
		{ label: "Cookbooks", href: "/cookbooks", match: "/cookbooks" },
		{ label: "Use Cases", href: "/use-cases" },
		{ label: "Registry", href: "/registry" },
		{ label: "Deploy", href: "/docs/deployment", match: "/docs/deployment" },
	],
	tabs: [
		{ label: "Documentation", href: "/docs", match: "/docs" },
		{ label: "Cookbooks", href: "/cookbooks", match: "/cookbooks" },
	],
	social: { discord: "https://rivet.dev/discord" },
	analytics: { posthogKey: "phc_6kfTNEAVw7rn1LA51cO3D69FefbKupSWFaM7OUgEpEo" },

	// Hosted Typesense docs search (same cluster as rivet). The search-only key
	// is safe to ship client-side; indexing uses the admin key (see scripts).
	search: {
		typesense: {
			host: "3lsug6t152oxcjndp-1.a1.typesense.net",
			searchApiKey: "o4qaOyinaSrfIVcxHwSjk0tby0pE14ry",
			collectionName: "agentos-docs",
		},
	},

	sitemap: [
		{
			title: "Documentation",
			href: "/docs",
			sidebar: [
				{ title: "Introduction", href: "/docs", icon: faCircleInfo },
				{
					title: "General",
					pages: [
						{ title: "Quickstart", href: "/docs/quickstart", icon: faForwardFast },
						{ title: "Crash Course", href: "/docs/crash-course", icon: faLightbulb },
						{ title: "agentOS vs Sandbox", href: "/docs/versus-sandbox", icon: faScaleBalanced },
					],
				},
				{
					title: "Agent",
					pages: [
						{
							title: "Agents",
							collapsible: true,
							icon: faRobot,
							pages: [
								{ title: "Pi", href: "/docs/agents/pi", icon: { src: "/images/registry/pi.svg" } },
								{ title: "ClaudeCode", href: "/docs/agents/claude", badge: "Beta", icon: { src: "/images/registry/claude-code.svg" } },
								{ title: "Codex", href: "/docs/agents/codex", badge: "Beta", icon: { src: "/images/registry/codex.svg" } },
								{ title: "OpenCode", href: "/docs/agents/opencode", icon: { src: "/images/registry/opencode.svg" } },
								{ title: "Custom Agents", href: "/docs/agents/custom", icon: faWrench },
							],
						},
						{
							title: "Frameworks",
							collapsible: true,
							icon: faToolbox,
							pages: [
								{
									title: "Vercel Eve",
									href: "/docs/frameworks/vercel-eve",
									badge: "Beta",
									icon: { src: "/images/frameworks/eve-icon.svg" },
								},
								{
									title: "Flue",
									href: "/docs/frameworks/flue",
									badge: "Beta",
									icon: { src: "/images/frameworks/flue.svg" },
								},
							],
						},
						{ title: "Sessions & Transcripts", href: "/docs/sessions", icon: faMessages },
						{ title: "Approvals", href: "/docs/approvals", icon: faCheck },
						{ title: "LLM Credentials", href: "/docs/llm-credentials", icon: faKey },
						{ title: "LLM Gateway", href: "/docs/llm-gateway", badge: "Coming Soon", icon: faCloud },
					],
				},
				{
					title: "Operating System",
					pages: [
						{ title: "Software", href: "/docs/software", icon: faDownload },
						{ title: "Filesystem", href: "/docs/filesystem", icon: faFloppyDisk },
						{ title: "Bindings", href: "/docs/bindings", icon: faWrench },
						{ title: "Processes & Shell", href: "/docs/processes", icon: faTerminal },
						{ title: "Networking & Previews", href: "/docs/networking", icon: faGlobe },
						{ title: "Cron Jobs", href: "/docs/cron", icon: faClock },
						{ title: "Browser", href: "/docs/browser", badge: "Beta", icon: faWindowMaximize },
						{ title: "Sandbox Mounting", href: "/docs/sandbox", badge: "Beta", icon: faHardDrive },
						{ title: "JavaScript Runtime", href: "/docs/js-runtime", icon: faNodeJs },
						{ title: "Permissions", href: "/docs/permissions", icon: faKey },
						{ title: "Resource Limits", href: "/docs/resource-limits", icon: faGauge },
					],
				},
				{
					title: "Orchestration",
					pages: [
						{ title: "Authentication", href: "/docs/authentication", icon: faKey },
						{ title: "Webhooks", href: "/docs/webhooks", icon: faLink },
						{ title: "Multiplayer & Realtime", href: "/docs/multiplayer", icon: faTowerBroadcast },
						{ title: "Agent-to-Agent", href: "/docs/agent-to-agent", icon: faArrowsLeftRight },
						{ title: "Workflows", href: "/docs/workflows", icon: faDiagramNext },
					],
				},
				{
					title: "Reference",
					pages: [
						{ title: "API Reference", href: "/api", external: true, target: "_blank" },
						{ title: "Deploy", href: "/docs/deployment" },
						{
							title: "Custom Software",
							collapsible: true,
							pages: [
								{ title: "Definition", href: "/docs/custom-software/definition" },
								{ title: "Building Binaries", href: "/docs/custom-software/building-wasm" },
								{ title: "Request Software", href: "https://github.com/rivet-dev/agentos/issues/new/choose", external: true, target: "_blank" },
							],
						},
						{
							title: "Architecture",
							collapsible: true,
							pages: [
								{ title: "Overview", href: "/docs/architecture" },
								{ title: "Security Model", href: "/docs/security-model" },
								{ title: "Limitations", href: "/docs/limitations" },
								{
									title: "Advanced",
									collapsible: true,
									pages: [
										{ title: "Agent Sessions", href: "/docs/architecture/agent-sessions" },
										{ title: "Agent SDK Snapshots", href: "/docs/architecture/agent-sdk-snapshots" },
										{ title: "Sessions & Persistence", href: "/docs/architecture/sessions-persistence" },
										{ title: "Processes", href: "/docs/architecture/processes" },
										{ title: "Filesystem", href: "/docs/architecture/filesystem" },
										{ title: "Networking", href: "/docs/architecture/networking" },
										{ title: "TLS & SSL", href: "/docs/architecture/tls-ssl" },
										{ title: "JavaScript Executor & Reactor", href: "/docs/architecture/javascript-executor" },
										{ title: "POSIX Syscalls", href: "/docs/architecture/posix-syscalls" },
										{ title: "Packages & Command Resolution", href: "/docs/architecture/packages-and-command-resolution" },
										{ title: "Compiler Toolchain", href: "/docs/architecture/compiler-toolchain" },
										{ title: "Limits & Observability", href: "/docs/architecture/limits-and-observability" },
										{ title: "System Prompt", href: "/docs/system-prompt" },
										{ title: "Persistence & Sleep", href: "/docs/persistence" },
									],
								},
							],
						},
						{
							title: "More",
							collapsible: true,
							pages: [
								{ title: "Core SDK", href: "/docs/core" },
								{ title: "Debugging", href: "/docs/debugging" },
								{ title: "Benchmarks", href: "/docs/benchmarks" },
								{ title: "Cost Evaluation", href: "/docs/cost-evaluation" },
							],
						},
					],
				},
			],
		},
		{
			title: "Cookbooks",
			href: "/cookbooks",
			sidebar: [
				{ title: "Overview", href: "/cookbooks", icon: faCircleInfo },
				{
					title: "Quickstart",
					pages: [
						{ title: "Quickstart App", href: "/cookbooks/quickstart-app" },
						{ title: "Crash Course", href: "/cookbooks/crash-course" },
					],
				},
				{
					title: "Agents",
					pages: [
						{ title: "Pi Agent", href: "/cookbooks/pi" },
						{ title: "Claude Agent", href: "/cookbooks/claude" },
						{ title: "Codex Agent", href: "/cookbooks/codex" },
						{ title: "OpenCode Agent", href: "/cookbooks/opencode" },
						{ title: "Agent to Agent", href: "/cookbooks/agent-to-agent" },
					],
				},
				{
					title: "Filesystem",
					pages: [{ title: "Filesystem", href: "/cookbooks/filesystem" }],
				},
				{
					title: "Processes & Shell",
					pages: [
						{ title: "Processes", href: "/cookbooks/processes" },
						{ title: "Browser Terminal", href: "/cookbooks/browser-terminal" },
					],
				},
				{
					title: "Networking",
					pages: [{ title: "Networking", href: "/cookbooks/networking" }],
				},
				{
					title: "Sessions & Permissions",
					pages: [
						{ title: "Sessions", href: "/cookbooks/sessions" },
						{ title: "Permissions", href: "/cookbooks/permissions" },
						{ title: "Approvals", href: "/cookbooks/approvals" },
						{ title: "Authentication", href: "/cookbooks/authentication" },
						{ title: "LLM Credentials", href: "/cookbooks/llm-credentials" },
						{ title: "Multiplayer", href: "/cookbooks/multiplayer" },
						{ title: "Persistence", href: "/cookbooks/persistence" },
					],
				},
				{
					title: "Orchestration",
					pages: [
						{ title: "Cron", href: "/cookbooks/cron" },
						{ title: "Workflows", href: "/cookbooks/workflows" },
						{ title: "Webhooks", href: "/cookbooks/webhooks" },
					],
				},
				{
					title: "Integrations",
					pages: [{ title: "Vercel Eve", href: "/cookbooks/eve" }],
				},
				{
					title: "Reference",
					pages: [
						{ title: "Core", href: "/cookbooks/core" },
						{ title: "Software", href: "/cookbooks/software" },
						{ title: "Bindings", href: "/cookbooks/bindings" },
						{ title: "Resource Limits", href: "/cookbooks/resource-limits" },
						{ title: "Sandbox", href: "/cookbooks/sandbox" },
					],
				},
			],
		},
	],
};

export default siteConfig;
