# @rivet-dev/agentos-flue

Use [agentOS](https://rivet.dev/agentos) as the sandbox for a
[Flue](https://flueframework.com) agent.

```ts
import { agentOS, setup } from "@rivet-dev/agentos";
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { createAgent } from "@flue/runtime";

const registry = setup({ use: { vm: agentOS() } });

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-5",
	sandbox: agentOSSandbox({ actor: "vm", registry }),
}));
```

Each Flue context maps to a stable agentOS actor with a durable `/workspace`
filesystem. The registry starts lazily in the same process.

See the [Flue integration guide](https://rivet.dev/agentos/docs/frameworks/flue)
and [complete example](https://github.com/rivet-dev/agentos/tree/main/examples/flue).
