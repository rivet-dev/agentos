import type { SoftwarePackageRef } from "@agentos-software/manifest";
import codexCli from "@agentos-software/codex-cli";

const packagePath = new URL("./package.aospkg", import.meta.url).pathname;

export default [{ packagePath }, codexCli] satisfies SoftwarePackageRef[];
