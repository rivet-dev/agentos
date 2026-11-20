#!/usr/bin/env bash
# Real upstream nghttpx handles QUIC/HTTP3; loopback HTTP/1 backend checks bytes.
set -euo pipefail
[[ $# == 5 ]] || { echo 'usage: http3-proxy.sh NGHTTPX UDP_PORT KEY CERT BACKEND_PORT' >&2; exit 2; }
server=$1 port=$2 key=$3 certificate=$4 backend=$5
[[ -x "$server" && -f "$key" && -f "$certificate" ]] || exit 2
for value in "$port" "$backend"; do
  [[ "$value" =~ ^[0-9]+$ && "$value" -gt 1024 && "$value" -lt 65536 ]] || exit 2
done
exec timeout --signal=TERM --kill-after=2 360 "$server" --conf=/dev/null --workers=1 \
  "--frontend=127.0.0.1,$port;quic" "--backend=127.0.0.1,$backend" \
  --frontend-quic-idle-timeout=5s "$key" "$certificate"
