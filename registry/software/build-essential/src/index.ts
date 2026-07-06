import common from "@agentos-software/common";
import git from "@agentos-software/git";
import curl from "@agentos-software/curl";

const buildEssential = [...common, git, curl];

export default buildEssential;
export { common, git, curl };
