# VERCEL THING!!!
#!/bin/bash
set -e

# Install GTK and related dev libraries
dnf install -y \
  gtk4-devel \
  libadwaita-devel \
  pkgconfig

# Ensure pkg-config can find them (usually automatic, but just in case)
export PKG_CONFIG_PATH=/usr/lib64/pkgconfig:/usr/share/pkgconfig

# Now run the actual cargo build
cargo build --release
