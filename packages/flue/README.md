# @rivet-dev/agentos-flue

Use [agentOS](https://agentos-sdk.dev) as the sandbox for a
[Flue](https://flueframework.com) agent.

```ts
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { createAgent } from "@flue/runtime";

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-5",
	sandbox: agentOSSandbox({
		createInput: {
			config: { environment: { NODE_ENV: "production" } },
		},
	}),
}));
```

Each Flue context maps to a stable instance of the fixed Rust agentOS actor with
a durable `/workspace` filesystem.

See the [Flue integration guide](https://agentos-sdk.dev/docs/frameworks/flue)
and [complete example](https://github.com/rivet-dev/agentos/tree/main/examples/flue).
