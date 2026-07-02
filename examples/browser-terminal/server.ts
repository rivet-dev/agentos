import common from "@agentos-software/common";
import curl from "@agentos-software/curl";
import git from "@agentos-software/git";
import jq from "@agentos-software/jq";
import ripgrep from "@agentos-software/ripgrep";
import sqlite3 from "@agentos-software/sqlite3";
import { agentOS, setup } from "@rivet-dev/agentos";

const shellVm = agentOS({
	software: [common, git, curl, ripgrep, jq, sqlite3],
});

export const registry = setup({ use: { shellVm } });

registry.start();
