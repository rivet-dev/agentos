#!/usr/bin/env bash
# osslserver is the real ngtcp2 1.26.0 example server, built with nghttp3/OpenSSL.
set -euo pipefail
[[ $# == 5 ]] || { echo 'usage: http3-server.sh OSSLSERVER PORT KEY CERT DOCUMENT_ROOT' >&2; exit 2; }
server=$1 port=$2 key=$3 certificate=$4 documents=$5
[[ -x "$server" && -f "$key" && -f "$certificate" && -d "$documents" ]] || exit 2
[[ "$port" =~ ^[0-9]+$ && "$port" -gt 1024 && "$port" -lt 65536 ]] || exit 2
exec timeout --signal=TERM --kill-after=2 360 "$server" --quiet --htdocs="$documents" 127.0.0.1 "$port" "$key" "$certificate"
