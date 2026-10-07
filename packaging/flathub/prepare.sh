#!/bin/sh
# Bereitet den Flathub-Bau vor.
#
#   ./prepare.sh                     Quelllisten erzeugen (für flatpak-builder hier)
#   ./prepare.sh --submission v1.2.0 zusätzlich submission/ für das Flathub-Repo
#                                    (statt eines Tags geht auch ein Commit)
#
# Erzeugt cargo-sources.json und node-sources.json aus Cargo.lock und
# package-lock.json (Flathub baut ohne Netz) und holt die shared-modules.
# Braucht git, python3 und Netz.
set -eu
cd "$(dirname "$0")"
root=$(git rev-parse --show-toplevel)

tag=""
if [ "${1:-}" = "--submission" ]; then
  tag=${2:?Tag fehlt, z. B. --submission v1.2.0}
fi

[ -d shared-modules ] || git clone --depth 1 https://github.com/flathub/shared-modules.git
tools=.tools
if [ ! -d "$tools/flatpak-builder-tools" ]; then
  mkdir -p "$tools"
  git clone --depth 1 https://github.com/flatpak/flatpak-builder-tools.git "$tools/flatpak-builder-tools"
fi
if [ ! -x "$tools/venv/bin/python" ]; then
  python3 -m venv "$tools/venv"
  "$tools/venv/bin/pip" install -q aiohttp tomlkit "$tools/flatpak-builder-tools/node"
fi

echo "Erzeuge cargo-sources.json"
"$tools/venv/bin/python" "$tools/flatpak-builder-tools/cargo/flatpak-cargo-generator.py" \
  "$root/Cargo.lock" -o cargo-sources.json
echo "Erzeuge node-sources.json"
"$tools/venv/bin/flatpak-node-generator" npm "$root/apps/desktop/package-lock.json" \
  -o node-sources.json >/dev/null

[ -n "$tag" ] || exit 0

commit=$(git rev-parse "$tag^{commit}")
git show-ref --verify --quiet "refs/tags/$tag" || tag=""
rm -rf submission
mkdir submission
cp cargo-sources.json node-sources.json submission/
# Quellcode aus dem Git-Tag statt aus dem Arbeitsverzeichnis
awk -v tag="$tag" -v commit="$commit" '
  /# BEGIN-SOURCE/ {
    sub(/# BEGIN-SOURCE.*/, "")
    indent = $0
    print indent "- type: git"
    print indent "  url: https://github.com/crazmoe/StarCLX.git"
    if (tag != "") print indent "  tag: " tag
    print indent "  commit: " commit
    skip = 1
    next
  }
  /# END-SOURCE/ { skip = 0; next }
  !skip
' io.github.crazmoe.StarCLX.yml > submission/io.github.crazmoe.StarCLX.yml
cat > submission/flathub.json <<JSON
{
  "only-arches": ["x86_64"]
}
JSON
echo "Fertig: $(pwd)/submission (dazu shared-modules als Git-Submodul im Flathub-Repo)"
