<p align="center">
  <img src="https://raw.githubusercontent.com/rivet-dev/agentos/main/.github/media/secure-exec-logo.png" alt="Secure Exec" height="160" />
</p>

<p align="center">
  Secure Node.js execution without a sandbox.<br/>Run untrusted JavaScript and TypeScript in an isolated VM with real Node.js APIs, npm packages, and a virtual filesystem.
</p>

<p align="center">
  <a href="https://rivet.dev/secure-exec/docs/quickstart">Quickstart</a> | <a href="https://rivet.dev/secure-exec/docs">Documentation</a> | <a href="https://rivet.dev/discord">Discord</a>
</p>

## Install

```bash
npm install secure-exec
```

Requires Node.js 22+ on Linux or macOS.

## Run Code

Each call runs in a fresh VM that is disposed when the call finishes. Nothing is
shared between calls.

```ts
import { evaluate, execute } from "secure-exec";

const sum = await evaluate<number>("1 + 2");
if (sum.outcome === "succeeded") console.log(sum.value); // 3

const run = await execute(`console.log("hello")`, {
  output: { capture: "all" },
  timeoutMs: 5_000,
});
console.log(run.stdout); // hello
```

`evaluate` takes one expression and returns its JSON value; wrap several
statements in a function. `execute` runs a whole module for its side effects.

`outcome` is `succeeded`, `failed`, `cancelled`, or `timed_out`. Every outcome
other than `succeeded` carries an `error`, and guest stack traces arrive on
`stderr` when you capture it.

## Configure the VM

VM options go on the call: `permissions`, `limits`, `mounts`, and the rest of
the agentOS VM options. External network access is denied unless you allow it,
and a policy is merged over the defaults.

```ts
await evaluate(
  `(async () => {
    const response = await fetch("https://example.com");
    await response.text();
    return response.status;
  })()`,
  { permissions: { network: "allow" } },
);
```

## Keep things with a VM

The functions above are one-shot conveniences. When files, installed packages,
or a running server should outlast one call, create a VM. It is an agentOS VM
with Secure Exec's defaults.

```ts
import { createVm } from "secure-exec";

const vm = await createVm({ permissions: { network: "allow" } });
try {
  await vm.npm.install(["zod"]);
  await vm.filesystem.writeFile("/workspace/data.json", JSON.stringify({ n: 40 }));

  const result = await vm.javascript.evaluate<number>(
    `import("node:fs").then((fs) => JSON.parse(fs.readFileSync("/workspace/data.json", "utf8")).n + 2)`,
  );

  const server = await vm.javascript.spawn(`/* long-running code */`);
  await vm.process.kill(server.pid);
} finally {
  await vm.dispose();
}
```

`vm.javascript`, `vm.typescript`, `vm.npm`, `vm.filesystem`, `vm.network`, and
`vm.process` behave exactly as they do in agentOS.

## Keep variables with a context

Each call on a VM starts with fresh JavaScript memory. A context keeps it, like a
REPL, and several contexts run in parallel in one VM.

```ts
const context = await vm.createContext();

await context.execute("globalThis.total = 40");
const result = await context.evaluate<number>("total + 2");

await context.reset();   // clear state
await context.dispose(); // delete the context, keep the VM
```

## Host functions

Give the code your own functions without giving it your credentials. Each
collection is a global inside the VM, and each function is async.

```ts
import { evaluate, hostFunction, hostFunctions } from "secure-exec";
import { z } from "zod";

const orders = hostFunctions({
  name: "orders",
  description: "Look up customer orders.",
  functions: {
    list: hostFunction({
      description: "List a customer's orders.",
      inputSchema: z.object({ customer: z.string() }),
      execute: ({ customer }) => db.orders.findMany({ customer }),
    }),
  },
});

await evaluate(`orders.list({ customer: "c_123" }).then((rows) => rows.length)`, {
  hostFunctions: [orders],
});
```

## TypeScript

`secure-exec/typescript` has the same `execute` and `evaluate`, plus `check`.
Running TypeScript strips types without checking them, so check first when it
matters.

```ts
import { check, evaluate } from "secure-exec/typescript";

const checked = await check(`const total: number = "nope";`);
for (const diagnostic of checked.diagnostics) console.log(diagnostic.message);
```

## Lifecycle

The first call starts a shared sidecar process. Call `init()` at startup to pay
that cost ahead of time, and `shutdown()` to stop it, for example in a test
teardown hook.

```ts
import { init, shutdown } from "secure-exec";

await init();
// ...
await shutdown();
```

## More

Need Python? Use the [agentOS Python execution API](https://rivet.dev/agentos/docs/python), which has the same `execute` and `evaluate` shape.

For processes, filesystem access, and agent sessions, use
[`@rivet-dev/agentos-core`](https://rivet.dev/agentos) directly. Read the [documentation](https://rivet.dev/secure-exec/docs), or browse the examples in
[`secure-exec/examples`](https://github.com/rivet-dev/agentos/tree/main/secure-exec/examples).
