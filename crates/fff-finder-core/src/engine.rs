//! Search engine: one `fff-search` index per search root, searched in
//! parallel and merged by score. Folders outside every root get a temporary
//! index on demand, so "search this folder" works anywhere.

use crate::entry::{FileEntry, display_name};
use crate::kind::{self, KindFilter};
use fff_search::file_picker::FilePicker;
use fff_search::{
    DirSearchConfig, FFFMode, FilePickerOptions, FrecencyTracker, FuzzySearchOptions, GrepMode,
    GrepSearchOptions, MixedItemRef, MixedSearchConfig, PaginationArgs, QueryParser, QueryTracker,
    SharedFilePicker, SharedFrecency, SharedQueryTracker,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// Temporary (outside-of-roots) indexes kept alive, most recent first.
const MAX_ADHOC: usize = 3;

#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub roots: Vec<PathBuf>,
    /// Directory for frecency/query-history databases; `None` disables them.
    pub db_dir: Option<PathBuf>,
    /// Keep indexes live with filesystem watchers.
    pub watch: bool,
}

#[derive(Clone)]
struct RootIndex {
    path: PathBuf,
    picker: SharedFilePicker,
}

impl RootIndex {
    fn start(path: &Path, frecency: &SharedFrecency, watch: bool) -> anyhow::Result<RootIndex> {
        let picker = SharedFilePicker::default();
        FilePicker::new_with_shared_state(
            picker.clone(),
            frecency.clone(),
            FilePickerOptions {
                base_path: path.to_string_lossy().into_owned(),
                mode: FFFMode::Neovim,
                watch,
                follow_symlinks: false,
                // Roots are chosen by the user, so home and `/` are allowed.
                enable_fs_root_scanning: true,
                enable_home_dir_scanning: true,
                ..Default::default()
            },
        )
        .map_err(|e| anyhow::anyhow!("cannot index {}: {e}", path.display()))?;
        Ok(RootIndex {
            path: path.to_path_buf(),
            picker,
        })
    }

    fn status(&self) -> RootStatus {
        let guard = self.picker.read().ok();
        let picker = guard.as_ref().and_then(|g| g.as_ref());
        match picker {
            Some(p) => {
                let progress = p.get_scan_progress();
                RootStatus {
                    path: self.path.clone(),
                    scanning: progress.is_scanning,
                    indexed_files: if progress.is_scanning {
                        progress.scanned_files_count
                    } else {
                        p.live_file_count()
                    },
                    git_repo: p.has_git_repo(),
                }
            }
            None => RootStatus {
                path: self.path.clone(),
                scanning: true,
                indexed_files: 0,
                git_repo: false,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootStatus {
    pub path: PathBuf,
    pub scanning: bool,
    /// Files scanned so far while scanning, or live files once ready.
    pub indexed_files: usize,
    pub git_repo: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchRequest {
    pub query: String,
    /// Only return results under this folder.
    pub scope: Option<PathBuf>,
    pub kind: KindFilter,
    pub limit: usize,
    pub show_hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub entry: FileEntry,
    pub score: i32,
    /// Byte ranges in `entry.name` to highlight.
    pub highlights: Vec<(usize, usize)>,
    pub git_status: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub total_matched: usize,
    pub total_indexed: usize,
    pub elapsed: Duration,
    /// True when some searched index was still scanning (results incomplete).
    pub indexing: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GrepRequest {
    pub pattern: String,
    pub scope: Option<PathBuf>,
    pub regex: bool,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrepHit {
    pub path: PathBuf,
    pub line: u64,
    pub column: usize,
    pub text: String,
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GrepResponse {
    pub hits: Vec<GrepHit>,
    pub files_with_matches: usize,
    pub files_searched: usize,
    pub elapsed: Duration,
    pub indexing: bool,
}

#[derive(Default)]
struct Indexes {
    roots: Vec<RootIndex>,
    adhoc: Vec<RootIndex>,
}

pub struct Engine {
    indexes: RwLock<Indexes>,
    frecency: SharedFrecency,
    queries: SharedQueryTracker,
    watch: bool,
    generation: AtomicU64,
}

impl Engine {
    pub fn new(options: EngineOptions) -> Arc<Engine> {
        let (frecency, queries) = open_dbs(options.db_dir.as_deref());
        let engine = Arc::new(Engine {
            indexes: RwLock::new(Indexes::default()),
            frecency,
            queries,
            watch: options.watch,
            generation: AtomicU64::new(0),
        });
        engine.set_roots(&options.roots);
        engine
    }

    /// Replace the indexed roots. Unchanged roots keep their index.
    pub fn set_roots(&self, roots: &[PathBuf]) {
        let roots = normalize_roots(roots);
        let mut indexes = self.indexes.write().unwrap_or_else(|e| e.into_inner());
        let mut keep: Vec<RootIndex> = Vec::new();
        for root in &roots {
            if let Some(existing) = indexes.roots.iter().find(|r| &r.path == root) {
                keep.push(existing.clone());
                continue;
            }
            match RootIndex::start(root, &self.frecency, self.watch) {
                Ok(index) => keep.push(index),
                Err(err) => log::warn!("{err}"),
            }
        }
        indexes.roots = keep;
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn roots(&self) -> Vec<PathBuf> {
        self.read_indexes()
            .roots
            .iter()
            .map(|r| r.path.clone())
            .collect()
    }

    /// Bumped whenever the set of roots changes.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> Vec<RootStatus> {
        self.read_indexes()
            .roots
            .iter()
            .map(RootIndex::status)
            .collect()
    }

    pub fn is_scanning(&self) -> bool {
        self.status().iter().any(|s| s.scanning)
    }

    /// Block until every root finished its initial scan, or `timeout` passes.
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let pickers: Vec<SharedFilePicker> = self
            .read_indexes()
            .roots
            .iter()
            .map(|r| r.picker.clone())
            .collect();
        pickers.iter().all(|p| {
            let left = deadline.saturating_duration_since(Instant::now());
            p.wait_for_scan(left)
        })
    }

    /// Create the index covering `scope` if needed and wait up to `timeout`
    /// for its scan. Returns whether it is ready.
    pub fn prepare(&self, scope: Option<&Path>, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        self.indexes_for(scope).iter().all(|r| {
            let left = deadline.saturating_duration_since(Instant::now());
            r.picker.wait_for_scan(left)
        })
    }

    /// Rescan every root (for example after permissions changed).
    pub fn rescan(&self) {
        for root in &self.read_indexes().roots {
            if let Err(err) = root.picker.trigger_full_rescan_async(&self.frecency) {
                log::warn!("rescan {}: {err}", root.path.display());
            }
        }
    }

    /// Indexes covering `scope`: the roots containing it, or a temporary
    /// index of the folder itself when no root does.
    fn indexes_for(&self, scope: Option<&Path>) -> Vec<RootIndex> {
        let Some(scope) = scope else {
            return self.read_indexes().roots.clone();
        };
        {
            let indexes = self.read_indexes();
            let covering: Vec<RootIndex> = indexes
                .roots
                .iter()
                .filter(|r| scope.starts_with(&r.path))
                .cloned()
                .collect();
            if !covering.is_empty() {
                return covering;
            }
            if let Some(index) = indexes.adhoc.iter().find(|r| r.path == scope) {
                return vec![index.clone()];
            }
        }
        match RootIndex::start(scope, &self.frecency, self.watch) {
            Ok(index) => {
                let mut indexes = self.indexes.write().unwrap_or_else(|e| e.into_inner());
                indexes.adhoc.retain(|r| r.path != scope);
                indexes.adhoc.insert(0, index.clone());
                indexes.adhoc.truncate(MAX_ADHOC);
                vec![index]
            }
            Err(err) => {
                log::warn!("{err}");
                Vec::new()
            }
        }
    }

    pub fn search(&self, req: &SearchRequest) -> SearchResponse {
        let started = Instant::now();
        let limit = if req.limit == 0 { 500 } else { req.limit };
        let indexes = self.indexes_for(req.scope.as_deref());
        let indexing = indexes.iter().any(|r| r.status().scanning);

        let parts: Vec<RootPart> = std::thread::scope(|s| {
            let handles: Vec<_> = indexes
                .iter()
                .map(|root| s.spawn(move || self.search_root(root, req, limit)))
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });

        let mut total_matched = 0;
        let mut total_indexed = 0;
        let mut raw = Vec::new();
        for part in parts {
            total_matched += part.total_matched;
            total_indexed += part.total_indexed;
            raw.extend(part.hits);
        }
        let words: Vec<String> = fuzzy_text(&req.query)
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        // Typo-tolerant matches are a fallback: when some results contain every
        // word as an in-order subsequence (fzf-style), keep only those.
        let base = req.scope.as_deref();
        let strict = |hit: &RawHit| {
            let shown = base
                .and_then(|b| hit.path.strip_prefix(b).ok())
                .unwrap_or(&hit.path)
                .to_string_lossy()
                .to_lowercase();
            words.iter().all(|w| is_subsequence(w, &shown))
        };
        if !words.is_empty() && raw.iter().any(&strict) {
            raw.retain(strict);
        }
        for hit in &mut raw {
            hit.score = rerank(hit, &words);
        }
        raw.sort_by_key(|h| std::cmp::Reverse(h.score));
        let mut seen = HashSet::new();
        raw.retain(|h| seen.insert(h.path.clone()));
        raw.truncate(limit);

        let needle = fuzzy_text(&req.query);
        let hits = raw
            .into_iter()
            .filter_map(|h| h.into_hit(&needle, req.show_hidden))
            .collect();
        SearchResponse {
            hits,
            total_matched,
            total_indexed,
            elapsed: started.elapsed(),
            indexing,
        }
    }

    fn search_root(&self, root: &RootIndex, req: &SearchRequest, limit: usize) -> RootPart {
        let mut part = RootPart::default();
        let Ok(guard) = root.picker.read() else {
            return part;
        };
        let Some(picker) = guard.as_ref() else {
            return part;
        };
        let base = picker.base_path().to_path_buf();
        part.total_indexed = picker.live_file_count();

        let scope_rel = req
            .scope
            .as_deref()
            .filter(|s| *s != base.as_path())
            .and_then(|s| s.strip_prefix(&base).ok())
            .map(|rel| rel.to_string_lossy().replace('\\', "/"));
        // A glob narrows the search inside fff; the prefix check below is exact.
        let scope_glob = scope_rel
            .as_deref()
            .filter(|rel| {
                !rel.chars()
                    .any(|c| c.is_whitespace() || "*?[]{}!\\".contains(c))
            })
            .map(|rel| format!("{rel}/**"));

        let user = req.query.trim();
        let single_char =
            fuzzy_text(user).chars().count() == 1 && !user.contains(char::is_whitespace);
        let mut query = if single_char {
            String::new()
        } else {
            user.to_string()
        };
        for ext in req.kind.extensions() {
            query.push_str(" *.");
            query.push_str(ext);
        }
        if let Some(glob) = &scope_glob {
            query.push(' ');
            query.push_str(glob);
        }
        let mut query = query.trim().to_string();
        // In mixed search a query ending in `/` means "folders only"; keep that
        // for a lone folder token but not for "main src/"-style path filters.
        if req.kind == KindFilter::Any
            && query.ends_with('/')
            && let Some((head, last)) = query.rsplit_once(char::is_whitespace)
        {
            query = format!("{last} {head}");
        }

        // fff ignores one-character words, so over-fetch and filter by name.
        // Over-fetch so the re-rank and package filter have candidates to work with.
        let fetch = if single_char || (scope_rel.is_some() && scope_glob.is_none()) {
            limit.max(20_000)
        } else {
            (limit * 4).max(2_000)
        };
        let qt_guard = self.queries.read().ok();
        let qt = qt_guard.as_ref().and_then(|g| g.as_ref());
        let options = FuzzySearchOptions {
            project_path: Some(&base),
            pagination: PaginationArgs {
                offset: 0,
                limit: fetch,
            },
            ..Default::default()
        };

        let in_scope = |path: &Path| {
            let scoped = match &req.scope {
                Some(scope) => path.starts_with(scope) && path != scope,
                None => true,
            };
            scoped && !inside_package(path, req.scope.as_deref())
        };
        let name_ok = |name: &str| {
            !single_char
                || name
                    .to_lowercase()
                    .contains(&fuzzy_text(user).to_lowercase())
        };

        if req.kind == KindFilter::Folder {
            let parsed = QueryParser::new(DirSearchConfig).parse(&query);
            let result = picker.fuzzy_search_directories(&parsed, options);
            part.total_matched = result.total_matched;
            for (item, score) in result.items.iter().zip(&result.scores) {
                let rel = item.relative_path(picker);
                if rel.is_empty() {
                    continue;
                }
                let path = join_rel(&base, &rel);
                let name = display_name(&path);
                if in_scope(&path) && name_ok(&name) {
                    part.hits.push(RawHit::dir(path, score.total));
                }
            }
        } else if req.kind == KindFilter::Any {
            let parsed = QueryParser::new(MixedSearchConfig).parse(&query);
            let result = picker.fuzzy_search_mixed(&parsed, qt, options);
            part.total_matched = result.total_matched;
            for (item, score) in result.items.iter().zip(&result.scores) {
                let hit = match item {
                    MixedItemRef::File(f) => {
                        let path = join_rel(&base, &f.relative_path(picker));
                        RawHit::file(
                            path,
                            f.size,
                            f.modified,
                            fff_search::git::format_git_status_opt(f.git_status),
                            score.total,
                        )
                    }
                    MixedItemRef::Dir(d) => {
                        let rel = d.relative_path(picker);
                        if rel.is_empty() {
                            continue;
                        }
                        RawHit::dir(join_rel(&base, &rel), score.total)
                    }
                };
                if in_scope(&hit.path) && name_ok(&display_name(&hit.path)) {
                    part.hits.push(hit);
                }
            }
        } else {
            let parsed = QueryParser::default().parse(&query);
            let result = picker.fuzzy_search(&parsed, qt, options);
            part.total_matched = result.total_matched;
            for (item, score) in result.items.iter().zip(&result.scores) {
                let path = join_rel(&base, &item.relative_path(picker));
                let kind = kind::classify(&path, false);
                if in_scope(&path) && req.kind.matches(kind) && name_ok(&display_name(&path)) {
                    part.hits.push(RawHit::file(
                        path,
                        item.size,
                        item.modified,
                        fff_search::git::format_git_status_opt(item.git_status),
                        score.total,
                    ));
                }
            }
        }
        part
    }

    /// Absolute paths of indexed files matching `pattern` (a glob over the
    /// path inside each root, like `**/*.app/Contents/Info.plist`), across
    /// every root. Nothing is re-ranked or hidden, package contents included.
    pub fn glob(&self, pattern: &str, limit: usize) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for root in &self.read_indexes().roots {
            let Ok(guard) = root.picker.read() else {
                continue;
            };
            let Some(picker) = guard.as_ref() else {
                continue;
            };
            let base = picker.base_path().to_path_buf();
            let result = picker.glob(
                pattern,
                FuzzySearchOptions {
                    project_path: Some(&base),
                    pagination: PaginationArgs { offset: 0, limit },
                    ..Default::default()
                },
            );
            found.extend(
                result
                    .items
                    .iter()
                    .map(|item| join_rel(&base, &item.relative_path(picker))),
            );
        }
        found.sort();
        found.dedup();
        found.truncate(limit);
        found
    }

    /// Files modified in the last 30 days across the roots, newest first,
    /// skipping hidden paths, `Library` folders, and package contents.
    pub fn recents(&self, limit: usize) -> Vec<FileEntry> {
        const WINDOW: Duration = Duration::from_secs(30 * 24 * 60 * 60);
        let cutoff = SystemTime::now()
            .checked_sub(WINDOW)
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let mut found: Vec<(u64, PathBuf, u64)> = Vec::new();
        for root in &self.read_indexes().roots {
            let Ok(guard) = root.picker.read() else {
                continue;
            };
            let Some(picker) = guard.as_ref() else {
                continue;
            };
            let base = picker.base_path().to_path_buf();
            let query = QueryParser::default().parse("");
            // An empty query ranks by frecency, then by newest modification.
            let result = picker.fuzzy_search(
                &query,
                None,
                FuzzySearchOptions {
                    pagination: PaginationArgs {
                        offset: 0,
                        limit: (limit * 4).max(4_000),
                    },
                    ..Default::default()
                },
            );
            for item in result.items {
                if item.modified < cutoff {
                    continue;
                }
                let rel = item.relative_path(picker);
                if rel
                    .split('/')
                    .any(|part| part.starts_with('.') || part == "Library")
                {
                    continue;
                }
                let path = join_rel(&base, &rel);
                if !inside_package(&path, None) {
                    found.push((item.modified, path, item.size));
                }
            }
        }
        found.sort_by_key(|f| std::cmp::Reverse(f.0));
        found.truncate(limit);
        found
            .into_iter()
            .filter_map(|(modified, path, size)| {
                RawHit::file(path, size, modified, None, 0)
                    .into_hit("", false)
                    .map(|hit| hit.entry)
            })
            .collect()
    }

    /// Content search across the indexes covering `scope`.
    pub fn grep(&self, req: &GrepRequest) -> GrepResponse {
        let started = Instant::now();
        let limit = if req.limit == 0 { 200 } else { req.limit };
        let indexes = self.indexes_for(req.scope.as_deref());
        let mut response = GrepResponse {
            indexing: indexes.iter().any(|r| r.status().scanning),
            ..Default::default()
        };
        for root in &indexes {
            let Ok(guard) = root.picker.read() else {
                continue;
            };
            let Some(picker) = guard.as_ref() else {
                continue;
            };
            let base = picker.base_path().to_path_buf();
            let mut text = req.pattern.clone();
            if let Some(rel) = req
                .scope
                .as_deref()
                .filter(|s| *s != base.as_path())
                .and_then(|s| s.strip_prefix(&base).ok())
            {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if !rel.contains(char::is_whitespace) {
                    text.push_str(&format!(" {rel}/**"));
                }
            }
            let query = fff_search::parse_grep_query(&text);
            let options = GrepSearchOptions {
                mode: if req.regex {
                    GrepMode::Regex
                } else {
                    GrepMode::PlainText
                },
                page_limit: limit,
                max_matches_per_file: 20,
                time_budget_ms: 5_000,
                trim_whitespace: true,
                ..Default::default()
            };
            let result = picker.grep(&query, &options);
            response.files_with_matches += result.files_with_matches;
            response.files_searched += result.total_files_searched;
            for m in &result.matches {
                let Some(file) = result.files.get(m.file_index) else {
                    continue;
                };
                let path = join_rel(&base, &file.relative_path(picker));
                if let Some(scope) = &req.scope
                    && !path.starts_with(scope)
                {
                    continue;
                }
                response.hits.push(GrepHit {
                    path,
                    line: m.line_number,
                    column: m.col,
                    text: m.line_content.clone(),
                    ranges: m
                        .match_byte_offsets
                        .iter()
                        .map(|&(a, b)| (a as usize, b as usize))
                        .collect(),
                });
                if response.hits.len() >= limit {
                    break;
                }
            }
            if response.hits.len() >= limit {
                break;
            }
        }
        response.elapsed = started.elapsed();
        response
    }

    /// Record that the user opened `path`, boosting it in later searches.
    pub fn track_open(&self, path: &Path, query: Option<&str>) {
        let Ok(frecency) = self.frecency.read() else {
            return;
        };
        let Some(tracker) = frecency.as_ref() else {
            return;
        };
        if let Err(err) = tracker.track_access(path) {
            log::debug!("track_access {}: {err}", path.display());
            return;
        }
        let roots = self.read_indexes().roots.clone();
        for root in roots.iter().filter(|r| path.starts_with(&r.path)) {
            if let Ok(mut guard) = root.picker.write()
                && let Some(picker) = guard.as_mut()
            {
                let _ = picker.update_single_file_frecency(path, tracker);
            }
            if let Some(query) = query.filter(|q| !q.trim().is_empty())
                && let Ok(mut qt) = self.queries.write()
                && let Some(qt) = qt.as_mut()
            {
                let _ = qt.track_query_completion(query.trim(), &root.path, path);
            }
        }
    }

    fn read_indexes(&self) -> std::sync::RwLockReadGuard<'_, Indexes> {
        self.indexes.read().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let indexes = self.indexes.get_mut().unwrap_or_else(|e| e.into_inner());
        for root in indexes.roots.iter().chain(indexes.adhoc.iter()) {
            root.picker.cancel();
        }
    }
}

#[derive(Default)]
struct RootPart {
    hits: Vec<RawHit>,
    total_matched: usize,
    total_indexed: usize,
}

struct RawHit {
    path: PathBuf,
    is_dir: bool,
    size: u64,
    modified: u64,
    git_status: Option<&'static str>,
    score: i32,
}

impl RawHit {
    fn file(
        path: PathBuf,
        size: u64,
        modified: u64,
        git_status: Option<&'static str>,
        score: i32,
    ) -> RawHit {
        RawHit {
            path,
            is_dir: false,
            size,
            modified,
            git_status,
            score,
        }
    }

    fn dir(path: PathBuf, score: i32) -> RawHit {
        RawHit {
            path,
            is_dir: true,
            size: 0,
            modified: 0,
            git_status: None,
            score,
        }
    }

    fn into_hit(self, needle: &str, show_hidden: bool) -> Option<SearchHit> {
        let entry = if self.is_dir {
            // Directory rows carry no metadata in the index.
            FileEntry::from_path(&self.path)?
        } else {
            let meta_modified = (self.modified > 0)
                .then(|| SystemTime::UNIX_EPOCH + Duration::from_secs(self.modified));
            let name = display_name(&self.path);
            FileEntry {
                kind: kind::classify(&self.path, false),
                hidden: name.starts_with('.'),
                name,
                is_dir: false,
                size: Some(self.size),
                modified: meta_modified,
                is_symlink: false,
                path: self.path,
            }
        };
        if entry.hidden && !show_hidden {
            return None;
        }
        Some(SearchHit {
            highlights: highlight(&entry.name, needle),
            entry,
            score: self.score,
            git_status: self
                .git_status
                .filter(|s| *s != "clean" && *s != "unknown")
                .map(str::to_owned),
        })
    }
}

/// Finder treats bundles (`.app`, `.framework`, …) as single items, so their
/// contents are hidden unless the search is scoped inside one.
fn inside_package(path: &Path, scope: Option<&Path>) -> bool {
    const PACKAGES: [&str; 7] = [
        ".app",
        ".framework",
        ".bundle",
        ".xcodeproj",
        ".xcworkspace",
        ".photoslibrary",
        ".plugin",
    ];
    let Some(parent) = path.parent() else {
        return false;
    };
    parent.ancestors().any(|dir| {
        if scope.is_some_and(|s| s.starts_with(dir)) {
            return false;
        }
        dir.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| PACKAGES.iter().any(|ext| n.ends_with(ext)))
    })
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|c| chars.any(|h| h == c))
}

/// Finder-style ranking on top of fff's score: words found in the file
/// name beat words found only in its folders, and shallower paths win ties.
fn rerank(hit: &RawHit, words: &[String]) -> i32 {
    if words.is_empty() {
        return hit.score;
    }
    let name = display_name(&hit.path).to_lowercase();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
    let mut bonus = 0;
    let mut in_name = 0;
    for word in words {
        if stem == word || name == *word {
            bonus += 120;
        } else if name.starts_with(word.as_str()) {
            bonus += 70;
        } else if let Some(pos) = name.find(word.as_str()) {
            let boundary = name[..pos]
                .chars()
                .last()
                .is_some_and(|c| !c.is_alphanumeric());
            bonus += if boundary { 55 } else { 40 };
        } else {
            continue;
        }
        in_name += 1;
    }
    if in_name == words.len() {
        bonus += 40;
    }
    let depth = hit.path.components().count() as i32;
    hit.score + bonus - depth * 3
}

/// fff returns `/`-separated relative paths on every OS.
fn join_rel(base: &Path, rel: &str) -> PathBuf {
    let rel = rel.trim_end_matches('/');
    let mut path = base.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        path.push(part);
    }
    path
}

/// The fuzzy words of a query, without constraint tokens.
fn fuzzy_text(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|t| {
            !t.starts_with('!')
                && !t.contains(['*', '?', '[', '{'])
                && !t.contains(':')
                && !t.starts_with('/')
                && !t.ends_with('/')
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Greedy case-insensitive subsequence match of `needle` in `name`, for highlighting.
pub fn highlight(name: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle: Vec<char> = needle
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let lower = name.to_lowercase();
    // Prefer a contiguous substring match when there is one.
    let joined: String = needle.iter().collect();
    if lower.len() == name.len()
        && let Some(start) = lower.find(&joined)
    {
        return vec![(start, start + joined.len())];
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut want = needle.iter().peekable();
    for (i, c) in name.char_indices() {
        let Some(&&w) = want.peek() else { break };
        if c.to_lowercase().eq(std::iter::once(w)) {
            let end = i + c.len_utf8();
            match ranges.last_mut() {
                Some(last) if last.1 == i => last.1 = end,
                _ => ranges.push((i, end)),
            }
            want.next();
        }
    }
    if want.peek().is_some() {
        // Typo-tolerant matches may not be subsequences; show no partial noise.
        return Vec::new();
    }
    ranges
}

fn normalize_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        let root = strip_trailing_separator(root);
        if !root.is_dir() {
            continue;
        }
        if out.iter().any(|r| root.starts_with(r)) {
            continue;
        }
        out.retain(|r| !r.starts_with(&root));
        out.push(root);
    }
    out
}

fn strip_trailing_separator(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    let trimmed = s.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || (trimmed.len() == 2 && trimmed.ends_with(':')) {
        path.to_path_buf()
    } else {
        PathBuf::from(trimmed)
    }
}

fn open_dbs(dir: Option<&Path>) -> (SharedFrecency, SharedQueryTracker) {
    let frecency = SharedFrecency::default();
    let queries = SharedQueryTracker::default();
    let Some(dir) = dir else {
        return (frecency, queries);
    };
    match FrecencyTracker::open(dir.join("frecency")) {
        Ok(tracker) => {
            if let Err(err) = frecency.init(tracker) {
                log::warn!("frecency db: {err}");
            }
        }
        Err(err) => log::warn!("frecency db: {err}"),
    }
    match QueryTracker::open(dir.join("queries")) {
        Ok(tracker) => {
            if let Err(err) = queries.init(tracker) {
                log::warn!("query db: {err}");
            }
        }
        Err(err) => log::warn!("query db: {err}"),
    }
    (frecency, queries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights() {
        assert_eq!(highlight("Project Atlas.txt", "proj"), vec![(0, 4)]);
        assert_eq!(highlight("main.rs", "mrs"), vec![(0, 1), (5, 7)]);
        assert!(highlight("abc", "xyz").is_empty());
    }

    #[test]
    fn roots_are_normalized() {
        let tmp = std::env::temp_dir();
        let nested = tmp.join(format!("fff-norm-{}", std::process::id()));
        std::fs::create_dir_all(&nested).unwrap();
        let roots = normalize_roots(&[nested.clone(), tmp.clone(), PathBuf::from("/nope/x")]);
        assert_eq!(roots, vec![strip_trailing_separator(&tmp)]);
        std::fs::remove_dir_all(nested).unwrap();
    }

    #[test]
    fn subsequence() {
        assert!(is_subsequence("mrs", "main.rs"));
        assert!(!is_subsequence("engine", "crates/nu_plugin_fff/cargo.toml"));
    }

    #[test]
    fn glob_finds_files_inside_bundles() {
        let dir = std::env::temp_dir().join(format!("fff-glob-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for app in ["One.app", "nested/Two.app"] {
            std::fs::create_dir_all(dir.join(app).join("Contents")).unwrap();
            std::fs::write(dir.join(app).join("Contents/Info.plist"), "<plist/>").unwrap();
        }
        std::fs::write(dir.join("Info.plist"), "<plist/>").unwrap();
        let engine = Engine::new(EngineOptions {
            roots: vec![dir.clone()],
            db_dir: None,
            watch: false,
        });
        assert!(engine.wait_ready(Duration::from_secs(20)));
        let found = engine.glob("**/*.app/Contents/Info.plist", 100);
        let dir = strip_trailing_separator(&dir);
        assert_eq!(
            found,
            vec![
                dir.join("One.app/Contents/Info.plist"),
                dir.join("nested/Two.app/Contents/Info.plist"),
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn package_contents_are_hidden() {
        let app = Path::new("/Applications/Xcode.app/Contents/README");
        assert!(inside_package(app, None));
        assert!(!inside_package(Path::new("/Applications/Xcode.app"), None));
        assert!(!inside_package(
            app,
            Some(Path::new("/Applications/Xcode.app"))
        ));
    }

    #[test]
    fn rerank_prefers_name_matches() {
        let words = vec!["project".to_string(), "atlas".to_string()];
        let deep = RawHit::file("/a/b/c/d/e/projected.h".into(), 1, 1, None, 78);
        let good = RawHit::file("/a/Project Atlas.txt".into(), 1, 1, None, 70);
        assert!(rerank(&good, &words) > rerank(&deep, &words));
    }

    #[test]
    fn fuzzy_text_drops_constraints() {
        assert_eq!(fuzzy_text("main *.rs /src/ !test git:m"), "main");
    }

    #[test]
    fn searches_a_folder() {
        let dir = std::env::temp_dir().join(format!("fff-engine-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs/notes")).unwrap();
        std::fs::write(dir.join("docs/notes/Project Atlas.txt"), "atlas").unwrap();
        std::fs::write(dir.join("docs/readme.md"), "hello").unwrap();
        std::fs::write(dir.join("photo.png"), [0u8; 4]).unwrap();
        let engine = Engine::new(EngineOptions {
            roots: vec![dir.clone()],
            db_dir: None,
            watch: false,
        });
        assert!(engine.wait_ready(Duration::from_secs(10)));

        let res = engine.search(&SearchRequest {
            query: "atlas".into(),
            limit: 10,
            ..Default::default()
        });
        assert_eq!(res.hits[0].entry.name, "Project Atlas.txt");

        let res = engine.search(&SearchRequest {
            query: "notes".into(),
            kind: KindFilter::Folder,
            limit: 10,
            ..Default::default()
        });
        assert!(
            res.hits
                .iter()
                .any(|h| h.entry.is_dir && h.entry.name == "notes")
        );

        let res = engine.search(&SearchRequest {
            query: String::new(),
            kind: KindFilter::Image,
            limit: 10,
            ..Default::default()
        });
        assert_eq!(res.hits.len(), 1);
        assert_eq!(res.hits[0].entry.name, "photo.png");

        let res = engine.search(&SearchRequest {
            query: "readme".into(),
            scope: Some(dir.join("docs")),
            limit: 10,
            ..Default::default()
        });
        assert_eq!(res.hits.len(), 1);

        let res = engine.grep(&GrepRequest {
            pattern: "hello".into(),
            limit: 10,
            ..Default::default()
        });
        assert_eq!(res.hits.len(), 1);
        assert_eq!(res.hits[0].line, 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
