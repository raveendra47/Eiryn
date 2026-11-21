use serde::{Deserialize, Serialize};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, PhraseQuery, Query, TermQuery};
use tantivy::schema::*;
use tantivy::tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

fn value_as_text(v: &OwnedValue) -> Option<&str> {
    match v {
        OwnedValue::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

use crate::library::track::Track;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub track_id: String,
    pub score: f32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub matched_lyric: Option<String>,
}

/// Bumped whenever the schema or analyzer changes; mismatched on-disk
/// indexes are wiped and rebuilt once instead of returning stale tokens.
const INDEX_VERSION: &str = "4";

/// Field boosts: title > artist > filename > album > lyrics > genre.
const BOOST_TITLE: f32 = 3.0;
const BOOST_ARTIST: f32 = 2.0;
const BOOST_FILENAME: f32 = 1.5;
const BOOST_ALBUM: f32 = 1.5;
const BOOST_LYRICS: f32 = 1.2;
const BOOST_GENRE: f32 = 1.0;
const BOOST_PHRASE: f32 = 2.0;
const BOOST_FUZZY: f32 = 0.5;

pub struct TantivyIndex {
    index: Index,
    index_writer: IndexWriter,
    reader: IndexReader,
    fields: FieldFields,
}

#[derive(Clone, Copy)]
struct FieldFields {
    track_id: Field,
    title: Field,
    artist: Field,
    album: Field,
    genre: Field,
    filename: Field,
    file_path: Field,
    lyrics: Field,
}

fn folding_indexing() -> TextFieldIndexing {
    // Positions are required for PhraseQuery; frequencies improve BM25.
    TextFieldIndexing::default()
        .set_tokenizer("folding")
        .set_index_option(IndexRecordOption::WithFreqsAndPositions)
}

fn text_field(
    builder: &mut SchemaBuilder,
    name: &str,
    stored: bool,
) -> Field {
    let mut options = TextOptions::default().set_indexing_options(folding_indexing());
    if stored {
        options = options.set_stored();
    }
    builder.add_text_field(name, options)
}

impl TantivyIndex {
    pub fn new(path: &Path) -> Result<Self, String> {
        // Wipe indexes written by an older schema/analyzer (one-time rebuild).
        let version_file = path.join("eiryn_index_version");
        let current = std::fs::read_to_string(&version_file).unwrap_or_default();
        if current.trim() != INDEX_VERSION {
            let _ = std::fs::remove_dir_all(path);
            let _ = std::fs::create_dir_all(path);
        }

        let mut schema_builder = Schema::builder();

        let track_id = schema_builder.add_text_field("track_id", STRING | STORED);
        let title = text_field(&mut schema_builder, "title", true);
        let artist = text_field(&mut schema_builder, "artist", true);
        let album = text_field(&mut schema_builder, "album", true);
        let genre = text_field(&mut schema_builder, "genre", true);
        let filename = text_field(&mut schema_builder, "filename", true);
        let file_path = schema_builder.add_text_field("file_path", STORED);
        let lyrics = text_field(&mut schema_builder, "lyrics", true);

        let schema = schema_builder.build();
        let fields = FieldFields {
            track_id,
            title,
            artist,
            album,
            genre,
            filename,
            file_path,
            lyrics,
        };

        // Create or open index
        let index = Index::create_in_dir(path, schema.clone())
            .or_else(|_| Index::open_in_dir(path))
            .map_err(|e| format!("Failed to create/open tantivy index: {}", e))?;

        // Lowercase + ASCII-fold so accented queries match ("beyonce"→"Beyoncé").
        // Must match the "folding" indexing options used by the TEXT fields.
        index.tokenizers().register(
            "folding",
            TextAnalyzer::builder(SimpleTokenizer::default())
                .filter(LowerCaser)
                .filter(AsciiFoldingFilter)
                .build(),
        );

        let index_writer = index
            .writer(50_000_000) // 50MB buffer
            .map_err(|e| format!("Failed to create index writer: {}", e))?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| format!("Failed to create index reader: {}", e))?;

        let _ = std::fs::write(&version_file, INDEX_VERSION);

        Ok(Self {
            index,
            index_writer,
            reader,
            fields,
        })
    }

    /// Tokenize + fold a raw query with the same analyzer used at index time.
    fn analyze_query(&self, query: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(mut analyzer) = self.index.tokenizers().get("folding") {
            let mut stream = analyzer.token_stream(query);
            while stream.advance() {
                let text = stream.token().text.clone();
                if !text.is_empty() {
                    out.push(text);
                }
            }
        }
        out
    }

    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    pub fn index_track_with_lyrics(&mut self, track: &Track, lyrics: Option<&str>) -> Result<(), String> {
        // Delete existing document if any
        let _ = self.remove_track(&track.id);

        let filename = Path::new(&track.file_path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let cleaned_lyrics = lyrics.map(clean_lyrics_text).unwrap_or_default();

        // Add new document
        self.index_writer
            .add_document(doc!(
                self.fields.track_id => track.id.as_str(),
                self.fields.title => track.title.as_str(),
                self.fields.artist => track.artist.as_str(),
                self.fields.album => track.album.as_str(),
                self.fields.genre => track.raw_genre_names.as_str(),
                self.fields.filename => filename.as_str(),
                self.fields.file_path => track.file_path.as_str(),
                self.fields.lyrics => cleaned_lyrics.as_str(),
            ))
            .map_err(|e| format!("Failed to add document: {}", e))?;

        // Commit periodically (caller should commit after batch operations)
        Ok(())
    }

    pub fn index_track(&mut self, track: &Track) -> Result<(), String> {
        self.index_track_with_lyrics(track, None)
    }

    pub fn remove_track(&mut self, track_id: &str) -> Result<(), String> {
        let term = Term::from_field_text(self.fields.track_id, track_id);
        self.index_writer.delete_term(term);
        Ok(())
    }

    pub fn commit(&mut self) -> Result<(), String> {
        self.index_writer
            .commit()
            .map_err(|e| format!("Failed to commit index: {}", e))?;
        // OnCommitWithDelay reloads lazily; force visibility so searches
        // immediately after a scan/index call see the new documents.
        self.reader
            .reload()
            .map_err(|e| format!("Failed to reload index reader: {}", e))?;
        Ok(())
    }

    pub fn rebuild_with_lyrics(
        &mut self,
        tracks: &[Track],
        lyrics_map: &std::collections::HashMap<String, String>,
    ) -> Result<(), String> {
        // Drop all documents
        self.index_writer
            .delete_all_documents()
            .map_err(|e| format!("Failed to delete all: {}", e))?;

        // Re-index all tracks
        for track in tracks {
            let lyric = lyrics_map.get(&track.id).map(|s| s.as_str());
            self.index_track_with_lyrics(track, lyric)?;
        }

        // Commit
        self.commit()?;

        Ok(())
    }

    pub fn rebuild(&mut self, tracks: &[Track]) -> Result<(), String> {
        let empty = std::collections::HashMap::new();
        self.rebuild_with_lyrics(tracks, &empty)
    }

    /// Ranked search: per-term groups AND-ed together (each group is an
    /// exact/prefix/fuzzy disjunction across boosted fields), plus a
    /// phrase boost on title. Replaces the old exact-Term-then-parser flow,
    /// which never matched multi-word queries in phase one and had no
    /// field boosts, folding, or fuzzy/prefix support.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        if query.trim().is_empty() || limit == 0 {
            return Vec::new();
        }

        let terms = self.analyze_query(query);
        if terms.is_empty() {
            return Vec::new();
        }

        let term_query = |field: Field, boost: f32, text: &str| -> Box<dyn Query> {
            let term = Term::from_field_text(field, text);
            let q: Box<dyn Query> = Box::new(TermQuery::new(term, IndexRecordOption::Basic));
            if (boost - 1.0).abs() < f32::EPSILON {
                q
            } else {
                Box::new(BoostQuery::new(q, boost))
            }
        };

        let mut must_clauses: Vec<(Occur, Box<dyn Query>)> = Vec::with_capacity(terms.len() + 1);
        for term in &terms {
            let mut should: Vec<(Occur, Box<dyn Query>)> = Vec::with_capacity(9);
            // Exact matches, boosted by field.
            should.push((Occur::Should, term_query(self.fields.title, BOOST_TITLE, term)));
            should.push((Occur::Should, term_query(self.fields.artist, BOOST_ARTIST, term)));
            should.push((Occur::Should, term_query(self.fields.filename, BOOST_FILENAME, term)));
            should.push((Occur::Should, term_query(self.fields.album, BOOST_ALBUM, term)));
            should.push((Occur::Should, term_query(self.fields.lyrics, BOOST_LYRICS, term)));
            should.push((Occur::Should, term_query(self.fields.genre, BOOST_GENRE, term)));
            // Prefix matches for as-you-type queries.
            for (field, boost) in [
                (self.fields.title, BOOST_TITLE),
                (self.fields.artist, BOOST_ARTIST),
            ] {
                let prefix_term = Term::from_field_text(field, term);
                let q: Box<dyn Query> =
                    Box::new(FuzzyTermQuery::new_prefix(prefix_term, 0, true));
                should.push((Occur::Should, Box::new(BoostQuery::new(q, boost * 0.8))));
            }
            // Typo tolerance (edit distance 1) on title/artist.
            for (field, boost) in [
                (self.fields.title, BOOST_TITLE),
                (self.fields.artist, BOOST_ARTIST),
            ] {
                let fuzzy_term = Term::from_field_text(field, term);
                let q: Box<dyn Query> = Box::new(FuzzyTermQuery::new(fuzzy_term, 1, true));
                should.push((Occur::Should, Box::new(BoostQuery::new(q, boost * BOOST_FUZZY))));
            }
            must_clauses.push((Occur::Must, Box::new(BooleanQuery::new(should))));
        }

        // Exact-phrase boost on title for multi-word queries.
        if terms.len() > 1 {
            let phrase_terms: Vec<Term> = terms
                .iter()
                .map(|t| Term::from_field_text(self.fields.title, t))
                .collect();
            let phrase: Box<dyn Query> = Box::new(PhraseQuery::new(phrase_terms));
            must_clauses.push((
                Occur::Should,
                Box::new(BoostQuery::new(phrase, BOOST_PHRASE)),
            ));
        }

        let query_lower = query.to_lowercase();
        let query_obj = BooleanQuery::new(must_clauses);
        let searcher = self.reader.searcher();
        let mut results = Vec::new();
        if let Ok(docs) = searcher.search(&query_obj, &TopDocs::with_limit(limit)) {
            for (score, doc_address) in docs {
                if let Ok(doc) = searcher.doc::<TantivyDocument>(doc_address) {
                    if let Some(val) = doc.get_first(self.fields.track_id).and_then(value_as_text) {
                        let title = doc
                            .get_first(self.fields.title)
                            .and_then(value_as_text)
                            .unwrap_or("")
                            .to_string();
                        let artist = doc
                            .get_first(self.fields.artist)
                            .and_then(value_as_text)
                            .unwrap_or("")
                            .to_string();
                        let album = doc
                            .get_first(self.fields.album)
                            .and_then(value_as_text)
                            .unwrap_or("")
                            .to_string();
                        let lyrics_text = doc
                            .get_first(self.fields.lyrics)
                            .and_then(value_as_text)
                            .unwrap_or("");

                        let mut matched_lyric: Option<String> = None;
                        if !lyrics_text.is_empty() {
                            let lyrics_lower = lyrics_text.to_lowercase();
                            if let Some(pos) = lyrics_lower.find(&query_lower) {
                                matched_lyric = Some(extract_lyric_snippet(lyrics_text, pos, query_lower.len()));
                            } else {
                                for term in &terms {
                                    if term.len() >= 3 {
                                        if let Some(pos) = lyrics_lower.find(term.as_str()) {
                                            matched_lyric = Some(extract_lyric_snippet(lyrics_text, pos, term.len()));
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        results.push(SearchResult {
                            track_id: val.to_string(),
                            score,
                            title,
                            artist,
                            album,
                            matched_lyric,
                        });
                    }
                }
            }
        }

        // TopDocs already returns score-descending order.
        results
    }
}

pub fn clean_lyrics_text(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    let mut in_bracket = false;
    for c in raw.chars() {
        if c == '[' || c == '<' {
            in_bracket = true;
        } else if c == ']' || c == '>' {
            in_bracket = false;
        } else if !in_bracket {
            cleaned.push(c);
        }
    }
    cleaned
}

fn extract_lyric_snippet(lyrics_text: &str, match_pos: usize, match_len: usize) -> String {
    let start = match_pos.saturating_sub(25);
    let end = (match_pos + match_len + 35).min(lyrics_text.len());
    let start_idx = lyrics_text.char_indices().map(|(i, _)| i).find(|&i| i >= start).unwrap_or(0);
    let end_idx = lyrics_text.char_indices().map(|(i, _)| i).find(|&i| i >= end).unwrap_or(lyrics_text.len());
    let mut snippet = String::new();
    if start_idx > 0 {
        snippet.push_str("...");
    }
    snippet.push_str(lyrics_text[start_idx..end_idx].trim());
    if end_idx < lyrics_text.len() {
        snippet.push_str("...");
    }
    snippet.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::track::Track;

    fn test_track(id: &str, title: &str, artist: &str, album: &str, file_path: &str) -> Track {
        Track {
            id: id.to_string(),
            file_path: file_path.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            album_artist: String::new(),
            album_id: String::new(),
            composer: String::new(),
            track_number: 0,
            disc_number: 0,
            year: 0,
            genre: String::new(),
            raw_genre_names: String::new(),
            duration: 0.0,
            sample_rate: 0,
            bit_depth: 0,
            channels: 0,
            bitrate: 0,
            file_size: 0,
            file_format: "mp3".to_string(),
            has_artwork: false,
            replaygain_track_gain: 0.0,
            replaygain_album_gain: 0.0,
            cue_parent_id: None,
            cue_offset: 0.0,
            artwork_key: None,
            bpm: 0.0,
            musical_key: String::new(),
            energy: 0.0,
            analysis_failed_reason: None,
            file_modified: 0,
            matched_lyric: None,
        }
    }

    fn temp_index(tracks: &[Track]) -> TantivyIndex {
        let dir = std::env::temp_dir().join(format!("eiryn-search-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut index = TantivyIndex::new(&dir).unwrap();
        index.rebuild(tracks).unwrap();
        index
    }

    fn seed_tracks() -> Vec<Track> {
        vec![
            test_track("1", "Shine On You Crazy Diamond", "Pink Floyd", "Wish You Were Here", "/m/01-shine.mp3"),
            test_track("2", "Beyoncé – Halo", "Beyoncé", "I Am...", "/m/02-halo.mp3"),
            test_track("3", "Dark Side Jam", "Studio Band", "Blue Sessions", "/m/dark-side-jam.mp3"),
        ]
    }

    #[test]
    fn test_accent_folding() {
        let index = temp_index(&seed_tracks());
        let hits = index.search("beyonce", 10);
        assert!(hits.iter().any(|r| r.track_id == "2"), "folded query should match Beyoncé, got {hits:?}");
        // And the reverse direction (accented query against folded index).
        let hits = index.search("Beyoncé", 10);
        assert!(hits.iter().any(|r| r.track_id == "2"));
    }

    #[test]
    fn test_prefix_and_multiword() {
        let index = temp_index(&seed_tracks());
        let hits = index.search("dark si", 10);
        assert!(!hits.is_empty(), "prefix query should match");
        assert_eq!(hits[0].track_id, "3");
        // Multi-word exact title outranks partial matches.
        let hits = index.search("shine on you crazy diamond", 10);
        assert_eq!(hits[0].track_id, "1");
    }

    #[test]
    fn test_typo_tolerance_and_field_boosts() {
        let index = temp_index(&seed_tracks());
        let hits = index.search("floud", 10);
        assert!(hits.iter().any(|r| r.track_id == "1"), "fuzzy should match Floyd, got {hits:?}");
        // Title match outranks album-only match for the same term.
        let tracks = vec![
            test_track("a", "Blue Morning", "Someone", "Elsewhere", "/m/a.mp3"),
            test_track("b", "Other Song", "Someone Else", "Blue Morning", "/m/b.mp3"),
        ];
        let index = temp_index(&tracks);
        let hits = index.search("blue morning", 10);
        assert_eq!(hits[0].track_id, "a");
    }

    #[test]
    fn test_filename_search_and_empty() {
        let index = temp_index(&seed_tracks());
        let hits = index.search("02-halo", 10);
        assert!(hits.iter().any(|r| r.track_id == "2"));
        assert!(index.search("   ", 10).is_empty());
        assert!(index.search("zzz-no-such-track", 10).is_empty());
    }

    #[test]
    fn test_lyrics_search_and_snippet() {
        let dir = std::env::temp_dir().join(format!("eiryn-search-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut index = TantivyIndex::new(&dir).unwrap();
        let tracks = vec![
            test_track("10", "Heroes", "David Bowie", "Heroes", "/m/heroes.mp3"),
            test_track("11", "Different Song", "Artist", "Album", "/m/other.mp3"),
        ];
        let mut lyrics = std::collections::HashMap::new();
        lyrics.insert("10".to_string(), "[00:15.00]I, I will be king\n[00:20.00]And you, you will be queen\n[00:25.00]Though nothing will drive them away\n[00:30.00]We can be heroes, just for one day".to_string());
        index.rebuild_with_lyrics(&tracks, &lyrics).unwrap();

        let hits = index.search("drive them away", 10);
        assert!(!hits.is_empty(), "lyric line search should match");
        assert_eq!(hits[0].track_id, "10");
        assert!(hits[0].matched_lyric.is_some());
        let snippet = hits[0].matched_lyric.as_ref().unwrap();
        assert!(snippet.to_lowercase().contains("drive them away"), "snippet was {snippet}");
    }
}
