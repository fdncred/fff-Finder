//! File classification used for icons, the "Kind" column, and kind filters.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Broad file category. Drives icon choice and the search "Kind" filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Folder,
    Application,
    Image,
    Video,
    Audio,
    Pdf,
    Archive,
    Code,
    Data,
    Text,
    Executable,
    Other,
}

impl FileKind {
    /// Asset path of the colored icon for this kind.
    pub fn icon_asset(self) -> &'static str {
        match self {
            FileKind::Folder => "filetypes/folder.svg",
            FileKind::Application => "filetypes/app.svg",
            FileKind::Image => "filetypes/file-image.svg",
            FileKind::Video => "filetypes/file-video.svg",
            FileKind::Audio => "filetypes/file-audio.svg",
            FileKind::Pdf => "filetypes/file-pdf.svg",
            FileKind::Archive => "filetypes/file-archive.svg",
            FileKind::Code => "filetypes/file-code.svg",
            FileKind::Data => "filetypes/file-data.svg",
            FileKind::Text => "filetypes/file-text.svg",
            FileKind::Executable => "filetypes/file-exec.svg",
            FileKind::Other => "filetypes/file.svg",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Folder => "folder",
            FileKind::Application => "application",
            FileKind::Image => "image",
            FileKind::Video => "video",
            FileKind::Audio => "audio",
            FileKind::Pdf => "pdf",
            FileKind::Archive => "archive",
            FileKind::Code => "code",
            FileKind::Data => "data",
            FileKind::Text => "text",
            FileKind::Executable => "executable",
            FileKind::Other => "other",
        }
    }
}

/// The search scope bar's "Kind" picker. Groups several [`FileKind`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindFilter {
    #[default]
    Any,
    Folder,
    Document,
    Image,
    Movie,
    Audio,
    Code,
    Archive,
}

impl KindFilter {
    pub const ALL: [KindFilter; 8] = [
        KindFilter::Any,
        KindFilter::Folder,
        KindFilter::Document,
        KindFilter::Image,
        KindFilter::Movie,
        KindFilter::Audio,
        KindFilter::Code,
        KindFilter::Archive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            KindFilter::Any => "Any Kind",
            KindFilter::Folder => "Folder",
            KindFilter::Document => "Document",
            KindFilter::Image => "Image",
            KindFilter::Movie => "Movie",
            KindFilter::Audio => "Audio",
            KindFilter::Code => "Code",
            KindFilter::Archive => "Archive",
        }
    }

    pub fn from_name(name: &str) -> Option<KindFilter> {
        Some(match name.to_ascii_lowercase().as_str() {
            "any" | "" => KindFilter::Any,
            "folder" | "dir" | "directory" => KindFilter::Folder,
            "document" | "doc" | "docs" => KindFilter::Document,
            "image" | "images" | "picture" => KindFilter::Image,
            "movie" | "video" | "videos" => KindFilter::Movie,
            "audio" | "music" | "sound" => KindFilter::Audio,
            "code" | "source" => KindFilter::Code,
            "archive" | "archives" | "zip" => KindFilter::Archive,
            _ => return None,
        })
    }

    pub fn matches(self, kind: FileKind) -> bool {
        match self {
            KindFilter::Any => true,
            KindFilter::Folder => kind == FileKind::Folder,
            KindFilter::Document => matches!(kind, FileKind::Pdf | FileKind::Text | FileKind::Data),
            KindFilter::Image => kind == FileKind::Image,
            KindFilter::Movie => kind == FileKind::Video,
            KindFilter::Audio => kind == FileKind::Audio,
            KindFilter::Code => kind == FileKind::Code,
            KindFilter::Archive => kind == FileKind::Archive,
        }
    }

    /// Extensions passed to fff as `*.ext` constraints (ORed together).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            KindFilter::Any | KindFilter::Folder => &[],
            KindFilter::Document => &[
                "pdf", "txt", "md", "markdown", "rtf", "doc", "docx", "odt", "pages", "csv", "tsv",
                "xlsx", "xls", "numbers", "ppt", "pptx", "key", "epub", "tex", "org", "rst",
                "json", "toml", "yaml", "yml", "xml",
            ],
            KindFilter::Image => &[
                "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "svg",
                "ico", "icns", "avif", "raw", "cr2", "nef", "psd",
            ],
            KindFilter::Movie => &[
                "mp4", "mov", "mkv", "avi", "webm", "m4v", "wmv", "flv", "mpg", "mpeg",
            ],
            KindFilter::Audio => &[
                "mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "aiff", "aif", "wma", "mid",
                "midi",
            ],
            KindFilter::Code => &[
                "rs", "c", "h", "cpp", "cc", "hpp", "cs", "go", "py", "rb", "js", "mjs", "ts",
                "tsx", "jsx", "java", "kt", "swift", "m", "zig", "lua", "php", "nu", "sh", "bash",
                "zsh", "fish", "ps1", "html", "css", "scss", "sql", "vue", "svelte", "dart", "ex",
                "hs", "ml", "scala", "jl",
            ],
            KindFilter::Archive => &[
                "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "dmg", "iso", "pkg",
                "deb", "rpm", "jar", "whl",
            ],
        }
    }

    /// Whether results for this filter can only be folders.
    pub fn wants_dirs(self) -> bool {
        self == KindFilter::Folder
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Classify a path. `is_dir` comes from metadata; app bundles are directories.
pub fn classify(path: &Path, is_dir: bool) -> FileKind {
    let ext = extension(path);
    if is_dir {
        return if ext == "app" {
            FileKind::Application
        } else {
            FileKind::Folder
        };
    }
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif"
        | "svg" | "ico" | "icns" | "avif" | "raw" | "cr2" | "nef" | "psd" => FileKind::Image,
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" | "wmv" | "flv" | "mpg" | "mpeg" => {
            FileKind::Video
        }
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "opus" | "aiff" | "aif" | "wma"
        | "mid" | "midi" => FileKind::Audio,
        "pdf" => FileKind::Pdf,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "dmg" | "iso"
        | "pkg" | "deb" | "rpm" | "jar" | "whl" | "crate" => FileKind::Archive,
        "rs" | "c" | "h" | "cpp" | "cc" | "hpp" | "cs" | "go" | "py" | "rb" | "js" | "mjs"
        | "cjs" | "ts" | "tsx" | "jsx" | "java" | "kt" | "swift" | "m" | "mm" | "zig" | "lua"
        | "php" | "pl" | "r" | "scala" | "ex" | "exs" | "erl" | "hs" | "ml" | "clj" | "dart"
        | "vue" | "svelte" | "html" | "htm" | "css" | "scss" | "sass" | "less" | "sql" | "nu"
        | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" | "vim" | "el" | "nim" | "jl"
        | "wgsl" | "glsl" | "metal" | "proto" | "graphql" | "cmake" | "mk" => FileKind::Code,
        "json" | "jsonl" | "nuon" | "csv" | "tsv" | "toml" | "yaml" | "yml" | "xml" | "ini"
        | "plist" | "sqlite" | "sqlite3" | "db" | "parquet" | "xlsx" | "xls" | "ods"
        | "numbers" | "lock" | "ron" | "env" | "conf" | "cfg" => FileKind::Data,
        "txt" | "md" | "markdown" | "rst" | "org" | "log" | "rtf" | "tex" | "doc" | "docx"
        | "odt" | "pages" | "epub" | "adoc" => FileKind::Text,
        "exe" | "msi" | "appimage" | "bin" | "run" | "command" | "dll" | "so" | "dylib" => {
            FileKind::Executable
        }
        "" if is_known_text_name(path) => FileKind::Text,
        _ => FileKind::Other,
    }
}

fn is_known_text_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    matches!(
        name,
        "README"
            | "LICENSE"
            | "COPYING"
            | "AUTHORS"
            | "CHANGELOG"
            | "Makefile"
            | "Dockerfile"
            | "Justfile"
            | "Gemfile"
            | "Rakefile"
            | "Procfile"
    )
}

/// Finder-style human description for the "Kind" column.
pub fn kind_label(path: &Path, is_dir: bool) -> String {
    let ext = extension(path);
    if is_dir {
        return match ext.as_str() {
            "app" => "Application".into(),
            "bundle" | "framework" => "Bundle".into(),
            _ => "Folder".into(),
        };
    }
    let label = match ext.as_str() {
        "" => {
            return if is_known_text_name(path) {
                "Plain Text".into()
            } else {
                "Document".into()
            };
        }
        "md" | "markdown" => "Markdown",
        "txt" => "Plain Text",
        "json" => "JSON",
        "jsonl" => "JSON Lines",
        "nuon" => "Nushell Object Notation",
        "nu" => "Nushell Script",
        "csv" => "Comma-Separated Values",
        "tsv" => "Tab-Separated Values",
        "toml" => "TOML Document",
        "yaml" | "yml" => "YAML Document",
        "xml" => "XML Document",
        "html" | "htm" => "HTML Document",
        "css" => "CSS Stylesheet",
        "rs" => "Rust Source",
        "py" => "Python Script",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" => "TypeScript",
        "tsx" | "jsx" => "React Component",
        "go" => "Go Source",
        "c" => "C Source",
        "h" | "hpp" => "C Header",
        "cpp" | "cc" => "C++ Source",
        "swift" => "Swift Source",
        "java" => "Java Source",
        "sh" | "bash" | "zsh" | "fish" => "Shell Script",
        "ps1" => "PowerShell Script",
        "lock" => "Lockfile",
        "log" => "Log File",
        "pdf" => "PDF Document",
        "png" => "PNG Image",
        "jpg" | "jpeg" => "JPEG Image",
        "gif" => "GIF Image",
        "webp" => "WebP Image",
        "svg" => "SVG Image",
        "heic" | "heif" => "HEIF Image",
        "tif" | "tiff" => "TIFF Image",
        "ico" | "icns" => "Icon",
        "mp4" | "m4v" => "MPEG-4 Movie",
        "mov" => "QuickTime Movie",
        "mkv" => "Matroska Video",
        "webm" => "WebM Video",
        "mp3" => "MP3 Audio",
        "wav" => "WAVE Audio",
        "flac" => "FLAC Audio",
        "m4a" | "aac" => "AAC Audio",
        "zip" => "ZIP Archive",
        "tar" => "Tar Archive",
        "gz" | "tgz" => "Gzip Archive",
        "dmg" => "Disk Image",
        "iso" => "Disc Image",
        "pkg" => "Installer Package",
        "exe" => "Windows Application",
        "msi" => "Windows Installer",
        "dylib" | "so" | "dll" => "Dynamic Library",
        "sqlite" | "sqlite3" | "db" => "SQLite Database",
        "xlsx" | "xls" => "Excel Spreadsheet",
        "docx" | "doc" => "Word Document",
        "pptx" | "ppt" => "PowerPoint Presentation",
        "plist" => "Property List",
        _ => return format!("{} File", ext.to_ascii_uppercase()),
    };
    label.into()
}

/// Short badge text shown on document icons (e.g. "JSON", "RS").
pub fn badge(path: &Path, is_dir: bool) -> Option<String> {
    if is_dir {
        return None;
    }
    let ext = extension(path);
    if ext.is_empty() || ext.len() > 5 {
        return None;
    }
    Some(ext.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_files() {
        assert_eq!(classify(Path::new("a/b.rs"), false), FileKind::Code);
        assert_eq!(classify(Path::new("Photo.JPG"), false), FileKind::Image);
        assert_eq!(
            classify(Path::new("Safari.app"), true),
            FileKind::Application
        );
        assert_eq!(classify(Path::new("src"), true), FileKind::Folder);
        assert_eq!(classify(Path::new("README"), false), FileKind::Text);
    }

    #[test]
    fn kind_filter_groups() {
        assert!(KindFilter::Document.matches(FileKind::Pdf));
        assert!(!KindFilter::Document.matches(FileKind::Image));
        assert!(KindFilter::Any.matches(FileKind::Folder));
        assert_eq!(KindFilter::from_name("movie"), Some(KindFilter::Movie));
    }

    #[test]
    fn labels() {
        assert_eq!(
            kind_label(Path::new("x.csv"), false),
            "Comma-Separated Values"
        );
        assert_eq!(kind_label(Path::new("x.qqq"), false), "QQQ File");
        assert_eq!(kind_label(Path::new("dir"), true), "Folder");
    }
}
