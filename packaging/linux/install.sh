#!/bin/sh
# Xavier headless server installer.
#
#   curl -fsSL https://raw.githubusercontent.com/iberi22/xavier/main/packaging/linux/install.sh | sh
#   sh install.sh [--version vX.Y.Z] [--archive FILE [--sha256 FILE]] [--user] [--no-start] [--force]
#
# As root it installs a system service (user "xavier", /var/lib/xavier, /etc/xavier/xavier.env).
# As a normal user, or with --user, it installs into ~/.local and a systemd --user unit.
# The release archive is always verified against its .sha256 before anything is installed.
set -eu

REPO="iberi22/xavier"
version=""
archive=""
sha_file=""
mode=""
start=1
force=0

die() { echo "install.sh: $*" >&2; exit 1; }
log() { echo "install.sh: $*"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --version) version="${2:?}"; shift 2 ;;
        --archive) archive="${2:?}"; shift 2 ;;
        --sha256) sha_file="${2:?}"; shift 2 ;;
        --user) mode=user; shift ;;
        --system) mode=system; shift ;;
        --no-start) start=0; shift ;;
        --force) force=1; shift ;;
        -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

if [ -z "$mode" ]; then
    if [ "$(id -u)" -eq 0 ]; then mode=system; else mode=user; fi
fi
[ "$mode" = system ] && [ "$(id -u)" -ne 0 ] && die "--system needs root"

fetch() {
    if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then wget -qO "$2" "$1"
    else die "need curl or wget"; fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

detect_target() {
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) die "unsupported architecture: $(uname -m)" ;;
    esac
    [ "$(uname -s)" = Linux ] || die "this installer targets Linux"
    echo "${arch}-unknown-linux-gnu"
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

if [ -z "$archive" ]; then
    target=$(detect_target)
    if [ -z "$version" ]; then
        fetch "https://api.github.com/repos/$REPO/releases/latest" "$tmp/latest.json"
        version=$(sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n1)
        [ -n "$version" ] || die "could not resolve the latest release"
    fi
    name="xavier-server-${version}-${target}.tar.gz"
    base="https://github.com/$REPO/releases/download/$version"
    log "downloading $name"
    fetch "$base/$name" "$tmp/$name"
    fetch "$base/$name.sha256" "$tmp/$name.sha256"
    archive="$tmp/$name"
    sha_file="$tmp/$name.sha256"
fi

[ -f "$archive" ] || die "archive not found: $archive"
[ -n "$sha_file" ] || sha_file="$archive.sha256"
[ -f "$sha_file" ] || die "checksum file not found: $sha_file (refusing to install unverified)"
expected=$(cut -d' ' -f1 < "$sha_file")
actual=$(sha256_of "$archive")
[ "$expected" = "$actual" ] || die "checksum mismatch for $(basename "$archive")"
log "checksum ok"

mkdir -p "$tmp/x"
tar -xzf "$archive" -C "$tmp/x"
root=$(find "$tmp/x" -mindepth 1 -maxdepth 1 -type d | head -n1)
[ -x "$root/bin/xavier" ] || die "archive has no bin/xavier"
share="$root/share/xavier"

has_systemd() { [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; }

if [ "$mode" = system ]; then
    install -m 0755 "$root/bin/xavier" /usr/local/bin/xavier
    install -d -m 0755 /usr/share/xavier
    install -m 0644 "$share/xavier.env.example" /usr/share/xavier/xavier.env.example
    sh "$share/setup-host.sh" /usr/share/xavier/xavier.env.example
    unit=/etc/systemd/system/xavier.service
    if [ -f "$unit" ] && [ "$force" -eq 0 ] && ! grep -q 'Xavier memory server (headless)' "$unit"; then
        log "keeping existing $unit (not installed by this script; use --force to replace)"
    else
        sed 's#^ExecStart=/usr/bin/xavier #ExecStart=/usr/local/bin/xavier #' \
            "$share/systemd/xavier.service" > "$tmp/xavier.service"
        install -m 0644 "$tmp/xavier.service" "$unit"
    fi
    if has_systemd; then
        systemctl daemon-reload
        if [ "$start" -eq 1 ]; then systemctl enable --now xavier.service; fi
    fi
    log "installed /usr/local/bin/xavier; token in /etc/xavier/xavier.env"
else
    bin_dir="$HOME/.local/bin"
    conf_dir="$HOME/.config/xavier"
    unit_dir="$HOME/.config/systemd/user"
    install -d -m 0755 "$bin_dir" "$HOME/.local/share/xavier"
    install -d -m 0700 "$conf_dir"
    install -m 0755 "$root/bin/xavier" "$bin_dir/xavier"
    if [ ! -f "$conf_dir/xavier.env" ]; then
        token=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
        (umask 077; sed "s/^XAVIER_TOKEN=.*/XAVIER_TOKEN=$token/" "$share/xavier.env.example" > "$conf_dir/xavier.env")
        log "wrote $conf_dir/xavier.env with a new XAVIER_TOKEN"
    fi
    unit="$unit_dir/xavier.service"
    if [ -f "$unit" ] && [ "$force" -eq 0 ] && ! grep -q 'Xavier memory server (headless, user)' "$unit"; then
        log "keeping existing $unit (not installed by this script; use --force to replace)"
    else
        install -d -m 0755 "$unit_dir"
        install -m 0644 "$share/systemd/xavier-user.service" "$unit"
        if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
            systemctl --user daemon-reload
            if [ "$start" -eq 1 ]; then systemctl --user enable --now xavier.service; fi
        fi
    fi
    log "installed $bin_dir/xavier; token in $conf_dir/xavier.env"
fi
