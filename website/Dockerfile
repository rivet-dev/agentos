# Build the Astro site. Railway (and most platforms) use the repository root as
# the Docker build context, so paths here are written relative to the repo root
# (the site lives in website/). The root .dockerignore narrows the context to
# website/ plus the agentos SDK source (needed by the typedoc API-gen step), so
# the multi-GB monorepo is never uploaded. The shared docs theme
# (@rivet-dev/docs-theme) is fetched from GitHub during install.
FROM node:22-alpine AS build
WORKDIR /app
# git is required for pnpm to resolve the github: docs-theme dependency.
RUN apk add --no-cache git && npm install -g pnpm@9

# Install deps first (cache layer) — from the website package, not the monorepo root.
COPY website/package.json ./
RUN pnpm install

# Copy the site source.
COPY website/ ./

# The docs:gen:ts (typedoc) step in the build reads the SDK source from
# ../packages, which lives outside website/. Bring just that package's source
# into the image at the path typedoc.json points at (../packages/agentos).
COPY packages/agentos /packages/agentos

RUN pnpm build

# Serve the static output. This is a static multi-page site (Astro/Starlight
# emits <path>/index.html), NOT an SPA — so resolve each page's index.html and
# serve the real 404.html on a miss (no SPA fallback to the landing page).
FROM caddy:alpine
COPY --from=build /app/dist /srv
RUN cat > /etc/caddy/Caddyfile <<'EOF'
:80 {
	root * /srv
	encode gzip

	# Canonicalize directory requests to a trailing slash BEFORE try_files maps
	# them to <path>/index.html. Pages like the TypeDoc /api index reference
	# their assets with relative URLs (e.g. "assets/style.css"). Without the
	# trailing slash the browser's base URL stays "/api", so those relatives
	# resolve to "/assets/style.css" at the site root and 404 (served as the
	# unstyled 404.html). Redirecting "/api" -> "/api/" makes the base URL
	# "/api/" so relative assets resolve to "/api/assets/...". The matcher only
	# fires when the bare path is itself a real directory on disk, so genuine
	# extension-less pages handled via {path}.html are unaffected.
	@dir_no_slash {
		not path */
		file {
			try_files {path}/index.html
		}
	}
	redir @dir_no_slash {path}/ 308

	try_files {path} {path}/index.html {path}.html
	file_server
	handle_errors {
		rewrite * /404.html
		file_server
	}
}
EOF
EXPOSE 80
