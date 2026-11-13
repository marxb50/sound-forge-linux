# Sound Forge Linux (Pro 10.0 Clean-Room Edition)

Editor de áudio nativo em **Rust** (`eframe`/`egui` + `symphonia` + `cpal` + `hound`) fiel ao visual, layout MDI e fluxo de trabalho do **Sony Sound Forge Pro 10.0**, criado do zero (clean-room) para rodar nativamente no **Linux** (e Windows).

## Instalação / Atualização Rápida no Linux

Se você ainda não clonou:

```bash
git clone https://github.com/marxb50/sound-forge-linux.git
cd sound-forge-linux
chmod +x linux/build-and-install.sh
./linux/build-and-install.sh
```

Se já clonou anteriormente, basta atualizar e recompilar:

```bash
cd sound-forge-linux
git pull
./linux/build-and-install.sh
```

---

## Como Funciona Trabalhar com Várias Músicas (Bancada MDI)

Assim como no **Sound Forge Pro 10.0**, cada música aberta é uma **janela de dados independente** dentro da bancada:

1. **Empilhadas (`▤ Empilhar / Tile Horizontally` — Padrão):**
   - Abra 2 ou mais músicas (`Ctrl+O` ou arraste-as para a janela). Elas ficam visíveis simultaneamente uma sobre a outra.
   - Cada janela possui sua própria barra de título, minimapa de overview, régua de tempo, botões de canal `[1] [-]` / `[2] [-]` (com linha `-Inf.`), marcadores laranjas numerados (`1`, `2`, ...) e sua própria barra inferior de transporte (`|◀`, `◀◀`, `■`, `▶`, `🔁`, controle `Rate: 0,00` e 3 visores de seleção/zoom).
2. **Transferir Trechos Entre Músicas:**
   - **Via Copiar/Colar:** Selecione um trecho na Música 1 (por exemplo, dando duplo clique entre o marcador `1` e o marcador `2`), pressione `Ctrl+C`, clique no ponto desejado da Música 2 e pressione `Ctrl+V` (inserir) ou `Ctrl+M` (mixar).
   - **Botão Direto `📥 Enviar Trecho p/ Outra Música`:** Aparece no topo da janela sempre que há uma seleção e 2+ músicas abertas; permite inserir no cursor da outra música, juntar no final com **Crossfade** ou mixar.
   - **Juntar Todas as Músicas Abertas (`🔗 Juntar Músicas` / `Tools -> Join / Merge Open Songs...`):** Concatena todas as músicas abertas (ou as seleções de cada uma) em uma nova faixa com crossfade suave ou pausa configurável.

---

## Menus e Diálogos Fiéis ao Sound Forge Pro 10.0

- **Tema Clássico Sound Forge 10:** Cromado claro (`#F0F0F0`), bancada cinza MDI (`#ABABAB`), fundo de onda branco (`#FFFFFF`) e onda **Azul-Marinho (`#001478`)** (com opção de alternar para *Dark Studio* em `Options`).
- **Diálogo `Volume` (`Process -> Volume...`):**
  - Fader vertical de `-Inf. a 20 dB` com leitura simultânea em `dB` e `%` (ex.: `4,28 dB (163,68 %)`), Presets, `Preview`, `Bypass` e caixas de tempo inferiores.
- **Diálogo `Sony Time Stretch` (`Process -> Time -> Time Stretch...`):**
  - Banner clássico `SONY | Time Stretch`, seletor de modos (`A01. Music 1 (minimum flange, may echo)` até `A05. Solo Instruments` e `V01. Varispeed`), slider de `Final Time`, `Initial Time`, `Percent of original` (ex.: `89,85 %`), calculadora por `BPM`, `Preview` e `Bypass`.
- **Menu `Process` Completo:**
  - `Auto Trim/Crop...`, `Bit Depth`, `Channel Converter...`, `DC Offset...`, `EQ` (3 bandas), `Fade` (`Graphic...`, `In`, `Out`), `Invert/Flip`, `Mute`, `Normalize...` (Pico e RMS), `Pan/Expand...`, `Resample` (`22050` a `96000 Hz`), `Reverse`, `Rotate Audio`, `Smooth/Enhance...`, `Time Stretch...`, `Extend Musical Phrase (Crossfade)...` e `Volume...`.
- **Menu `Effects` & Ferramentas (`┼ Edit`, `🔍 Zoom`, `✏ Lápis`):**
  - `Pitch Shift` (semitons e cents mantendo a duração), `Delay/Echo`, `Reverb`, além de ferramenta Lápis para desenhar diretamente nas amostras em alto zoom.
