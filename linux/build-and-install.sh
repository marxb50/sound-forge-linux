#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=== Sound Forge Linux — Instalador & Compilador ==="

install_deps() {
  if command -v apt-get >/dev/null 2>&1; then
    echo "[1/4] Instalando dependências (Debian/Ubuntu/Mint/Pop!_OS)..."
    sudo apt-get update
    sudo apt-get install -y \
      build-essential pkg-config \
      libasound2-dev libgtk-3-dev \
      libxkbcommon-dev libxcb-shape0-dev libxcb-xfixes0-dev \
      libssl-dev ffmpeg
  elif command -v dnf >/dev/null 2>&1; then
    echo "[1/4] Instalando dependências (Fedora/RHEL)..."
    sudo dnf install -y \
      gcc pkgconf-pkg-config \
      alsa-lib-devel gtk3-devel \
      libxkbcommon-devel libxcb-devel \
      openssl-devel ffmpeg
  elif command -v pacman >/dev/null 2>&1; then
    echo "[1/4] Instalando dependências (Arch/Manjaro)..."
    sudo pacman -S --needed --noconfirm \
      base-devel pkgconf alsa-lib gtk3 libxkbcommon libxcb openssl ffmpeg
  else
    echo "Gerenciador de pacotes não reconhecido. Certifique-se de ter: gcc, pkg-config, alsa-lib-dev, gtk3-dev, libxkbcommon-dev, ffmpeg."
  fi
}

install_rust() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "[2/4] Instalando Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck source=/dev/null
    source "${HOME}/.cargo/env"
  else
    echo "[2/4] Rust já encontrado: $(rustc --version)"
  fi
}

build_app() {
  echo "[3/4] Compilando Sound Forge Linux em modo release..."
  cd "${PROJECT_DIR}"
  cargo test --release
  cargo build --release
}

install_desktop() {
  echo "[4/4] Instalando binário e atalho no menu de aplicativos..."
  local bin_src="${PROJECT_DIR}/target/release/sound-forge-linux"
  local bin_dir="${HOME}/.local/bin"
  local app_dir="${HOME}/.local/share/applications"
  local icon_dir="${HOME}/.local/share/icons/hicolor/scalable/apps"

  mkdir -p "${bin_dir}" "${app_dir}" "${icon_dir}"
  cp "${bin_src}" "${bin_dir}/sound-forge-linux"
  chmod +x "${bin_dir}/sound-forge-linux"
  cp "${SCRIPT_DIR}/sound-forge-linux.svg" "${icon_dir}/sound-forge-linux.svg"

  sed "s|^Exec=sound-forge-linux|Exec=${bin_dir}/sound-forge-linux|" \
    "${SCRIPT_DIR}/sound-forge-linux.desktop" > "${app_dir}/sound-forge-linux.desktop"

  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${app_dir}" || true
  fi

  echo ""
  echo "Instalação concluída!"
  echo "  Binário: ${bin_dir}/sound-forge-linux"
  echo "  Menu:    Sound Forge Linux"
  if [[ ":${PATH}:" != *":${bin_dir}:"* ]]; then
    echo "  Dica: adicione ${bin_dir} ao PATH ou execute diretamente: ${bin_dir}/sound-forge-linux"
  fi
}

install_deps
install_rust
build_app
install_desktop
