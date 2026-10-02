# VERCEL THING!!! DONT CONFUSE WITH build-appimage.sh
#!/bin/bash
set -e

dnf install -y \
  gtk4-devel \
  libadwaita-devel \
  pkgconfig

export PKG_CONFIG_PATH=/usr/lib64/pkgconfig:/usr/share/pkgconfig

cargo build --release
