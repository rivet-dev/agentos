FROM node:22-alpine AS build
WORKDIR /app
RUN npm install -g pnpm@9

# Copy website package manifest and install dependencies
COPY packages/website/package.json ./
RUN pnpm install

# Copy website source (preserve node_modules from install step)
COPY packages/website/src/ ./src/
COPY packages/website/public/ ./public/
COPY packages/website/astro.config.* packages/website/tailwind.config.* packages/website/vite.config.* packages/website/tsconfig.json ./

# Build
RUN pnpm build

FROM caddy:alpine
COPY --from=build /app/dist /srv
RUN cat > /etc/caddy/Caddyfile <<'EOF'
:80 {
	root * /srv
	file_server
	try_files {path} /index.html
}
EOF
EXPOSE 80
