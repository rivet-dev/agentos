import coreutils from "@agentos-software/coreutils";
import sed from "@agentos-software/sed";
import grep from "@agentos-software/grep";
import gawk from "@agentos-software/gawk";
import findutils from "@agentos-software/findutils";
import diffutils from "@agentos-software/diffutils";
import tar from "@agentos-software/tar";
import gzip from "@agentos-software/gzip";

const common = [coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip];

export default common;
export { coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip };
