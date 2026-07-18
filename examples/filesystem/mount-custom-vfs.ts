import { AgentOs } from "@rivet-dev/agentos-core";

const vm = await AgentOs.create({ defaultSoftware: false });
await vm.mountFs({
	path: "/home/agentos/scratch",
	plugin: { id: "memory", config: {} },
});
await vm.writeFile("/home/agentos/scratch/hello.txt", "hello");
