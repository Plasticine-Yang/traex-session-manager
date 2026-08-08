#!/bin/sh
set -eu

OWNER_REPO=Plasticine-Yang/traex-session-manager
INSTALL_DIR="$HOME/.local/bin"
LATEST_URL="https://github.com/$OWNER_REPO/releases/latest"

fail() {
  printf 'tsm installer: %s\n' "$*" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-musl ;;
  *) fail "unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  arm64 | aarch64) arch=aarch64 ;;
  *) fail "unsupported architecture: $(uname -m)" ;;
esac

target="$arch-$os"
effective_url=$(curl -fsSL -o /dev/null -w '%{url_effective}' "$LATEST_URL")
tag=${effective_url%/}
tag=${tag##*/}
version=${tag#v}
[ "$tag" != "$version" ] && [ -n "$version" ] ||
  fail "latest release URL did not end in a vX.Y.Z tag"

asset="tsm-$version-$target.tar.gz"
download_base="https://github.com/$OWNER_REPO/releases/latest/download"
tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/tsm-install.XXXXXX")
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

curl -fsSL "$download_base/$asset" -o "$tmp_dir/$asset"
curl -fsSL "$download_base/SHA256SUMS" -o "$tmp_dir/SHA256SUMS"

expected=$(
  awk -v asset="$asset" '$2 == asset || $2 == "*" asset { print $1; exit }' \
    "$tmp_dir/SHA256SUMS"
)
[ -n "$expected" ] || fail "$asset is missing from SHA256SUMS"

if command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$tmp_dir/$asset" | awk '{print $1}')
elif command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp_dir/$asset" | awk '{print $1}')
else
  fail "shasum or sha256sum is required"
fi
[ "$actual" = "$expected" ] || fail "SHA256 checksum mismatch for $asset"

mkdir "$tmp_dir/unpacked"
tar -xzf "$tmp_dir/$asset" -C "$tmp_dir/unpacked"
[ -f "$tmp_dir/unpacked/tsm" ] || fail "$asset does not contain tsm"

mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp_dir/unpacked/tsm" "$INSTALL_DIR/.tsm.new"
mv -f "$INSTALL_DIR/.tsm.new" "$INSTALL_DIR/tsm"
ln -sfn tsm "$INSTALL_DIR/traex-session-manager"

printf 'installed tsm %s to %s/tsm\n' "$version" "$INSTALL_DIR"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf 'note: %s is not in PATH; add it to your shell profile\n' "$INSTALL_DIR" >&2
    ;;
esac
