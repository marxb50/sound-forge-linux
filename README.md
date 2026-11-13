# Sound Forge Linux (Clean-Room Rust Edition)

Editor de áudio nativo em **Rust** (`eframe`/`egui` + `symphonia` + `cpal` + `hound`) inspirado no **Sony Sound Forge Pro 10.0**, criado do zero (clean-room) para rodar nativamente no **Linux**.

## Instalação Rápida no Linux

Na sua máquina Linux, basta clonar e executar o instalador:

```bash
git clone https://github.com/marxb50/sound-forge-linux.git
cd sound-forge-linux
chmod +x linux/build-and-install.sh
./linux/build-and-install.sh
```

O script instala as dependências do sistema (`alsa`, `gtk3`, `libxkbcommon`, `ffmpeg`), instala o Rust (caso ainda não tenha), compila em modo `--release`, instala o binário em `~/.local/bin/sound-forge-linux` e adiciona o atalho **Sound Forge Linux** no menu de aplicativos.

---

## Recursos

- **Waveform Estéreo/Mono Profissional:** Barra de Overview (minimapa), escala de tempo e dB, zoom no cursor (`Scroll`), pan (`Shift + Scroll` ou botão do meio) e **Snap Zero-Crossing (`Z`)**.
- **Edição Não-Destrutiva (`Ctrl+Z` / `Ctrl+Y`):** Recortar, Copiar, Colar, Colar Mixando, Excluir, Aparar/Trim (`Ctrl+T`), **Estender Trecho com Crossfade** (`+1.5s`, `+2.5s`, `+3.5s` ou customizado).
- **Marcadores (`M`) & Regiões (`R`):** Painel lateral dedicado, duplo clique para selecionar entre marcadores e acompanhamento automático em edições.
- **Acelerar e Ralentar Músicas & Pitch:**
  - Controle ao vivo (`0.50x` a `2.00x`) na barra de Transporte com opção de **Preservar Tom (WSOLA)** ou **Varispeed**.
  - Calculadora por **BPM** e **Pitch Shift** (`-12` a `+12` semitons mantendo a duração).
- **Exportação & Reprodução:** Abre **MP3, WAV, FLAC, OGG, M4A** nativamente via `symphonia`; salva **WAV 16/24/32-bit** nativamente e exporta **MP3 (320/192 kbps), FLAC, OGG, M4A** via `ffmpeg`.
