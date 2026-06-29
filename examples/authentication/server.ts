import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

// The credential shape clients pass when they connect. This documents the
// connection params; the client's params are typed as unknown, so the real
// check is the onBeforeConnect hook below.
interface ConnParams {
  authToken: string;
}

// Validate credentials server-side. onBeforeConnect receives the connection
// params and rejects the connection by throwing. Wired via the underlying Rivet
// Actor; see Actor Authentication for the full hook signatures.
export function onBeforeConnect(_c: unknown, params: ConnParams): void {
  if (typeof params?.authToken !== "string" || params.authToken.length === 0) {
    throw new Error("missing or invalid authToken");
  }
  // verify the token (JWT signature, lookup, ...) here
}

const vm = agentOS<ConnParams>({
  software: [pi],
});

export const registry = setup({ use: { vm } });
registry.start();
