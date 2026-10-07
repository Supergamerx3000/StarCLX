#!/bin/sh
# Baut das Flatpak von StarCLX aus einem .deb.
#
#   ./build.sh                    neuestes Release von GitHub verpacken
#   ./build.sh starclx_1.1.0_amd64.deb
#
# Ergebnis: starclx.flatpak hier im Ordner. Installieren mit
#   flatpak install --user ./starclx.flatpak
# Braucht flatpak, flatpak-builder und git.
set -eu
cd "$(dirname "$0")"

for tool in flatpak flatpak-builder git; do
  command -v "$tool" >/dev/null || { echo "$tool fehlt (z. B. sudo apt install flatpak flatpak-builder git)" >&2; exit 1; }
done

if [ $# -ge 1 ]; then
  cp "$1" starclx.deb
else
  url=$(curl -fsSL https://api.github.com/repos/crazmoe/StarCLX/releases/latest \
    | grep -o '"browser_download_url": *"[^"]*_amd64\.deb"' | cut -d'"' -f4 | head -n1)
  [ -n "$url" ] || { echo "Kein .deb im neuesten Release gefunden" >&2; exit 1; }
  echo "Lade $url"
  curl -fL -o starclx.deb "$url"
fi

[ -d shared-modules ] || git clone --depth 1 https://github.com/flathub/shared-modules.git

flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install-deps-from=flathub --force-clean --repo=repo build ch.crazmoe.StarCLX.yml
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
  repo starclx.flatpak ch.crazmoe.StarCLX
echo "Fertig: $(pwd)/starclx.flatpak"
