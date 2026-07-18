/**
 * Convenience re-export of the unmodified `rivetkit/client` entrypoint.
 *
 * This subpath exists so AgentOS applications only need a direct dependency on
 * `@rivet-dev/agentos`. It must never wrap, proxy, decorate, specialize, or add
 * behavior to RivetKit clients, accessors, handles, connections, actions, or
 * events.
 */
export * from "rivetkit/client";
