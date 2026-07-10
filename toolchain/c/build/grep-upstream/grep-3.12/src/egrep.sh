#!@SHELL@
cmd=${0##*/}
echo "$cmd: warning: $cmd is obsolescent; using @grep@ @option@" >&2
exec @grep@ @option@ "$@"
