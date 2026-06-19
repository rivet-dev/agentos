import { defineConfig } from "astro/config";
import react from "@astrojs/react";
import tailwind from "@astrojs/tailwind";
import sitemap from "@astrojs/sitemap";
import starlight from "@astrojs/starlight";
import { docsTheme } from "@rivet-dev/docs-theme";
import { siteConfig } from "./docs.config.mjs";

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
		// The shared Rivet docs theme — wraps Starlight entirely. All docs
		// branding/chrome lives in @rivet-dev/docs-theme; docs.config.mjs maps
		// Agent OS's identity, nav, and pages onto it.
		...docsTheme(starlight, siteConfig),
		sitemap(),
	],
});
