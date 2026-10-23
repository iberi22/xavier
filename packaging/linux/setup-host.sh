#!/bin/sh
# Creates the xavier system user, its state directory and /etc/xavier/xavier.env.
# Shared by the .deb/.rpm maintainer scripts and install.sh. Idempotent: an
# existing env file (and its token) is never overwritten.
# Usage: setup-host.sh <path-to-xavier.env.example>
set -eu

example="${1:-/usr/share/xavier/xavier.env.example}"
state_dir=/var/lib/xavier
conf_dir=/etc/xavier
env_file="$conf_dir/xavier.env"

if ! getent group xavier >/dev/null 2>&1; then
    groupadd --system xavier
fi
if ! getent passwd xavier >/dev/null 2>&1; then
    useradd --system --gid xavier --home-dir "$state_dir" --no-create-home \
        --shell /usr/sbin/nologin --comment "Xavier memory server" xavier
fi

install -d -m 0750 -o xavier -g xavier "$state_dir" "$state_dir/data"
install -d -m 0750 -o root -g xavier "$conf_dir"

if [ ! -f "$env_file" ]; then
    token=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
    tmp=$(mktemp "$conf_dir/.xavier.env.XXXXXX")
    sed "s/^XAVIER_TOKEN=.*/XAVIER_TOKEN=$token/" "$example" > "$tmp"
    chown root:xavier "$tmp"
    chmod 0640 "$tmp"
    mv "$tmp" "$env_file"
    echo "xavier: wrote $env_file with a new XAVIER_TOKEN"
fi

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
fi
