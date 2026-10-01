#!/usr/bin/env bash
# `make dist`: builds the release binary for this machine's architecture,
# prints its SHA256, and records it in release.sha256 (what
# scripts/plugin-start.sh checks downloads against). This is the by-hand
# version of what .github/workflows/release.yml does on a tag for both
# architectures; `make release` is the normal way to publish.

set -euo pipefail

cd "$(dirname "$0")/.."
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
manifest_version="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' manifest.json | head -1)"
if [[ "$version" != "$manifest_version" ]]; then
  echo "Cargo.toml says $version, manifest.json says $manifest_version; make them agree" >&2
  exit 1
fi
arch="$(uname -m)"

cargo build --release
mkdir -p dist
cp target/release/om "dist/om-$arch"
sum="$(sha256sum "dist/om-$arch" | awk '{print $1}')"

touch release.sha256
grep -v " $arch\$" release.sha256 >release.sha256.new || true
echo "$sum $arch" >>release.sha256.new
mv release.sha256.new release.sha256

echo "dist/om-$arch  $sum"
echo "release.sha256 now holds:"
cat release.sha256
echo
echo "next: gh release create v$version dist/om-$arch  (add the other architecture's file too), or use \`make release\`"
