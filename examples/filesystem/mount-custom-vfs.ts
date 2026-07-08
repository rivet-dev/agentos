import { AgentOs, createInMemoryFileSystem } from "@rivet-dev/agentos-core";

const vm = await AgentOs.create({ defaultSoftware: false });
const driver = createInMemoryFileSystem();

await vm.mountFs("/home/agentos/scratch", driver);
await vm.writeFile("/home/agentos/scratch/hello.txt", "hello");
