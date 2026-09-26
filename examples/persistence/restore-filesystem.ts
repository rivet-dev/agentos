import { vm } from "./client.js";

// Files written before sleep are restored when the actor wakes.
const contents = await vm.filesystem.readFile({
	path: "/home/agentos/notes.md",
});
console.log(new TextDecoder().decode(contents));
