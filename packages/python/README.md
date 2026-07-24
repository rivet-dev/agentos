# AgentOS language execution for Python

Execute Python and install Python packages inside an AgentOS VM.

```sh
pnpm add @rivet-dev/agentos-python
```

```ts
import { PythonRuntime } from "@rivet-dev/agentos-python";

const runtime = await PythonRuntime.create();
const result = await runtime.execute(`print("hello")`);
await runtime.dispose();
```

See the [AgentOS Python documentation](https://agentos-sdk.dev/docs/execution/python) for the complete API.
