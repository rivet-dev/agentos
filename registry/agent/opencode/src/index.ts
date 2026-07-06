import type { SoftwarePackageRef } from "@agentos-software/manifest";

const packageTar = new URL("./package.tar", import.meta.url).pathname;

export default { packageTar } satisfies SoftwarePackageRef;
