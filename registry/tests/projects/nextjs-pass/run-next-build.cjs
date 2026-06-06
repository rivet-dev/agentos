const projectDir = __dirname;

require("./next-wasm-shim.cjs");

const { nextBuild } = require("next/dist/cli/next-build");

nextBuild(
	{
		debug: false,
		experimentalAppOnly: false,
		experimentalBuildMode: "compile",
		experimentalDebugMemoryUsage: false,
		experimentalTurbo: false,
		lint: true,
		mangling: true,
		profile: false,
	},
	projectDir,
);
