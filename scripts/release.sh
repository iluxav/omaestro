#!/usr/bin/env bash
# `make release`: bumps the version in Cargo.toml, Cargo.lock and
# manifest.json, commits, tags vX.Y.Z and pushes the branch and the tag.
# GitHub Actions (.github/workflows/release.yml) then builds the binaries
# for both architectures, publishes the release and pins their checksums in
# release.sha256 on the default branch; this script waits for that run when
# `gh` is available and pulls the pinned file back.
#
#   scripts/release.sh                       next patch version (0.1.0 -> 0.1.1)
#   scripts/release.sh --bump minor|major    0.1.0 -> 0.2.0 | 1.0.0
#   scripts/release.sh --version 1.2.3       exactly that
#   scripts/release.sh --dry-run             the checks and the plan, no changes
#
# YES=1 skips the confirmation; NO_CHECK=1 skips `make check` first.

set -euo pipefail

cd "$(dirname "$0")/.."

bump="patch"
version=""
dry_run=0
while (($# > 0)); do
  case "$1" in
    --bump) bump="${2:-}"; shift 2 ;;
    --version) version="${2:-}"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    -h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "release: unknown argument $1" >&2; exit 2 ;;
  esac
done

current="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
manifest_version="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' manifest.json | head -1)"
if [[ -z "$version" ]]; then
  IFS=. read -r major minor patch <<<"$current"
  case "$bump" in
    major) version="$((major + 1)).0.0" ;;
    minor) version="$major.$((minor + 1)).0" ;;
    patch) version="$major.$minor.$((patch + 1))" ;;
    *) echo "release: --bump takes major, minor or patch, not '$bump'" >&2; exit 2 ;;
  esac
fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "release: '$version' is not X.Y.Z" >&2; exit 2; }
tag="v$version"

# Everything that has to be true before a release is made, listed in one go.
problems=()
branch="$(git symbolic-ref --short -q HEAD || echo "")"
default="$(git remote show origin 2>/dev/null | sed -n 's/.*HEAD branch: //p')"
[[ -n "$default" ]] || default="main"
[[ "$branch" == "$default" ]] || problems+=("on branch '$branch', releases are made from '$default'")
[[ -z "$(git status --porcelain)" ]] || problems+=("the working tree has uncommitted changes")
# Compared through ls-remote: nothing is fetched or built from the remote
# before the checks run on the local, committed tree.
remote_head="$(git ls-remote --heads origin "$default" 2>/dev/null | cut -f1)"
if [[ -n "$remote_head" ]]; then
  [[ "$(git rev-parse HEAD)" == "$remote_head" ]] ||
    problems+=("HEAD is not origin/$default: pull or push first")
else
  problems+=("cannot reach origin")
fi
[[ "$manifest_version" == "$current" ]] ||
  problems+=("Cargo.toml says $current but manifest.json says $manifest_version")
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null || git ls-remote --exit-code --tags origin "$tag" >/dev/null 2>&1; then
  problems+=("tag $tag exists already")
fi

echo "release: $current -> $version (tag $tag, branch $default)"
echo "  1. Cargo.toml, Cargo.lock and manifest.json get version $version"
echo "  2. commit 'Release $tag', tag $tag, push $default and the tag"
echo "  3. GitHub Actions builds om for x86_64 and aarch64, publishes the release,"
echo "     and commits the checksums to release.sha256 on $default"
if ((${#problems[@]} > 0)); then
  for problem in "${problems[@]}"; do echo "  not yet: $problem"; done
  exit 1
fi
((dry_run)) && { echo "  (dry run: nothing changed)"; exit 0; }

if [[ "${NO_CHECK:-}" != 1 ]]; then
  echo "release: make check"
  make check
fi

if [[ "${YES:-}" != 1 ]]; then
  read -r -p "Release $tag? [y/N] " answer
  [[ "$answer" == y || "$answer" == Y ]] || { echo "release: not released"; exit 1; }
fi

sed -i "0,/^version = \"$current\"/s//version = \"$version\"/" Cargo.toml
sed -i "s/\"version\": *\"$current\"/\"version\": \"$version\"/" manifest.json
cargo update --workspace --offline --quiet
git add Cargo.toml Cargo.lock manifest.json
git commit -q -m "Release $tag"
git tag -a "$tag" -m "omaestro $tag"
git push -q origin "$default" "$tag"
echo "release: pushed $tag"

if ! command -v gh >/dev/null; then
  echo "release: watch the run at https://github.com/$(git remote get-url origin | sed -n 's#.*github.com[:/]\(.*\)\.git$#\1#p')/actions, then git pull for release.sha256"
  exit 0
fi

echo "release: waiting for the Release workflow"
run_id=""
for _ in $(seq 60); do
  run_id="$(gh run list --workflow release.yml --json databaseId,headBranch --jq ".[] | select(.headBranch == \"$tag\") | .databaseId" 2>/dev/null | head -1)"
  [[ -n "$run_id" ]] && break
  sleep 3
done
if [[ -z "$run_id" ]]; then
  echo "release: the run did not appear within three minutes; gh run list --workflow release.yml"
  exit 1
fi
if gh run watch "$run_id" --exit-status; then
  # The workflow committed release.sha256: take exactly that commit, by SHA.
  slug="$(git remote get-url origin | sed -n 's#.*github.com[:/]\(.*\)\.git$#\1#p')"
  pinned="$(gh api "repos/$slug/commits/$default" --jq .sha)"
  git fetch -q origin "$pinned" && git merge -q --ff-only "$pinned"
  echo "release: $tag is published; release.sha256 pinned in $pinned and pulled"
  gh release view "$tag" --json url --jq .url
else
  echo "release: the workflow failed; gh run view $run_id --log-failed" >&2
  exit 1
fi
