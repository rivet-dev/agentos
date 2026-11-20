import common from "@agentos-software/common";
import git from "@agentos-software/git";
import ssh from "@agentos-software/ssh";
import curl from "@agentos-software/curl";

const buildEssential = [...common, git, ssh, curl];

export default buildEssential;
export { common, git, ssh, curl };
