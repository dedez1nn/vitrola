//! Busca na biblioteca: sem diferenciar maiúsculas nem acentos ("ze neto" acha "Zé Neto").

use crate::db::Track;

/// Minúsculas e sem acentos comuns do português.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            _ => c,
        })
        .collect()
}

pub struct Entry {
    pub track: Track,
    title: Vec<String>, // palavras do título
    all: Vec<String>,   // palavras do título + artistas
}

fn words(s: &str) -> Vec<String> {
    fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

pub fn build(tracks: Vec<Track>) -> Vec<Entry> {
    tracks
        .into_iter()
        .map(|t| {
            let title = words(&t.title);
            let mut all = title.clone();
            all.extend(words(&t.artists));
            Entry { track: t, title, all }
        })
        .collect()
}

/// Cada palavra da busca precisa ser o começo de alguma palavra do título ou do artista
/// ("neto" acha "Zé Neto", mas não "Guineto"). Quem casa no título vem antes; depois, o que está no disco.
pub fn query(index: &[Entry], q: &str, limit: usize) -> Vec<Track> {
    let q = words(q);
    if q.is_empty() {
        return vec![];
    }
    let starts = |ws: &[String], w: &String| ws.iter().any(|x| x.starts_with(w.as_str()));
    let mut hits: Vec<(bool, &Entry)> = index
        .iter()
        .filter(|e| q.iter().all(|w| starts(&e.all, w)))
        .map(|e| (q.iter().all(|w| starts(&e.title, w)), e))
        .collect();
    hits.sort_by_key(|(in_title, e)| (!*in_title, !e.track.local)); // estável
    hits.into_iter().take(limit).map(|(_, e)| e.track.clone()).collect()
}
