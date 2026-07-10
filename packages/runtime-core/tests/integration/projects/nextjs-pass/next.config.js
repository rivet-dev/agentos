/** @type {import('next').NextConfig} */
module.exports = {
	eslint: {
		ignoreDuringBuilds: true,
	},
	experimental: {
		webpackBuildWorker: false,
	},
	typescript: {
		ignoreBuildErrors: true,
	},
};
