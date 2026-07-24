# AgentOS language execution for JavaScript

Execute JavaScript and TypeScript inside an AgentOS VM.

```sh
pnpm add @rivet-dev/agentos-javascript
```

```ts
import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const runtime = await JavaScriptRuntime.create();
const result = await runtime.execute(`console.log("hello")`);
await runtime.dispose();
```

See the [AgentOS JavaScript documentation](https://agentos-sdk.dev/docs/execution/javascript) for the complete API.
