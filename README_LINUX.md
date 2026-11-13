# Sound Forge Linux (Clean-Room Rust Edition)

Editor de áudio nativo em **Rust** (`eframe`/`egui` + `symphonia` + `cpal` + `hound`) inspirado no **Sony Sound Forge Pro 10.0**, criado do zero (clean-room) para rodar nativamente no **Linux** (e também compilável no Windows).

## Recursos Implementados

- **Visualizador de Waveform Profissional:**
  - Barra superior de **Overview (minimapa)** com clique/arraste para navegar rapidamente.
  - Canais Estéreo/Mono com régua de tempo adaptativa e escala vertical em dB (`0`, `-6`, `-12`, `-∞`).
  - Zoom fluido no ponteiro do mouse (`Scroll`), navegação lateral (`Shift + Scroll` ou botão do meio) e atalhos (`Ctrl+0` para mostrar tudo).
  - **Snap Zero-Crossing (`Z`)**: ajusta cortes/seleções para cruzamento por zero, evitando estalos.
- **Edição Não-Destrutiva com Desfazer/Refazer (`Ctrl+Z` / `Ctrl+Y`):**
  - Recortar (`Ctrl+X`), Copiar (`Ctrl+C`), Colar Inserindo (`Ctrl+V`), Colar Mixando, Excluir (`Del`), Aparar/Trim (`Ctrl+T`).
  - **Marcadores (`M`)** e **Regiões (`R`)** com painel lateral, duplo clique para selecionar entre marcadores e preservação automática de posições ao editar.
  - **Estender Trecho com Crossfade**: alonga uma frase musical em `+1.5s`, `+2.5s`, `+3.5s` ou valor personalizado com crossfade suave.
- **Acelerar / Ralentar Músicas & Pitch:**
  - **Controle ao vivo no Transporte**: deslize de `0.50x` a `2.00x` (ou presets `0.75x`, `0.85x`, `1.0x`, `1.15x`, `1.25x`) e clique em **Aplicar ao Áudio** para gravar no trecho ou arquivo inteiro.
  - **Preservar Tom (WSOLA Time-Stretch)**: acelera ou desacelera sem alterar a afinação (ideal para música, ensaios e dança).
  - **Varispeed**: altera velocidade e tom juntos (estilo fita/vinil).
  - **Calculadora por BPM** e **Pitch Shift** (`-12` a `+12` semitons sem alterar duração).
- **Processamento & Exportação:**
  - Ganho em dB, Normalização de Pico, Fade In / Fade Out (Linear, S-Curve, Exponencial), Inverter (Reverse), Silenciar, Inserir Silêncio.
  - Medidor **VU Estéreo** em tempo real (`-60 dB` a `0 dB`).
  - Abre **MP3, WAV, FLAC, OGG, M4A/AAC** nativamente via `symphonia` (com fallback FFmpeg).
  - Salva **WAV 16/24/32-bit** nativamente e exporta **MP3 (320/192 kbps), FLAC, OGG, M4A** via `ffmpeg`.

---

## Como Instalar no Linux (1 Comando)

Copie a pasta `sound-forge-linux` para a sua máquina Linux e execute:

```bash
cd sound-forge-linux
chmod +x linux/build-and-install.sh
./linux/build-and-install.sh
```

O script instala automaticamente as dependências (`alsa`, `gtk3`, `libxkbcommon`, `ffmpeg`), compila em `--release`, instala o binário em `~/.local/bin/sound-forge-linux` e cria o atalho **Sound Forge Linux** no seu menu de aplicativos.

> **Dica:** Se você copiou os arquivos do Windows e o `.sh` tiver quebras de linha CRLF, rode antes: `sed -i 's/\r$//' linux/build-and-install.sh`

### Dependências Manuais (Ubuntu / Debian / Mint)

```bash
sudo apt install build-essential pkg-config libasound2-dev libgtk-3-dev libxkbcommon-dev libxcb-shape0-dev libxcb-xfixes0-dev ffmpeg
cargo build --release
```
