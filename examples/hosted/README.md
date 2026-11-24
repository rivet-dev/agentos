# Hosted agentOS example

Run the native hosted worker with the `hosted` Cargo feature enabled and connect
it to your local Rivet control plane. Set `RIVET_ENDPOINT`, `RIVET_NAMESPACE`,
`RIVET_POOL`, and `RIVET_TOKEN`, then run `pnpm --dir examples/hosted start`.

The example uses the generated Rust actor contract. It creates or resolves the
VM by key, writes a file, runs a bounded process, and disposes its client. The
configured `/workspace` mount remains durable after the client disconnects. `AGENTOS_VM_KEY` selects a key
other than `hosted-example`.

Keep worker credentials on your backend. A hosted actor accepts serializable
VM configuration and remote package URLs; it does not accept host callbacks,
host directory mounts, or local package paths.

The checked credential helpers in `credentials.ts` use Rivet 3.0.7 JWT issuance.
Keep the admin credential on your backend. Give workers a node JWT (including
preload coordinator grants) and give end users only an Actor-ID-scoped token
after your backend checks VM ownership. A scoped VM token authorizes the VM's agentOS
actions; it does not define application roles or permit creating another VM.

Trusted worker bootstrap needs `actor_gateway:create/read` for its shared
preload coordinator, alongside the `node` and `actor` grants shown in the helper.
Existing-Actor caller credentials remain gateway-read only. Rivet 3.0.7 accepts
tokens for its published 30 s clock-skew allowance after nominal expiry.
