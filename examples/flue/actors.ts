import { setup } from "rivetkit";

// Flue adds its product-owned actors to this registry during `flue build`.
// The hosted agentOS actor is a separate static deployment and is not embedded
// or configured from this application.
export const registry = setup({ use: {} });
