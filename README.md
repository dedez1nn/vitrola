# ytm-local

Backup local da sua biblioteca do YouTube Music, só áudio (m4a original, sem reencode).

## Fluxo

```bash
uv run ytm-local auth --from-browser firefox   # lê os cookies do Firefox (você precisa estar logado no music.youtube.com)
# alternativa manual: uv run ytm-local auth (cola os headers)
uv run ytm-local sync                 # curtidas, biblioteca, playlists e histórico -> SQLite
uv run ytm-local status
uv run ytm-local download --limit 5 --cookies-browser firefox   # teste pequeno
uv run ytm-local download --cookies-browser firefox             # tudo que está pendente
```

Estado em `~/.local/share/ytm-local/state.db`, músicas em `~/Music/ytm-local`
(mude com `YTM_LOCAL_DATA` e `YTM_LOCAL_MUSIC`). É retomável: faixas `done` não são rebaixadas.
`--retry-failed` tenta de novo as que falharam.

## Limitações conhecidas

- Pela URL `watch?v=` o título pode vir como a versão em vídeo ("Official Video").
  Próximo passo: gravar os metadados do `ytmusicapi` (artista, álbum, faixa) nas tags.
- Qualidade máxima (~256 kbps) exige cookies de uma conta com YouTube Music Premium.
- Uso pessoal: baixar viola os termos do YouTube.

## Próximos passos

- Tags a partir do banco (mutagen) e pasta `Artista/Álbum/NN Faixa`.
- `sync` periódico via systemd timer.
- Servir com Navidrome.

## Recomendação e descoberta

```bash
uv run ytm-local recommend              # o que ouvir agora (só faixas já baixadas)
uv run ytm-local recommend -n 20 --mode again --play   # o que você anda ouvindo, tocando no mpv
uv run ytm-local discover               # músicas novas, a partir da rádio das suas favoritas
uv run ytm-local discover --new-artists --add 10        # só artistas novos; 10 vão para a fila de download
```

`recommend` pontua por afinidade com o artista, fonte (curtida > playlist > biblioteca), "favorita esquecida"
(fora do histórico), repetição recente (o que o `--play` já tocou é penalizado) e diversidade de artistas, com um
pouco de aleatoriedade (`--temperature`). `discover` sorteia sementes entre as favoritas, pega a rádio do YouTube
Music de cada uma e ordena pelo que aparece em mais rádios; o que você já tem fica de fora.
Rode `sync history` para o `recommend` usar o histórico.

## Toca (app desktop)

Interface "duas estantes" (disco x nuvem) em Rust + GTK4, pensada para gastar pouca RAM.

```bash
cd toca && cargo build --release
./target/release/toca
```

- **Ouvir agora**: 5 sugestões do `ytm-local recommend --scope all --json`. O seletor Tudo / No disco / Na nuvem filtra.
- **No disco** toca o arquivo local. **Na nuvem** toca por streaming (mpv + yt-dlp, com os cookies do Firefox) e tem o botão **Baixar** (`ytm-local download --id`).
- **Mesmo estilo** (padrão): ao tocar um cartão, a fila é a rádio do YouTube Music daquela faixa (`ytm-local similar`), com o que já está na sua biblioteca na frente. O YT Music não expõe gênero por faixa, então "estilo" é a recomendação dele. O botão fica em Agora tocando e dá para desligar.
- **Capas**: não vão para o disco. São buscadas só quando há internet, ficam em memória (até ~300) e, sem rede, aparece um bloco neutro.
- **RAM**: renderizador por software (`GSK_RENDERER=cairo`, use `TOCA_GPU=1` para o padrão do GTK), listas virtualizadas, um processo `mpv` por faixa que só existe enquanto toca, interface orientada a eventos (sem polling). Medido: ~40 MB de memória própria.
- Variáveis: `TOCA_YTM` (caminho do `ytm-local`), `YTM_LOCAL_DATA` (onde fica o banco).
