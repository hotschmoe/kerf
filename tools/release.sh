#!/usr/bin/env bash
# Cut a release: bump the one version (engine/Cargo.toml [workspace.package]) and Cargo.lock, commit, tag v<version>, push.
#   tools/release.sh 0.1.0-alpha.12
# The release workflow refuses a tag that does not match the manifest.
set -euo pipefail
v="${1:?usage: tools/release.sh <semver, e.g. 0.1.0-alpha.12>}"
v="${v#v}"
root="$(git rev-parse --show-toplevel)"
manifest="$root/engine/Cargo.toml"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "not semver: $v" >&2; exit 1; }
[ -z "$(git -C "$root" status --porcelain --untracked-files=no)" ] || { echo "working tree not clean" >&2; exit 1; }
git -C "$root" rev-parse -q --verify "refs/tags/v$v" >/dev/null && { echo "tag v$v exists" >&2; exit 1; }
sed -i.bak '/^\[workspace.package\]/,/^\[/s/^version = ".*"$/version = "'"$v"'"/' "$manifest" && rm -f "$manifest.bak"
grep -q "^version = \"$v\"\$" "$manifest" || { echo "failed to set the version in $manifest" >&2; exit 1; }
(cd "$root/engine" && cargo update --workspace)
git -C "$root" diff --quiet -- engine/Cargo.lock && { echo "Cargo.lock did not change" >&2; exit 1; }
git -C "$root" commit -qm "release: v$v" -- engine/Cargo.toml engine/Cargo.lock
git -C "$root" tag -a "v$v" -m "kerf v$v"
git -C "$root" push -q origin HEAD "v$v"
echo "released v$v (CI builds and publishes it)"
