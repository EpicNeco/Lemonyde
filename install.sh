#!/usr/bin/env bash
# Lemonyde bootstrapper installer (Rust edition).
# Builds Lemonyde from source, makes sure Flatpak + Flathub are set up,
# and installs the app + its lemon logo as your icon theme's app icon.
# Never runs sudo without telling you first.
#
# Works two ways:
#   1. Cloned locally:  git clone ... && cd Lemonyde && ./install.sh
#   2. Piped via curl:  curl -fsSL <raw-url-to-this-file> | bash
# In case 2 there's no local checkout sitting next to this script, so it
# fetches the source itself first (git clone if available, otherwise a
# plain curl + tar of the repo tarball) into a cache directory, then
# proceeds exactly as case 1 would from there.

set -euo pipefail

REPO_URL="https://github.com/EpicNeco/Lemonyde"
REPO_BRANCH="main"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/lemonyde-src"

c_green() { printf '\033[1;32m%s\033[0m\n' "$1"; }
c_yellow() { printf '\033[1;33m%s\033[0m\n' "$1"; }
c_red() { printf '\033[1;31m%s\033[0m\n' "$1"; }

# Prompts for a y/N answer, reading from the real terminal (/dev/tty)
# instead of stdin. When this script is piped from curl, stdin *is* the
# pipe, so a plain `read` would silently get EOF instead of prompting. If
# there's no terminal at all (e.g. CI), this defaults to "no" rather than
# failing.
ask_yn() {
  local answer=""
  if [ -r /dev/tty ]; then
    read -rp "$1 [y/N] " answer < /dev/tty || answer=""
  fi
  [[ "${answer:-N}" =~ ^[Yy]$ ]]
}

echo "🍋 Lemonyde bootstrapper (Rust)"
echo "--------------------------------"

# 0. Locate the source. If this script is sitting next to an actual
# checkout (Cargo.toml in the same directory), use that directly. If not
# -- e.g. it was piped straight from curl, so there's no sibling source
# tree at all -- fetch the repo into a cache directory instead.
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || true)"
if [ -n "${SELF_DIR}" ] && [ -f "${SELF_DIR}/Cargo.toml" ]; then
  SRC_DIR="${SELF_DIR}"
else
  echo "Running via curl — fetching the source into ${CACHE_DIR}…"
  if command -v git >/dev/null 2>&1; then
    if [ -d "${CACHE_DIR}/.git" ]; then
      (cd "${CACHE_DIR}" && git fetch --depth 1 origin "${REPO_BRANCH}" && git reset --hard "origin/${REPO_BRANCH}")
    else
      rm -rf "${CACHE_DIR}"
      git clone --depth 1 --branch "${REPO_BRANCH}" "${REPO_URL}.git" "${CACHE_DIR}"
    fi
  elif command -v curl >/dev/null 2>&1 && command -v tar >/dev/null 2>&1; then
    c_yellow "git not found — falling back to a plain tarball download (no update support later)."
    rm -rf "${CACHE_DIR}"
    mkdir -p "${CACHE_DIR}"
    curl -fsSL "${REPO_URL}/archive/refs/heads/${REPO_BRANCH}.tar.gz" \
      | tar xz -C "${CACHE_DIR}" --strip-components=1
  else
    c_red "Need either git, or curl+tar, to fetch the source. Install one and re-run."
    exit 1
  fi
  SRC_DIR="${CACHE_DIR}"
fi

INSTALL_DIR="${HOME}/.local/share/lemonyde"
BIN_DIR="${HOME}/.local/bin"
DESKTOP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

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
  if ask_yn "Try to install these automatically now?"; then
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
(cd "${SRC_DIR}" && env -u RUSTFLAGS -u CARGO_BUILD_RUSTFLAGS cargo build --release)

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
  if ask_yn "Sober isn't installed yet — install it now via Flathub?"; then
    flatpak install --user -y flathub org.vinegarhq.Sober
  fi
fi

if [[ ":$PATH:" != *":${BIN_DIR}:"* ]]; then
  c_yellow "Note: ${BIN_DIR} isn't on your PATH yet. Add this to your shell rc file:"
  echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo "Launch Lemonyde with: lemonyde"
echo "…or find it in your app menu."
