#!/usr/bin/env bash
# Fetch every distributable source pinned in sources/manifest.json into
# .sources/<name> at its pinned commit.
# Idempotent: a checkout already at the pinned commit is not fetched again.
# Verifies the commit and, when the manifest gives one, the license file hash.
# Entries marked "distributable": false are skipped with a message. Unreachable
# sources are reported and skipped; the exit code is non-zero if anything failed.
#
# Needs: bash, git, jq, shasum or sha256sum.
# Override the target directory with V1PER_SOURCES_DIR.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/sources/manifest.json"
dest="${V1PER_SOURCES_DIR:-$root/.sources}"
export GIT_TERMINAL_PROMPT=0

for tool in git jq; do
  command -v "$tool" >/dev/null || { echo "missing required tool: $tool" >&2; exit 2; }
done

if command -v sha256sum >/dev/null; then
  sha256() { sha256sum | cut -d' ' -f1; }
else
  sha256() { shasum -a 256 | cut -d' ' -f1; }
fi

failures=0
fail() { echo "  FAIL: $*" >&2; failures=$((failures + 1)); }

mkdir -p "$dest"

while IFS=$'\t' read -r name distributable; do
  if [ "$distributable" != true ]; then
    echo "== $name: not distributable, skipped (read it from its local checkout)"
    continue
  fi

  src() { jq -r --arg n "$name" ".sources[] | select(.name == \$n) | $1" "$manifest"; }
  url="$(src .url)"
  commit="$(src .commit)"
  dir="$dest/$name"
  echo "== $name @ ${commit:0:12}"

  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" != "$commit" ]; then
    if [ ! -d "$dir/.git" ]; then
      mkdir -p "$dir"
      git -C "$dir" init -q
      git -C "$dir" remote add origin "$url"
    fi
    if ! git -C "$dir" fetch -q --depth 1 origin "$commit" 2>/dev/null; then
      fail "cannot fetch $commit from $url"
      continue
    fi
    git -C "$dir" checkout -q --detach "$commit" || { fail "checkout of $commit failed"; continue; }
  fi

  if [ "$(git -C "$dir" rev-parse HEAD)" != "$commit" ]; then
    fail "HEAD is not $commit"
    continue
  fi

  lic="$(src '.license.file // empty')"
  lic_sha="$(src '.license.sha256 // empty')"
  if [ -n "$lic" ] && [ -n "$lic_sha" ]; then
    actual="$(git -C "$dir" show "$commit:$lic" 2>/dev/null | sha256)"
    [ "$actual" = "$lic_sha" ] || fail "$lic hash mismatch (expected $lic_sha, got $actual)"
  fi
done < <(jq -r '.sources[] | [.name, (.distributable == true)] | @tsv' "$manifest")

if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "all distributable sources verified in $dest"
