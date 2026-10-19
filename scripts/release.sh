#!/usr/bin/env bash
# Cuts a release from a clean main: sets today's CalVer version (YY.M.D) in Cargo.toml and
# apps/web/package.json, regenerates the protocol contract, writes a CHANGELOG.md section
# from the gitmoji commits since the last tag, commits, tags vYY.M.D-<short commit> and
# pushes. The tag starts .github/workflows/release.yml.
#
#   scripts/release.sh                    # release
#   scripts/release.sh highlights.md      # put a hand-written summary above the commit list
#   scripts/release.sh --dry-run          # print the section, change nothing
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

dry_run=false
highlights=""
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=true ;;
    *) highlights="$arg" ;;
  esac
done
if [ -n "$highlights" ] && [ ! -f "$highlights" ]; then
  echo "no such highlights file: $highlights" >&2
  exit 1
fi

version=$(date +%y.%-m.%-d)
last_tag=$(git describe --tags --abbrev=0 --match 'v*' 2>/dev/null || true)
subjects=$(git log --no-merges --reverse --format='%s' ${last_tag:+"$last_tag"..HEAD} | grep -v '^🔖' || true)
if [ -z "$subjects" ]; then
  echo "nothing to release since ${last_tag:-the first commit}" >&2
  exit 1
fi

# Prints a changelog group: the subjects whose gitmoji matches the pattern.
group() {
  local lines
  lines=$(printf '%s\n' "$subjects" | grep -E "^($2)" || true)
  if [ -n "$lines" ]; then
    printf '### %s\n\n%s\n\n' "$1" "$(printf '%s\n' "$lines" | sed 's/^/- /')"
  fi
}

features='✨|🎉'
fixes='🐛|🚑'
performance='⚡'
security='🔒|🔐'
interface='💄|♿|🌐|💬'
section=$(
  printf '## %s\n\n' "$version"
  if [ -n "$highlights" ]; then printf '%s\n\n' "$(cat "$highlights")"; fi
  group Features "$features"
  group Fixes "$fixes"
  group Performance "$performance"
  group Security "$security"
  group Interface "$interface"
  others=$(printf '%s\n' "$subjects" | grep -vE "^($features|$fixes|$performance|$security|$interface)" || true)
  if [ -n "$others" ]; then
    printf '### Other changes\n\n%s\n' "$(printf '%s\n' "$others" | sed 's/^/- /')"
  fi
)

if $dry_run; then
  printf '%s\n' "$section"
  exit 0
fi

if [ -n "$(git status --porcelain)" ]; then
  echo "the working tree has uncommitted changes" >&2
  exit 1
fi
if [ "$(git branch --show-current)" != main ]; then
  echo "releases are cut from main" >&2
  exit 1
fi

perl -0pi -e "s/(\[workspace\.package\]\nversion = )\"[^\"]*\"/\${1}\"$version\"/" Cargo.toml
perl -pi -e "s/^  \"version\": \"[^\"]*\"/  \"version\": \"$version\"/" apps/web/package.json
cargo update --workspace --quiet
# The OpenAPI document carries the version.
cargo run --quiet -p v1per-protocol --bin export-types

# The new section goes above the newest one, below the file's introduction.
printf '%s\n\n' "$section" > .release-section.md
awk 'FNR == NR { section = section $0 "\n"; next }
     !done && /^## / { printf "%s", section; done = 1 }
     { print }
     END { if (!done) printf "\n%s", section }' .release-section.md CHANGELOG.md > CHANGELOG.md.next
mv CHANGELOG.md.next CHANGELOG.md
rm .release-section.md

git add Cargo.toml Cargo.lock apps/web/package.json CHANGELOG.md crates/protocol/openapi.json apps/web/src/generated
git commit --quiet -m "🔖 release $version"
tag="v$version-$(git rev-parse --short HEAD)"
git tag -a "$tag" -m "$tag"
git push --atomic origin HEAD:main "$tag"
echo "released $tag"
