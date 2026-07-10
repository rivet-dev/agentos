import coreutils from "@agentos-software/coreutils";
import sed from "@agentos-software/sed";
import grep from "@agentos-software/grep";
import gawk from "@agentos-software/gawk";
import findutils from "@agentos-software/findutils";
import diffutils from "@agentos-software/diffutils";
import tar from "@agentos-software/tar";
import gzip from "@agentos-software/gzip";
import curl from "@agentos-software/curl";
import wget from "@agentos-software/wget";
import duckdb from "@agentos-software/duckdb";
import envsubst from "@agentos-software/envsubst";
import git from "@agentos-software/git";
import sqlite3 from "@agentos-software/sqlite3";
import vim from "@agentos-software/vim";
import zip from "@agentos-software/zip";
import unzip from "@agentos-software/unzip";
import jq from "@agentos-software/jq";
import ripgrep from "@agentos-software/ripgrep";
import fd from "@agentos-software/fd";
import tree from "@agentos-software/tree";
import procps from "@agentos-software/procps";
import psmisc from "@agentos-software/psmisc";
import file from "@agentos-software/file";
import yq from "@agentos-software/yq";
import codex from "@agentos-software/codex-cli";

const everything = [
	coreutils,
	sed,
	grep,
	gawk,
	findutils,
	diffutils,
	tar,
	gzip,
	curl,
	wget,
	duckdb,
	envsubst,
	git,
	sqlite3,
	vim,
	zip,
	unzip,
	jq,
	ripgrep,
	fd,
	tree,
	procps,
	psmisc,
	file,
	yq,
	codex,
];

export default everything;
export {
	coreutils,
	sed,
	grep,
	gawk,
	findutils,
	diffutils,
	tar,
	gzip,
	curl,
	wget,
	duckdb,
	envsubst,
	git,
	sqlite3,
	vim,
	zip,
	unzip,
	jq,
	ripgrep,
	fd,
	tree,
	procps,
	psmisc,
	file,
	yq,
	codex,
};
