import { agentOS, setup } from "@rivet-dev/agentos";

let nextVmId = 0;

const vm = agentOS({
	defaultSoftware: false,
	createOptions: async () => {
		const vmId = ++nextVmId;
		return {
			options: {
				additionalInstructions: `This is actor VM ${vmId}.`,
			},
			dispose: () => {
				console.log(`disposed actor VM ${vmId}`);
			},
		};
	},
});

export const registry = setup({ use: { vm } });
