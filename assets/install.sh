#!/bin/sh
# Installs Kinetto for the current user: binaries, icon and app-menu entry.
# Run from the repository after `cargo build --release`.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
profile="${1:-release}"
case "$profile" in release|debug) ;; *) echo "Usage: $0 [release|debug]" >&2; exit 2 ;; esac
bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/target/$profile/kinetto" "$bin/kinetto"
if [ -f "$here/target/$profile/kinetto-cli" ]; then
    install -m 755 "$here/target/$profile/kinetto-cli" "$bin/kinetto-cli"
fi
install -m 644 "$here/assets/kinetto.svg" "$icons/kinetto.svg"
# Use an absolute executable: desktop sessions may not include ~/.local/bin.
sed "s|^Exec=.*|Exec=\"$bin/kinetto\" %f|" "$here/assets/kinetto.desktop" > "$apps/kinetto.desktop"
mime="$HOME/.local/share/mime"
mkdir -p "$mime/packages"
install -m 644 "$here/assets/kinetto-mime.xml" "$mime/packages/kinetto.xml"
update-mime-database "$mime"
command -v xdg-mime >/dev/null && xdg-mime default kinetto.desktop application/x-kinetto
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" || true
echo "Installed. Kinetto is in your app menu; make sure $bin is on your PATH."
