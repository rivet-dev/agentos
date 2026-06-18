import { defineConfig } from "astro/config";
import react from "@astrojs/react";
import tailwind from "@astrojs/tailwind";
import sitemap from "@astrojs/sitemap";
import starlight from "@astrojs/starlight";

// PostHog (Rivet instance) — ported from the original Mintlify docs config.
const posthogSnippet = `
!function(t,e){var o,n,p,r;e.__SV||(window.posthog=e,e._i=[],e.init=function(i,s,a){function g(t,e){var o=e.split(".");2==o.length&&(t=t[o[0]],e=o[1]),t[e]=function(){t.push([e].concat(Array.prototype.slice.call(arguments,0)))}}(p=t.createElement("script")).type="text/javascript",p.async=!0,p.src=s.api_host+"/static/array.js",(r=t.getElementsByTagName("script")[0]).parentNode.insertBefore(p,r);var u=e;for(void 0!==a?u=e[a]=[]:a="posthog",u.people=u.people||[],u.toString=function(t){var e="posthog";return"posthog"!==a&&(e+="."+a),t||(e+=" (stub)"),e},u.people.toString=function(){return u.toString(1)+".people (stub)"},o="capture identify alias people.set people.set_once set_config register register_once unregister opt_out_capturing has_opted_out_capturing opt_in_capturing reset isFeatureEnabled onFeatureFlags getFeatureFlag getFeatureFlagPayload reloadFeatureFlags group updateEarlyAccessFeatureEnrollment getEarlyAccessFeatures getActiveMatchingSurveys getSurveys getNextSurveyStep onSessionId".split(" "),n=0;n<o.length;n++)g(u,o[n]);e._i.push([i,s,a])},e.__SV=1)}(document,window.posthog||[]);
posthog.init('phc_6kfTNEAVw7rn1LA51cO3D69FefbKupSWFaM7OUgEpEo',{api_host:'https://ph.rivet.gg',session_recording:{maskAllInputs:false}});
`;

// https://astro.build/config
export default defineConfig({
	site: "https://agentos-sdk.dev",
	output: "static",
	integrations: [
		react(),
		// Tailwind base styles are scoped to the landing page (which imports
		// global.css itself); disabling global injection keeps them out of
		// Starlight's docs pages.
		tailwind({ applyBaseStyles: false }),
		starlight({
			title: "Agent OS",
			favicon: "/favicon.svg",
			logo: {
				light: "./src/assets/logo.svg",
				dark: "./src/assets/logo.svg",
				replacesTitle: true,
			},
			customCss: ["./src/styles/starlight-custom.css"],
			social: [
				{
					icon: "github",
					label: "GitHub",
					href: "https://github.com/rivet-dev/agent-os",
				},
			],
			head: [{ tag: "script", content: posthogSnippet }],
			sidebar: [
				{
					label: "Getting Started",
					items: [
						"docs",
						"docs/quickstart",
						"docs/crash-course",
						"docs/versus-sandbox",
					],
				},
				{
					label: "Agents",
					items: [
						"docs/agents/pi",
						"docs/agents/claude",
						"docs/agents/codex",
						"docs/agents/amp",
						"docs/agents/opencode",
						"docs/sessions",
						"docs/permissions",
						"docs/tools",
						"docs/llm-credentials",
						"docs/llm-gateway",
					],
				},
				{
					label: "Operating System",
					items: [
						"docs/software",
						"docs/filesystem",
						"docs/processes",
						"docs/networking",
						"docs/cron",
						"docs/sandbox",
						"docs/security",
					],
				},
				{
					label: "Orchestration",
					items: [
						"docs/authentication",
						"docs/webhooks",
						"docs/multiplayer",
						"docs/agent-to-agent",
						"docs/workflows",
						"docs/queues",
						"docs/sqlite",
					],
				},
				{
					label: "Reference",
					items: [
						"docs/core",
						"docs/configuration",
						"docs/events",
						"docs/deployment",
						"docs/limitations",
						{
							label: "Internals",
							items: [
								"docs/security-model",
								"docs/persistence",
								"docs/system-prompt",
								"docs/benchmarks",
							],
						},
					],
				},
			],
		}),
		sitemap(),
	],
});
