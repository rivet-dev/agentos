#!/usr/bin/env bash
# Launch real OpenSSH on loopback with generated credentials. Caller owns cleanup.
set -euo pipefail
[[ $# == 2 ]] || { echo 'usage: ssh-server.sh ABSOLUTE_FIXTURE_DIR PORT' >&2; exit 2; }
directory=$1
port=$2
[[ "$directory" = /* && "$directory" != *$'\n'* && "$directory" != *'"'* ]] || exit 2
[[ "$port" =~ ^[0-9]+$ && "$port" -gt 1024 && "$port" -lt 65536 ]] || exit 2
: "${SSHD:=/usr/sbin/sshd}"
[[ -x "$SSHD" ]] || { echo 'A real native OpenSSH sshd executable is required' >&2; exit 2; }
mkdir -p "$directory"
chmod 700 "$directory"
ssh-keygen -q -t ed25519 -N '' -f "$directory/host_key"
ssh-keygen -q -t ed25519 -N '' -f "$directory/client_key"
cp "$directory/client_key.pub" "$directory/authorized_keys"
chmod 600 "$directory/authorized_keys"
username=$(id -un)
cat > "$directory/sshd_config" <<CONFIG
ListenAddress 127.0.0.1
Port $port
HostKey "$directory/host_key"
PidFile "$directory/sshd.pid"
AuthorizedKeysFile "$directory/authorized_keys"
AllowUsers $username
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes yes
PermitRootLogin prohibit-password
AllowAgentForwarding no
AllowTcpForwarding no
X11Forwarding no
PermitTunnel no
PermitTTY no
MaxAuthTries 2
MaxSessions 4
MaxStartups 4
LoginGraceTime 5
Subsystem sftp internal-sftp
LogLevel ERROR
CONFIG
"$SSHD" -t -f "$directory/sshd_config"
ssh-keygen -lf "$directory/host_key.pub" -E sha256 > "$directory/host-fingerprint"
# timeout propagates TERM, then KILL after two seconds. No detached daemon.
exec timeout --signal=TERM --kill-after=2 360 "$SSHD" -D -e -f "$directory/sshd_config"
