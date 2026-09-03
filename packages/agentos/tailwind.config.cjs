/** @type {import('tailwindcss').Config} */
// Vendored from the rivet design system's tailwind-base.ts (the subset the
// inspector tabs use): IBM Plex fonts, the --radius scale, and the semantic
// color names mapped to the CSS variables in src/inspector-tabs/styles.css.
// JIT scans only the tab sources, so the generated CSS stays tiny.
module.exports = {
	// The dashboard passes its theme in the iframe URL; main.tsx sets the
	// class. Dark is the default when the param is absent.
	darkMode: "class",
	content: ["./src/inspector-tabs/**/*.{ts,tsx,html}"],
	theme: {
		extend: {
			fontFamily: {
				sans: ["IBM Plex Sans", "ui-sans-serif", "system-ui", "sans-serif"],
				mono: ["IBM Plex Mono", "ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
			},
			borderRadius: {
				lg: "var(--radius)",
				md: "calc(var(--radius) - 2px)",
				sm: "calc(var(--radius) - 4px)",
			},
			colors: {
				border: "hsl(var(--border))",
				input: "hsl(var(--input))",
				ring: "hsl(var(--ring))",
				background: "hsl(var(--background))",
				foreground: "hsl(var(--foreground))",
				primary: {
					DEFAULT: "hsl(var(--primary))",
					foreground: "hsl(var(--primary-foreground))",
				},
				secondary: {
					DEFAULT: "hsl(var(--secondary))",
					foreground: "hsl(var(--secondary-foreground))",
				},
				muted: {
					DEFAULT: "hsl(var(--muted))",
					foreground: "hsl(var(--muted-foreground))",
				},
				accent: {
					DEFAULT: "hsl(var(--accent))",
					foreground: "hsl(var(--accent-foreground))",
				},
				card: {
					DEFAULT: "hsl(var(--card))",
					foreground: "hsl(var(--card-foreground))",
				},
				popover: {
					DEFAULT: "hsl(var(--popover))",
					foreground: "hsl(var(--popover-foreground))",
				},
				destructive: {
					DEFAULT: "hsl(var(--destructive))",
					foreground: "hsl(var(--destructive-foreground))",
				},
				warning: "hsl(var(--warning))",
			},
		},
	},
	plugins: [],
};
