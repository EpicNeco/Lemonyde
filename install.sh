#!/usr/bin/env bash
# Lemonyde bootstrapper installer (Rust edition).
# Builds Lemonyde from source, makes sure Flatpak + Flathub are set up,
# and installs the app + its lemon logo as your icon theme's app icon.
# Never runs sudo without telling you first.

set -euo pipefail

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="${HOME}/.local/share/lemonyde"
BIN_DIR="${HOME}/.local/bin"
DESKTOP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

c_green() { printf '\033[1;32m%s\033[0m\n' "$1"; }
c_yellow() { printf '\033[1;33m%s\033[0m\n' "$1"; }
c_red() { printf '\033[1;31m%s\033[0m\n' "$1"; }

echo "🍋 Lemonyde bootstrapper (Rust)"
echo "--------------------------------"

# 1. Rust toolchain
if ! command -v cargo >/dev/null 2>&1; then
  c_red "cargo/rustc not found. Install Rust first: https://rustup.rs"
  exit 1
fi

# 2. GTK4 / libadwaita dev headers (needed to build)
missing_pkgs=()
pkg-config --exists gtk4 2>/dev/null || missing_pkgs+=("gtk4")
pkg-config --exists libadwaita-1 2>/dev/null || missing_pkgs+=("libadwaita-1")

if [ "${#missing_pkgs[@]}" -gt 0 ]; then
  c_yellow "Missing dev packages: ${missing_pkgs[*]}"
  echo "Install them for your distro, then re-run this script:"
  echo
  echo "  Debian/Ubuntu:  sudo apt install libgtk-4-dev libadwaita-1-dev build-essential"
  echo "  Fedora:         sudo dnf install gtk4-devel libadwaita-devel"
  echo "  Arch:           sudo pacman -S --needed gtk4 libadwaita base-devel"
  echo
  read -rp "Try to install these automatically now? [y/N] " yn
  if [[ "${yn:-N}" =~ ^[Yy]$ ]]; then
    if command -v apt >/dev/null 2>&1; then
      sudo apt update && sudo apt install -y libgtk-4-dev libadwaita-1-dev build-essential
    elif command -v dnf >/dev/null 2>&1; then
      sudo dnf install -y gtk4-devel libadwaita-devel
    elif command -v pacman >/dev/null 2>&1; then
      sudo pacman -S --needed gtk4 libadwaita base-devel
    else
      c_red "Unrecognized package manager — please install the packages manually."
      exit 1
    fi
  else
    exit 1
  fi
fi

# 3. Flatpak + Flathub (needed to install/run Sober itself)
if ! command -v flatpak >/dev/null 2>&1; then
  c_yellow "Flatpak isn't installed. Lemonyde can still open, but it can't install/launch Sober."
  echo "See https://flatpak.org/setup/ for instructions for your distro."
else
  if ! flatpak remote-list | grep -q flathub; then
    c_yellow "Adding the Flathub remote (needed to install Sober)…"
    flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
  fi
fi

# 4. Build
echo "Building Lemonyde (release, this can take a couple of minutes)…"
(cd "${SRC_DIR}" && cargo build --release)

# 5. Install files
echo "Installing Lemonyde to ${INSTALL_DIR}"
mkdir -p "${INSTALL_DIR}/assets" "${BIN_DIR}" "${DESKTOP_DIR}" "${ICON_DIR}"
cp "${SRC_DIR}/target/release/lemonyde" "${INSTALL_DIR}/lemonyde-bin"
cp "${SRC_DIR}/style.css" "${INSTALL_DIR}/style.css"
cp -r "${SRC_DIR}/assets/." "${INSTALL_DIR}/assets/"
cp "${SRC_DIR}/assets/lemonyde.svg" "${ICON_DIR}/lemonyde.svg"

cat > "${BIN_DIR}/lemonyde" <<EOF
#!/usr/bin/env bash
exec "${INSTALL_DIR}/lemonyde-bin" "\$@"
EOF
chmod +x "${BIN_DIR}/lemonyde"

sed "s|Exec=lemonyde|Exec=${BIN_DIR}/lemonyde|; s|Icon=lemonyde|Icon=${ICON_DIR}/lemonyde.svg|" \
  "${SRC_DIR}/lemonyde.desktop" > "${DESKTOP_DIR}/lemonyde.desktop"
chmod +x "${DESKTOP_DIR}/lemonyde.desktop"
update-desktop-database "${DESKTOP_DIR}" >/dev/null 2>&1 || true
gtk-update-icon-cache >/dev/null 2>&1 || true

c_green "Done!"
echo
if command -v flatpak >/dev/null 2>&1 && ! flatpak info org.vinegarhq.Sober >/dev/null 2>&1; then
  read -rp "Sober isn't installed yet — install it now via Flathub? [y/N] " yn
  if [[ "${yn:-N}" =~ ^[Yy]$ ]]; then
    flatpak install --user -y flathub org.vinegarhq.Sober
  fi
fi

if [[ ":$PATH:" != *":${BIN_DIR}:"* ]]; then
  c_yellow "Note: ${BIN_DIR} isn't on your PATH yet. Add this to your shell rc file:"
  echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo "Launch Lemonyde with: lemonyde"
echo "…or find it in your app menu."
