#!/bin/sh
# Installs Kinetto for the current user: binaries, icon and app-menu entry.
# Run from the repository after `cargo build --release`.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/target/release/kinetto" "$bin/kinetto"
install -m 755 "$here/target/release/kinetto-cli" "$bin/kinetto-cli"
install -m 644 "$here/assets/kinetto.svg" "$icons/kinetto.svg"
install -m 644 "$here/assets/kinetto.desktop" "$apps/kinetto.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" || true
echo "Installed. Kinetto is in your app menu; make sure $bin is on your PATH."
