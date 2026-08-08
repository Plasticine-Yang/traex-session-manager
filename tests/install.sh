#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

mkdir -p "$TMP/bin" "$TMP/release/tsm-0.1.0-x86_64-apple-darwin" "$TMP/home"
cat >"$TMP/release/tsm-0.1.0-x86_64-apple-darwin/tsm" <<'EOF'
#!/bin/sh
printf '%s\n' 'tsm fixture'
EOF
chmod +x "$TMP/release/tsm-0.1.0-x86_64-apple-darwin/tsm"
tar -C "$TMP/release/tsm-0.1.0-x86_64-apple-darwin" -czf \
  "$TMP/release/tsm-0.1.0-x86_64-apple-darwin.tar.gz" tsm

if command -v shasum >/dev/null 2>&1; then
  HASH=$(shasum -a 256 "$TMP/release/tsm-0.1.0-x86_64-apple-darwin.tar.gz" | awk '{print $1}')
else
  HASH=$(sha256sum "$TMP/release/tsm-0.1.0-x86_64-apple-darwin.tar.gz" | awk '{print $1}')
fi
printf '%s  %s\n' "$HASH" 'tsm-0.1.0-x86_64-apple-darwin.tar.gz' >"$TMP/release/SHA256SUMS"

cat >"$TMP/bin/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) printf '%s\n' Darwin ;;
  -m) printf '%s\n' x86_64 ;;
  *) exit 1 ;;
esac
EOF

cat >"$TMP/bin/curl" <<'EOF'
#!/bin/sh
output=
write_effective=false
url=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) output=$2; shift 2 ;;
    -w) write_effective=true; shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
if [ "$write_effective" = true ]; then
  printf '%s\n' 'https://github.com/Plasticine-Yang/traex-session-manager/releases/tag/v0.1.0'
elif [ -n "$output" ]; then
  cp "$TSM_TEST_RELEASE_DIR/${url##*/}" "$output"
else
  cat "$TSM_TEST_RELEASE_DIR/${url##*/}"
fi
EOF
chmod +x "$TMP/bin/uname" "$TMP/bin/curl"

PATH="$TMP/bin:$PATH" HOME="$TMP/home" TSM_TEST_RELEASE_DIR="$TMP/release" \
  sh "$ROOT/install.sh"

test -x "$TMP/home/.local/bin/tsm"
test "$(readlink "$TMP/home/.local/bin/traex-session-manager")" = tsm
test "$("$TMP/home/.local/bin/traex-session-manager")" = "tsm fixture"

printf '%s\n' 'bad checksum' >"$TMP/release/SHA256SUMS"
if PATH="$TMP/bin:$PATH" HOME="$TMP/home" TSM_TEST_RELEASE_DIR="$TMP/release" \
  sh "$ROOT/install.sh" >"$TMP/bad.out" 2>"$TMP/bad.err"; then
  echo "installer accepted a bad checksum" >&2
  exit 1
fi
test "$("$TMP/home/.local/bin/tsm")" = "tsm fixture"

printf '%s\n' "install.sh tests passed"
