# Models & Credentials

Choose agent models and pass provider credentials to sessions securely.

Choose the model through your agent adapter, then pass its provider credentials
to the session from trusted server code. Credentials are injected at session
creation and can be isolated per tenant.

## Passing API keys

Pass LLM provider keys via the `env` option on `openSession`. The VM does not inherit from the host `process.env`, so keys must be passed explicitly.

## Per-tenant credentials

Give each tenant an isolated VM by keying `getOrCreate` on the tenant id, look up that tenant's API key on the server, and inject it via the session `env`. Credentials stay on the server and never reach the client.

First, declare the agent software on the server:

Then resolve each tenant's key and pass it at session creation:

Because keys are resolved per tenant from your own credential store (the `lookupTenantApiKey` stand-in above) and stay on the server, each session uses the tenant's own key and one tenant's key never reaches another tenant or the client.

## Models

Model selection belongs to the configured agent adapter. AgentOS forwards the
session environment and preserves the agent's native model behavior instead of
introducing a second model-selection layer. See the page for your
[agent](/docs/agents/pi) for its supported model and provider options.