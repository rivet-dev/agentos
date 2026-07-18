# LLM Credentials

Pass LLM API keys to agent sessions securely.

Pass LLM provider API keys to agent sessions so keys stay on the server and are injected at session creation, with per-tenant isolation for multi-tenant deployments.

## Passing API keys

Pass LLM provider keys via the `env` option on `openSession`. The VM does not inherit from the host `process.env`, so keys must be passed explicitly.

## Per-tenant credentials

Give each tenant an isolated VM by keying `getOrCreate` on the tenant id, look up that tenant's API key on the server, and inject it via the session `env`. Credentials stay on the server and never reach the client.

First, declare the agent software on the server:

Then resolve each tenant's key and pass it at session creation:

Because keys are resolved per tenant from your own credential store (the `lookupTenantApiKey` stand-in above) and stay on the server, each session uses the tenant's own key and one tenant's key never reaches another tenant or the client.

## Embedded LLM Gateway

The [Embedded LLM Gateway](/docs/llm-gateway) (coming soon) will remove the need to manage API keys manually. It routes all agent LLM requests through a managed proxy built into agentOS, providing per-tenant usage metering, rate limiting, and cost controls without deploying a separate gateway service.