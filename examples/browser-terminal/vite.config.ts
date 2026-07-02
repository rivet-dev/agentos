import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The RivetKit server (server.ts) listens on :6420. The web app talks to it
// directly from the browser; RivetKit serves permissive CORS for public
// clients, so no proxy is needed. Override the endpoint with
// VITE_AGENTOS_ENDPOINT if you run the server elsewhere.
export default defineConfig({
	plugins: [react()],
	server: {
		port: 5173,
	},
});
