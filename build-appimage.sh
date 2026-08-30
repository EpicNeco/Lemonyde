#!/usr/bin/env bash
# Builds Lemonyde and packages it as a single portable .AppImage that your
# friends can download and just double-click / `chmod +x && ./run` — no
# Rust, GTK dev headers, or `cargo build` needed on their end.
#
# Run this ONCE on your own machine (with the build deps from install.sh
# already installed). It downloads linuxdeploy + appimagetool from their
# official GitHub releases the first time you run it, then reuses them.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLS_DIR="${ROOT}/.appimage-tools"
APPDIR="${ROOT}/AppDir"
ARCH="$(uname -m)"

c_green() { printf '\033[1;32m%s\033[0m\n' "$1"; }
c_yellow() { printf '\033[1;33m%s\033[0m\n' "$1"; }
c_red() { printf '\033[1;31m%s\033[0m\n' "$1"; }

echo "🍋 Packaging Lemonyde as an AppImage (${ARCH})"
echo "------------------------------------------------"

command -v cargo >/dev/null 2>&1 || { c_red "cargo not found — install Rust first (rustup.rs)."; exit 1; }
pkg-config --exists gtk4 || { c_red "GTK4 dev headers not found — run ./install.sh's dependency step first."; exit 1; }

# 1. Build the release binary
echo "Building release binary…"
(cd "${ROOT}" && env -u RUSTFLAGS -u CARGO_BUILD_RUSTFLAGS cargo build --release)

# 2. Fetch packaging tools (once)
mkdir -p "${TOOLS_DIR}"
LINUXDEPLOY="${TOOLS_DIR}/linuxdeploy-${ARCH}.AppImage"
GTK_PLUGIN="${TOOLS_DIR}/linuxdeploy-plugin-gtk.sh"
APPIMAGETOOL="${TOOLS_DIR}/appimagetool-${ARCH}.AppImage"

fetch() {
  local url="$1" dest="$2"
  if [ ! -f "${dest}" ]; then
    c_yellow "Downloading $(basename "${dest}")…"
    curl -fL -o "${dest}" "${url}"
    chmod +x "${dest}"
  fi
}

fetch "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${ARCH}.AppImage" "${LINUXDEPLOY}"
fetch "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh" "${GTK_PLUGIN}"
fetch "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage" "${APPIMAGETOOL}"

# 3. Lay out the AppDir
echo "Assembling AppDir…"
rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin" "${APPDIR}/usr/share/applications" "${APPDIR}/usr/share/icons/hicolor/scalable/apps"

cp "${ROOT}/target/release/lemonyde" "${APPDIR}/usr/bin/lemonyde"
cp "${ROOT}/style.css" "${APPDIR}/usr/bin/style.css"
mkdir -p "${APPDIR}/usr/bin/assets"
cp -r "${ROOT}/assets/." "${APPDIR}/usr/bin/assets/"
cp "${ROOT}/assets/lemonyde.svg" "${APPDIR}/usr/share/icons/hicolor/scalable/apps/lemonyde.svg"
cp "${ROOT}/assets/lemonyde.svg" "${APPDIR}/lemonyde.svg"

sed 's|Exec=lemonyde|Exec=lemonyde|' "${ROOT}/lemonyde.desktop" > "${APPDIR}/usr/share/applications/lemonyde.desktop"
cp "${APPDIR}/usr/share/applications/lemonyde.desktop" "${APPDIR}/lemonyde.desktop"

cat > "${APPDIR}/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "${HERE}/usr/bin/lemonyde" "$@"
EOF
chmod +x "${APPDIR}/AppRun"

# 4. Bundle shared libraries (GTK4, libadwaita, glib, etc.) + GTK runtime bits
echo "Bundling shared libraries (this is the slow part)…"
export LDAI_OUTPUT="Lemonyde-${ARCH}.AppImage"
export DEPLOY_GTK_VERSION=4
"${LINUXDEPLOY}" \
  --appdir "${APPDIR}" \
  --executable "${APPDIR}/usr/bin/lemonyde" \
  --desktop-file "${APPDIR}/usr/share/applications/lemonyde.desktop" \
  --icon-file "${APPDIR}/lemonyde.svg" \
  --plugin gtk \
  --output appimage \
  PATH="${TOOLS_DIR}:${PATH}"

mv Lemonyde-*.AppImage "${ROOT}/Lemonyde-${ARCH}.AppImage" 2>/dev/null || true

c_green "Done!"
echo
echo "Share this one file with your friends:"
echo "  ${ROOT}/Lemonyde-${ARCH}.AppImage"
echo
echo "They just need to:"
echo "  chmod +x Lemonyde-${ARCH}.AppImage"
echo "  ./Lemonyde-${ARCH}.AppImage"
echo
c_yellow "Note: AppImages built on an older distro tend to run on newer ones fine" 
c_yellow "(thanks to glibc backward compatibility), but not always the reverse. If you" 
c_yellow "can, build this on something like Ubuntu 22.04/24.04 rather than a rolling release."
