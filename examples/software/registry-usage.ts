import { vm } from "./actor.js";

for (const software of await vm.software.list({})) {
	console.log(software.packageId, software.commands.join(", "));
}
