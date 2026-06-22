import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const conn = client.vm.getOrCreate("my-agent").connect();

conn.on("shellData", (data) => {
  // data.shellId: string
  // data.data: Uint8Array
  const text = new TextDecoder().decode(data.data);
  process.stdout.write(text);
});
