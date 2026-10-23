#!/bin/sh
# Builds xavier-server-<version>-<target>.tar.gz (+ .sha256) from a built binary.
# Usage: make-tarball.sh <binary> <version> <target> <out-dir>
set -eu
bin="$1"; version="$2"; target="$3"; out="$4"
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
name="xavier-server-${version}-${target}"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT INT TERM
dir="$stage/$name"
install -d "$dir/bin" "$dir/share/xavier/systemd"
install -m 0755 "$bin" "$dir/bin/xavier"
install -m 0755 "$here/install.sh" "$dir/install.sh"
install -m 0755 "$here/setup-host.sh" "$dir/share/xavier/setup-host.sh"
install -m 0644 "$here/xavier.env.example" "$dir/share/xavier/xavier.env.example"
install -m 0644 "$here/xavier.service" "$here/xavier-user.service" "$dir/share/xavier/systemd/"
install -m 0644 "$repo/LICENSE" "$dir/LICENSE"
install -m 0644 "$repo/docs/DEPLOY/LINUX_SERVER.md" "$dir/LINUX_SERVER.md"
mkdir -p "$out"
tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "$out/$name.tar.gz"
