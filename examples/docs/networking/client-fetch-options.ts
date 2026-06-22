import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

const response = await client.vm.getOrCreate("my-agent").vmFetch(3000, "/api/data", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ key: "value" }),
});

console.log("Status:", response.status, response.statusText);
console.log("Headers:", response.headers);
console.log("Body:", new TextDecoder().decode(response.body));
