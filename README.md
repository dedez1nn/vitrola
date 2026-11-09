# ytm-local

Backup local da sua biblioteca do YouTube Music, só áudio (m4a original, sem reencode).

## Fluxo

```bash
uv run ytm-local auth                 # cola os headers do music.youtube.com (uma vez)
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
