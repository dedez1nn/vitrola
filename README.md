# Vitrola

Player de música local para a sua biblioteca do YouTube Music. Toca do disco ou por streaming, sugere o que ouvir
agora e aprende com o que você de fato escuta, sem playlists: a fila é montada na hora, a partir de uma faixa de
partida e do seu histórico.

O app é escrito em Rust + GTK4 (pasta `toca/`, executável `vitrola`) e conversa com o `ytm-local`, a ferramenta de
linha de comando que sincroniza a conta, baixa o áudio (m4a original, sem reencode) e calcula as recomendações.

## Vitrola (o app)

```bash
cd toca && cargo build --release
./target/release/vitrola
```

### Ouvir agora

Na tela inicial ficam 5 sugestões (`ytm-local recommend --scope all --json`). O seletor **Tudo / No disco / Na nuvem**
filtra a origem. Os botões de seta ao lado percorrem as 30 sugestões já buscadas, de 5 em 5, e pedem um lote novo
quando elas acabam. Tocar um cartão monta a fila a partir dele.

### Tocar

- **No disco** toca o arquivo local. **Na nuvem** toca por streaming (mpv + yt-dlp, com os cookies do Firefox) e tem o
  botão **Baixar** (`ytm-local download --id`).
- **Agora tocando** mostra a capa grande e a fila. Dá para pular para uma faixa, remover da fila ou limpar.
- **Autoplay**: com a fila acabando, o app busca mais faixas sozinho, então a música não para.
- **Mesmo estilo** (padrão): ao tocar um cartão ou um resultado de busca, a fila é a rádio do YouTube Music daquela
  faixa (`ytm-local similar`), com o que já está na sua biblioteca na frente. O YT Music não expõe gênero por faixa,
  então "estilo" é a recomendação dele. Dá para desligar em Agora tocando.
- **Volume e mudo** ficam na barra do player. O volume é lembrado entre execuções.

### Busca

O campo no topo filtra a biblioteca enquanto você digita (sem acento, por começo de palavra; `Ctrl+F` foca). `Enter`
também procura no YouTube Music (`ytm-local search`): o que ainda não está na biblioteca aparece com a etiqueta
**Nova** e só toca por streaming.

### Histórico de reprodução

O Vitrola registra o que você ouve, para o perfil e as sugestões melhorarem com o tempo.

- **Quando grava**: a linha só é criada depois de 5 s realmente ouvidos. Cliques acidentais e erros não entram.
- **O que grava**: tempo ouvido (pausa e seek não contam), duração, como a faixa começou (cartão, linha da
  biblioteca, busca, fila, próxima, anterior, automática), como terminou (acabou, pulou, trocada, fechou, erro),
  se tocou do disco, título e artista. O tempo parcial é salvo a cada 5 s, então um travamento perde pouco.
- **Contador** na barra lateral: quantas reproduções estão registradas, subindo no instante em que a faixa passa dos
  5 s.
- **Remover e desfazer**: os dois botões acima do contador apagam a última reprodução registrada e devolvem a que
  foi removida (em pilha, enquanto o app estiver aberto).
- **Regra de "contou"**: 30 s ou 50% da faixa, o que vier primeiro. Ela vale na leitura (`ytm-local stats` e
  `recommend`), não na gravação, então dá para mudá-la sem perder dado.

### Como as sugestões são escolhidas

O `recommend` pontua cada faixa por afinidade com o artista, fonte (curtida > playlist > biblioteca), "favorita
esquecida" (fora do histórico do YT Music), repetição recente, **saturação** e diversidade de artistas, com um pouco
de aleatoriedade para não ser sempre a mesma lista. A saturação reduz a chance de uma faixa que você reproduziu
muito nas últimas semanas (janelas de 7 e 30 dias, relativas ao seu ritmo habitual dos últimos 90 dias), e some
sozinha quando essas reproduções saem da janela. Os parâmetros ainda não foram calibrados com o seu histórico.

### Memória e capas

- **Capas** não vão para o disco: são buscadas só quando há internet, ficam em memória (até ~300) e, sem rede,
  aparece um bloco neutro.
- **RAM**: renderizador por software (`GSK_RENDERER=cairo`, use `TOCA_GPU=1` para o padrão do GTK), listas
  virtualizadas, um processo `mpv` por faixa que só existe enquanto toca, interface orientada a eventos (sem
  polling). Medido: ~40 MB de memória própria.

### Variáveis de ambiente

- `TOCA_YTM`: caminho do `ytm-local`.
- `YTM_LOCAL_DATA`: onde fica o banco.
- `TOCA_GPU=1`: usa o renderizador padrão do GTK.
- `TOCA_APP_ID`: abre uma segunda instância isolada (útil em testes).

## ytm-local (linha de comando)

### Biblioteca

```bash
uv run ytm-local auth --from-browser firefox   # lê os cookies do Firefox (você precisa estar logado no music.youtube.com)
# alternativa manual: uv run ytm-local auth (cola os headers)
uv run ytm-local sync                 # curtidas, biblioteca, playlists e histórico -> SQLite
uv run ytm-local status
uv run ytm-local download --limit 5 --cookies-browser firefox   # teste pequeno
uv run ytm-local download --cookies-browser firefox             # tudo que está pendente
```

Estado em `~/.local/share/ytm-local/state.db`, músicas em `~/Music/ytm-local` (mude com `YTM_LOCAL_DATA` e
`YTM_LOCAL_MUSIC`). É retomável: faixas `done` não são rebaixadas. `--retry-failed` tenta de novo as que falharam.

### Recomendação e descoberta

```bash
uv run ytm-local recommend              # o que ouvir agora (só faixas já baixadas)
uv run ytm-local recommend -n 20 --mode again --play   # o que você anda ouvindo, tocando no mpv
uv run ytm-local discover               # músicas novas, a partir da rádio das suas favoritas
uv run ytm-local discover --new-artists --add 10        # só artistas novos; 10 vão para a fila de download
```

`discover` sorteia sementes entre as favoritas, pega a rádio do YouTube Music de cada uma e ordena pelo que aparece
em mais rádios; o que você já tem fica de fora. Rode `sync history` para o `recommend` usar o histórico do YT Music.

### Perfil: gênero e estatísticas

```bash
uv run ytm-local genres fetch           # gênero por artista (iTunes, com Deezer de reserva), só de quem já tocou
uv run ytm-local genres fetch --all     # a biblioteca inteira
uv run ytm-local genres set "Artista" pagode   # ajuste manual, que vale mais que as outras fontes
uv run ytm-local genres show
uv run ytm-local stats                  # top artistas, taxa de pulo, gênero, hora do dia, como começou
```

O gênero é guardado por artista (`artist_tags`), com a fonte de cada tag, e não por reprodução. Assim dá para trocar
a fonte ou corrigir uma tag e reclassificar todo o histórico.

## Limitações conhecidas

- Pela URL `watch?v=` o título pode vir como a versão em vídeo ("Official Video").
  Próximo passo: gravar os metadados do `ytmusicapi` (artista, álbum, faixa) nas tags.
- Qualidade máxima (~256 kbps) exige cookies de uma conta com YouTube Music Premium.
- Uso pessoal: baixar viola os termos do YouTube.

## Próximos passos

- Tags a partir do banco (mutagen) e pasta `Artista/Álbum/NN Faixa`.
- `sync` periódico via systemd timer.
- Servir com Navidrome.
- Calibrar os pesos do `recommend` (meia-vida, saturação) com o histórico real, depois de algumas centenas de
  reproduções.
