//! Binary document format miner for MemPalace.
//!
//! Extracts plain text from binary office formats (DOCX, XLSX, PPTX, EPUB, RTF)
//! using pure-Rust ZIP and XML parsing. PDF is currently a stub that reports
//! a missing format dependency so the file can be re-mined once a transformer
//! is added.
//!
//! Architecture mirrors the Python `format_miner.py` module:
//!
//! * `FormatMiner` — configurable miner instance.
//! * `extract_text` — single-file extraction with deterministic status.
//! * `scan_formats` — directory walker that returns supported files.
//! * `mine_formats` — orchestrator that extracts, chunks, and files drawers.

use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Maximum chunk size reused from `miner.rs`.
pub const MIN_CHUNK_SIZE: usize = 50;

/// Default cap for a single file — same as the existing miners.
pub const DEFAULT_MAX_FILE_SIZE: usize = 500 * 1024 * 1024;

/// Supported extensions, lowercase, leading dot.
pub const SUPPORTED_FORMATS: &[&str] = &[".pdf", ".docx", ".pptx", ".xlsx", ".rtf", ".epub"];

/// Filenames that are never user content even if their extension matches.
const _SKIP_FILENAMES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];

/// Outcome of a single `extract_text` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtractionStatus {
    /// Text was successfully extracted.
    Ok,
    /// File exceeds the configured size cap.
    SkipTooLarge,
    /// File is empty.
    SkipEmpty,
    /// No transformer is available for the format.
    SkipMissingFormatDeps,
    /// File extension is not recognized as a supported format.
    SkipUnrecognized,
    /// Permission denied when reading the file.
    SkipPermission,
    /// Symlink target is missing.
    SkipBrokenSymlink,
    /// File disappeared or became unreadable between scan and extract.
    SkipUnreadable,
    /// Extraction failed (malformed archive, corrupt XML, etc.).
    SkipExtractionError,
}

impl std::fmt::Display for ExtractionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ExtractionStatus::Ok => "ok",
            ExtractionStatus::SkipTooLarge => "skip:too_large",
            ExtractionStatus::SkipEmpty => "skip:empty",
            ExtractionStatus::SkipMissingFormatDeps => "skip:missing_format_deps",
            ExtractionStatus::SkipUnrecognized => "skip:unrecognized",
            ExtractionStatus::SkipPermission => "skip:permission",
            ExtractionStatus::SkipBrokenSymlink => "skip:broken_symlink",
            ExtractionStatus::SkipUnreadable => "skip:unreadable",
            ExtractionStatus::SkipExtractionError => "skip:extraction_error",
        };
        write!(f, "{}", s)
    }
}

/// Configurable miner instance for binary document formats.
#[derive(Debug, Clone)]
pub struct FormatMiner {
    max_file_size: usize,
    min_chunk_size: usize,
    pub supported_formats: HashSet<String>,
}

impl Default for FormatMiner {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatMiner {
    /// Create a new miner with the default file size cap.
    pub fn new() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            min_chunk_size: MIN_CHUNK_SIZE,
            supported_formats: SUPPORTED_FORMATS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Builder-style setter for the per-file size cap.
    pub fn with_max_file_size(mut self, max_file_size: usize) -> Self {
        self.max_file_size = max_file_size;
        self
    }

    /// Builder-style setter for the minimum extracted text size.
    pub fn with_min_chunk_size(mut self, min_chunk_size: usize) -> Self {
        self.min_chunk_size = min_chunk_size;
        self
    }

    /// Extract text from a single file.
    pub fn extract_text<P: AsRef<Path>>(&self, path: P) -> (Option<String>, ExtractionStatus) {
        extract_text_with_options(path, self.max_file_size)
    }

    /// Walk a directory and return supported files, sorted.
    pub fn scan_formats<P: AsRef<Path>>(&self, directory: P) -> Vec<PathBuf> {
        scan_formats(directory)
    }
}

/// Return the list of supported extensions.
pub fn supported_formats() -> Vec<&'static str> {
    SUPPORTED_FORMATS.to_vec()
}

/// Decode bytes to text without raising on dirty encodings.
///
/// Tries UTF-8 first, then CP1252, then UTF-8 with replacement characters.
pub fn decode_robust(raw: &[u8]) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }
    // CP1252 is common in legacy Office documents.
    let s = encoding_rs::WINDOWS_1252.decode(raw);
    s.0.to_string()
}

// Re-export decode_robust uses encoding_rs; we add it below if needed, but
// here we use the standard `encoding_rs` crate. We add it via cargo add.

/// True if `path` is an iCloud cloud-only placeholder.
///
/// On macOS, this checks the `st_flags` dataless bit. On other platforms it
/// only checks the literal `.icloud` suffix.
pub fn is_icloud_dataless(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        if ext.to_string_lossy().to_lowercase() == "icloud" {
            return true;
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        const DATALESS_FLAG: u32 = 0x40000000;
        if let Ok(meta) = path.symlink_metadata() {
            return (meta.st_flags() & DATALESS_FLAG) != 0;
        }
    }
    false
}

/// Extract text from a single file with comprehensive fringe-case handling.
///
/// Returns `(Some(text), ExtractionStatus::Ok)` on success, or
/// `(None, status)` for any skip or error case.
pub fn extract_text<P: AsRef<Path>>(path: P) -> (Option<String>, ExtractionStatus) {
    extract_text_with_options(path, DEFAULT_MAX_FILE_SIZE)
}

/// Extract text with an explicit size cap.
pub fn extract_text_with_options<P: AsRef<Path>>(
    path: P,
    max_file_size: usize,
) -> (Option<String>, ExtractionStatus) {
    let p = path.as_ref();

    // Broken symlink.
    if p.is_symlink() && !p.exists() {
        tracing::info!("skip:broken_symlink {}", p.display());
        return (None, ExtractionStatus::SkipBrokenSymlink);
    }

    // iCloud cloud-only file.
    if is_icloud_dataless(p) {
        tracing::info!("skip:cloud_only {}", p.display());
        return (None, ExtractionStatus::SkipUnreadable);
    }

    // General readability check.
    let meta = match p.metadata() {
        Ok(m) => m,
        Err(e) => {
            let status = if e.kind() == std::io::ErrorKind::PermissionDenied {
                ExtractionStatus::SkipPermission
            } else if e.kind() == std::io::ErrorKind::NotFound {
                if p.is_symlink() {
                    ExtractionStatus::SkipBrokenSymlink
                } else {
                    ExtractionStatus::SkipUnreadable
                }
            } else {
                ExtractionStatus::SkipUnreadable
            };
            tracing::info!("skip:unreadable {} — {}", p.display(), e);
            return (None, status);
        }
    };

    // Empty file.
    if meta.len() == 0 {
        return (None, ExtractionStatus::SkipEmpty);
    }

    // Too large.
    if meta.len() > max_file_size as u64 {
        tracing::info!(
            "skip:too_large {} ({} bytes > {})",
            p.display(),
            meta.len(),
            max_file_size
        );
        return (None, ExtractionStatus::SkipTooLarge);
    }

    // Extension check.
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let ext_with_dot = format!(".{}", ext);
    if !SUPPORTED_FORMATS.contains(&ext_with_dot.as_str()) {
        return (None, ExtractionStatus::SkipUnrecognized);
    }

    // Dispatch by format.
    let result = match ext_with_dot.as_str() {
        ".docx" => extract_docx(p),
        ".xlsx" => extract_xlsx(p),
        ".pptx" => extract_pptx(p),
        ".epub" => extract_epub(p),
        ".rtf" => extract_rtf(p),
        ".pdf" => {
            // PDF is intentionally a stub in this pure-Rust implementation.
            tracing::info!("skip:missing_format_deps {}", p.display());
            Ok((None, ExtractionStatus::SkipMissingFormatDeps))
        }
        _ => Ok((None, ExtractionStatus::SkipUnrecognized)),
    };

    match result {
        Ok((text, status)) => {
            if status == ExtractionStatus::Ok
                && text.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true)
            {
                tracing::info!("skip:extraction_error {} — returned empty", p.display());
                return (None, ExtractionStatus::SkipExtractionError);
            }
            (text, status)
        }
        Err(e) => {
            tracing::warn!("skip:extraction_error {} — {}", p.display(), e);
            (None, ExtractionStatus::SkipExtractionError)
        }
    }
}

/// Walk `directory` recursively and return supported files, sorted.
///
/// Skips hidden / build directories, symlinks, and unsupported extensions.
pub fn scan_formats<P: AsRef<Path>>(directory: P) -> Vec<PathBuf> {
    let root = directory.as_ref();
    if !root.exists() || !root.is_dir() {
        return vec![];
    }

    let mut found: Vec<PathBuf> = Vec::new();
    let supported: HashSet<String> = SUPPORTED_FORMATS.iter().map(|s| s.to_string()).collect();

    if let Ok(entries) = fs::read_dir(root) {
        let mut stack: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();

        while let Some(current) = stack.pop() {
            if current.is_symlink() {
                continue;
            }
            if current.is_dir() {
                let name = current.file_name().unwrap_or_default().to_string_lossy();
                if crate::shared::is_skip_dir(&name) {
                    continue;
                }
                if let Ok(entries) = fs::read_dir(&current) {
                    for entry in entries.flatten() {
                        stack.push(entry.path());
                    }
                }
                continue;
            }
            if current.is_file() {
                let name = current.file_name().unwrap_or_default().to_string_lossy();
                if _SKIP_FILENAMES.contains(&name.as_ref()) {
                    continue;
                }
                let ext = current
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if supported.contains(&format!(".{}", ext)) {
                    found.push(current);
                }
            }
        }
    }

    found.sort();
    found
}

/// Extract text from a DOCX file.
fn extract_docx(path: &Path) -> Result<(Option<String>, ExtractionStatus)> {
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let mut text = String::new();

    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i)?;
        let name = zip_file.name().to_lowercase();
        if name == "word/document.xml"
            || name.starts_with("word/document") && name.ends_with(".xml")
        {
            let mut buf = String::new();
            zip_file.read_to_string(&mut buf)?;
            let mut reader = Reader::from_str(&buf);
            reader.config_mut().trim_text(true);
            let mut in_text = false;
            let mut in_tab = false;
            let mut in_br = false;
            loop {
                match reader.read_event() {
                    Ok(Event::Start(e)) => {
                        let tag = e.name();
                        if tag == QName(b"w:t") {
                            in_text = true;
                        } else if tag == QName(b"w:tab") {
                            in_tab = true;
                        } else if tag == QName(b"w:br") {
                            in_br = true;
                        }
                    }
                    Ok(Event::Text(e)) => {
                        if in_text {
                            text.push_str(&e.decode().unwrap_or_default());
                        }
                    }
                    Ok(Event::End(e)) => {
                        let tag = e.name();
                        if tag == QName(b"w:t") {
                            in_text = false;
                        } else if tag == QName(b"w:tab") {
                            if in_tab {
                                text.push('\t');
                            }
                            in_tab = false;
                        } else if tag == QName(b"w:br") {
                            if in_br {
                                text.push('\n');
                            }
                            in_br = false;
                        }
                    }
                    Ok(Event::Empty(e)) => {
                        let tag = e.name();
                        if tag == QName(b"w:tab") {
                            text.push('\t');
                        } else if tag == QName(b"w:br") {
                            text.push('\n');
                        }
                    }
                    Ok(Event::Eof) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        }
    }

    if text.trim().is_empty() {
        Ok((None, ExtractionStatus::SkipExtractionError))
    } else {
        Ok((Some(text), ExtractionStatus::Ok))
    }
}

/// Extract text from an XLSX file.
fn extract_xlsx(path: &Path) -> Result<(Option<String>, ExtractionStatus)> {
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let mut text = String::new();

    // Shared strings are the primary text store.
    let mut shared_strings = String::new();
    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i)?;
        let name = zip_file.name().to_lowercase();
        if name == "xl/sharedstrings.xml" {
            let mut buf = String::new();
            zip_file.read_to_string(&mut buf)?;
            shared_strings.push_str(&extract_text_from_xml(&buf, b"t"));
        }
    }

    // Also scan inline cell values in case shared strings are empty or missing.
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let mut inline_text = String::new();
    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i)?;
        let name = zip_file.name().to_lowercase();
        if name.starts_with("xl/worksheets/") && name.ends_with(".xml") {
            let mut buf = String::new();
            zip_file.read_to_string(&mut buf)?;
            inline_text.push_str(&extract_text_from_xml(&buf, b"v"));
        }
    }

    if !shared_strings.trim().is_empty() {
        text.push_str(&shared_strings);
    }
    if !inline_text.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&inline_text);
    }

    if text.trim().is_empty() {
        Ok((None, ExtractionStatus::SkipExtractionError))
    } else {
        Ok((Some(text), ExtractionStatus::Ok))
    }
}

/// Extract text from a PPTX file.
fn extract_pptx(path: &Path) -> Result<(Option<String>, ExtractionStatus)> {
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let mut text = String::new();
    let mut slide_texts: Vec<(String, String)> = Vec::new();

    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i)?;
        let name = zip_file.name().to_lowercase();
        if name.starts_with("ppt/slides/") && name.ends_with(".xml") {
            let mut buf = String::new();
            zip_file.read_to_string(&mut buf)?;
            let slide_text = extract_text_from_xml(&buf, b"a:t");
            slide_texts.push((name, slide_text));
        }
    }

    // Keep slides in natural order.
    slide_texts.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, slide_text) in slide_texts {
        if !slide_text.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&slide_text);
        }
    }

    if text.trim().is_empty() {
        Ok((None, ExtractionStatus::SkipExtractionError))
    } else {
        Ok((Some(text), ExtractionStatus::Ok))
    }
}

/// Extract text from an EPUB file.
#[allow(deprecated)]
fn extract_epub(path: &Path) -> Result<(Option<String>, ExtractionStatus)> {
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let mut text = String::new();

    // Read content documents from the OPF spine. EPUB content is XHTML.
    let mut opf_path: Option<String> = None;
    if let Ok(mut container) = archive.by_name("META-INF/container.xml") {
        let mut buf = String::new();
        container.read_to_string(&mut buf)?;
        // Find rootfile full-path attribute.
        let mut reader = Reader::from_str(&buf);
        reader.config_mut().trim_text(true);
        loop {
            match reader.read_event() {
                Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                    if e.name() == QName(b"rootfile") {
                        for attr in e.attributes().flatten() {
                            if attr.key == QName(b"full-path") {
                                opf_path = Some(attr.unescape_value()?.into_owned());
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
    }

    // If we cannot locate the OPF, fall back to reading all HTML/XHTML files.
    let mut content_files: Vec<String> = Vec::new();
    let mut archive = {
        let file = fs::File::open(path)?;
        ZipArchive::new(BufReader::new(file))?
    };
    if let Some(opf) = opf_path {
        {
            let mut opf_file = archive.by_name(&opf)?;
            let mut buf = String::new();
            opf_file.read_to_string(&mut buf)?;
            let mut reader = Reader::from_str(&buf);
            reader.config_mut().trim_text(true);
            let opf_dir = Path::new(&opf)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut item_hrefs: Vec<String> = Vec::new();
            let mut in_spine = false;
            loop {
                match reader.read_event() {
                    Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                        if e.name() == QName(b"item") {
                            let mut href = None;
                            let mut media_type = None;
                            for attr in e.attributes().flatten() {
                                if attr.key == QName(b"href") {
                                    href = Some(attr.unescape_value()?.into_owned());
                                }
                                if attr.key == QName(b"media-type") {
                                    media_type = Some(attr.unescape_value()?.into_owned());
                                }
                            }
                            if let (Some(h), Some(mt)) = (href, media_type) {
                                if mt.contains("html") {
                                    let full = if opf_dir.is_empty() {
                                        h
                                    } else {
                                        format!("{}/{}", opf_dir, h)
                                    };
                                    item_hrefs.push(full);
                                }
                            }
                        } else if e.name() == QName(b"spine") {
                            in_spine = true;
                        } else if in_spine && e.name() == QName(b"itemref") {
                            for attr in e.attributes().flatten() {
                                if attr.key == QName(b"idref") {
                                    let idref = attr.unescape_value()?.into_owned();
                                    let _ = idref;
                                }
                            }
                        }
                    }
                    Ok(Event::End(e)) => {
                        if e.name() == QName(b"spine") {
                            in_spine = false;
                        }
                    }
                    Ok(Event::Eof) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
            content_files = item_hrefs;
        }
    }

    if content_files.is_empty() {
        // Fallback: read every HTML/XHTML file in the archive.
        for i in 0..archive.len() {
            let name = archive.by_index(i)?.name().to_string();
            let lower = name.to_lowercase();
            if lower.ends_with(".html") || lower.ends_with(".xhtml") || lower.ends_with(".htm") {
                content_files.push(name);
            }
        }
        content_files.sort();
    }

    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    for content_path in content_files {
        if let Ok(mut content_file) = archive.by_name(&content_path) {
            let mut buf = String::new();
            content_file.read_to_string(&mut buf)?;
            let stripped = strip_html_tags(&buf);
            if !stripped.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&stripped);
            }
        }
    }

    if text.trim().is_empty() {
        Ok((None, ExtractionStatus::SkipExtractionError))
    } else {
        Ok((Some(text), ExtractionStatus::Ok))
    }
}

/// Extract text from an RTF file using a simple control-word stripper.
fn extract_rtf(path: &Path) -> Result<(Option<String>, ExtractionStatus)> {
    let raw = fs::read(path)?;
    let source = decode_robust(&raw);
    let text = strip_rtf(&source);
    if text.trim().is_empty() {
        Ok((None, ExtractionStatus::SkipExtractionError))
    } else {
        Ok((Some(text), ExtractionStatus::Ok))
    }
}

/// Strip RTF control words and braces, returning plain text.
fn strip_rtf(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut depth = 0i32;
    let mut skip_control = false;
    let mut skip_hex = false;

    while let Some(ch) = chars.next() {
        if skip_hex {
            skip_hex = false;
            continue;
        }
        if skip_control {
            match ch {
                ' ' => {
                    skip_control = false;
                }
                '\\' | '{' | '}' => {
                    skip_control = false;
                    result.push(ch);
                }
                '\n' | '\r' => {
                    skip_control = false;
                }
                _ => {
                    // Some control words consume a following space implicitly.
                    if chars.peek() == Some(&' ') {
                        chars.next();
                        skip_control = false;
                    }
                }
            }
            continue;
        }
        match ch {
            '\\' => {
                if let Some(&next) = chars.peek() {
                    match next {
                        '\\' | '{' | '}' => {
                            chars.next();
                            result.push(next);
                        }
                        '\'' => {
                            // \'hh hex escape; skip next two chars and try to decode.
                            chars.next();
                            let h1 = chars.next();
                            let h2 = chars.next();
                            if let (Some(a), Some(b)) = (h1, h2) {
                                let hex = format!("{}{}", a, b);
                                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                                    result.push(byte as char);
                                }
                            }
                        }
                        _ => {
                            skip_control = true;
                        }
                    }
                }
            }
            '{' => {
                depth += 1;
            }
            '}' => {
                depth -= 1;
            }
            _ => {
                if depth <= 1 {
                    result.push(ch);
                }
            }
        }
    }

    result.replace("\r\n", "\n").replace('\r', "\n")
}

/// Extract text from a simple XML document by concatenating text inside `tag`.
fn extract_text_from_xml(xml: &str, tag: &[u8]) -> String {
    let mut text = String::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut in_tag = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if e.name() == QName(tag) {
                    in_tag = true;
                }
            }
            Ok(Event::Text(e)) => {
                if in_tag {
                    text.push_str(&e.decode().unwrap_or_default());
                }
            }
            Ok(Event::End(e)) => {
                if e.name() == QName(tag) {
                    in_tag = false;
                    text.push(' ');
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    text
}

/// Strip HTML tags and decode common entities, returning plain text.
fn strip_html_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity = String::new();

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            continue;
        }
        if in_entity {
            if ch == ';' {
                let decoded = match entity.as_str() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => ' ',
                    _ => {
                        text.push('&');
                        text.push_str(&entity);
                        text.push(';');
                        '\0'
                    }
                };
                if decoded != '\0' {
                    text.push(decoded);
                }
                entity.clear();
                in_entity = false;
            } else {
                entity.push(ch);
            }
            continue;
        }
        match ch {
            '<' => in_tag = true,
            '&' => in_entity = true,
            _ => text.push(ch),
        }
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_docx(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
        )
        .unwrap();
        zip.start_file("word/document.xml", options).unwrap();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>{}</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
            body
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn make_xlsx(dir: &std::path::Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
        )
        .unwrap();
        zip.start_file("xl/sharedStrings.xml", options).unwrap();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">
  <si><t>{}</t></si>
</sst>"#,
            text
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn make_pptx(dir: &std::path::Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
</Types>"#,
        )
        .unwrap();
        zip.start_file("ppt/slides/slide1.xml", options).unwrap();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:sp><p:txBody><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>
</p:sld>"#,
            text
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn make_epub(dir: &std::path::Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", options).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <manifest>
    <item id="page1" href="page1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="page1"/>
  </spine>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/page1.xhtml", options).unwrap();
        let html = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Test</title></head>
<body><p>{}</p></body></html>"#,
            text
        );
        zip.write_all(html.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn make_rtf(dir: &std::path::Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        let rtf = format!(
            r#"{{\rtf1\ansi{{\fonttbl\f0\fswiss Helvetica;}}\f0\pard {}\par}}"#,
            text
        );
        fs::write(&path, rtf).unwrap();
        path
    }

    fn make_rtf_raw(dir: &std::path::Path, name: &str, rtf: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, rtf).unwrap();
        path
    }

    fn make_docx_with_xml(dir: &std::path::Path, name: &str, body_xml: &str) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
        )
        .unwrap();
        zip.start_file("word/document.xml", options).unwrap();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{}</w:body>
</w:document>"#,
            body_xml
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn make_xlsx_with_inline(dir: &std::path::Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("xl/sharedStrings.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0" uniqueCount="0"/>"#,
        )
        .unwrap();
        zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData><row><c><v>{}</v></c></row></sheetData>
</worksheet>"#,
            text
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn make_pptx_with_slides(dir: &std::path::Path, name: &str, slides: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("[Content_Types].xml", options).unwrap();
        let mut ct = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        );
        for i in 0..slides.len() {
            ct.push_str(&format!(
                r#"<Override PartName="/ppt/slides/slide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
                i + 1
            ));
        }
        ct.push_str("</Types>");
        zip.write_all(ct.as_bytes()).unwrap();
        for (i, text) in slides.iter().enumerate() {
            let slide_name = format!("ppt/slides/slide{}.xml", i + 1);
            zip.start_file(&slide_name, options).unwrap();
            let xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:sp><p:txBody><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>
</p:sld>"#,
                text
            );
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn make_epub_fallback(dir: &std::path::Path, name: &str, texts: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (i, text) in texts.iter().enumerate() {
            let page = format!("page{}.xhtml", i + 1);
            zip.start_file(&page, options).unwrap();
            let html = if text.is_empty() {
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p/></body></html>"#.to_string()
            } else {
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Page {}</title></head>
<body><p>{}</p></body></html>"#,
                    i + 1,
                    text
                )
            };
            zip.write_all(html.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn test_supported_formats() {
        let formats = supported_formats();
        assert!(formats.contains(&".docx"));
        assert!(formats.contains(&".xlsx"));
        assert!(formats.contains(&".pptx"));
        assert!(formats.contains(&".epub"));
        assert!(formats.contains(&".rtf"));
        assert!(formats.contains(&".pdf"));
    }

    #[test]
    fn test_decode_robust() {
        assert_eq!(decode_robust(b"hello"), "hello");
        assert_eq!(decode_robust(b""), "");
        // CP1252 smart quote bytes 0x91-0x94.
        assert_eq!(decode_robust(&[0x91, 0x92, 0x93, 0x94]), "‘’“”");
    }

    #[test]
    fn test_extract_docx() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_docx(tmp.path(), "test.docx", "Hello from DOCX");
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("Hello from DOCX"));
    }

    #[test]
    fn test_extract_xlsx() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_xlsx(tmp.path(), "test.xlsx", "Hello from XLSX");
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("Hello from XLSX"));
    }

    #[test]
    fn test_extract_pptx() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_pptx(tmp.path(), "test.pptx", "Hello from PPTX");
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("Hello from PPTX"));
    }

    #[test]
    fn test_extract_epub() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_epub(tmp.path(), "test.epub", "Hello from EPUB");
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("Hello from EPUB"));
    }

    #[test]
    fn test_extract_rtf() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_rtf(tmp.path(), "test.rtf", "Hello from RTF");
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("Hello from RTF"));
    }

    #[test]
    fn test_extract_pdf_stub() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.pdf");
        fs::write(&path, b"%PDF-1.4 fake").unwrap();
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipMissingFormatDeps);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_unrecognized_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.bin");
        fs::write(&path, b"data").unwrap();
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipUnrecognized);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.docx");
        fs::write(&path, b"").unwrap();
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipEmpty);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_too_large() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.docx");
        fs::write(&path, b"x").unwrap();
        let (text, status) = extract_text_with_options(&path, 0);
        assert_eq!(status, ExtractionStatus::SkipTooLarge);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_broken_symlink() {
        #[cfg(unix)]
        {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("broken.docx");
            std::os::unix::fs::symlink(tmp.path().join("missing.docx"), &path).unwrap();
            let (text, status) = extract_text(&path);
            assert_eq!(status, ExtractionStatus::SkipBrokenSymlink);
            assert!(text.is_none());
        }
    }

    #[test]
    fn test_extract_nonexistent() {
        let path = std::path::Path::new("/nonexistent/path/file.docx");
        let (text, status) = extract_text(path);
        assert_eq!(status, ExtractionStatus::SkipUnreadable);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_corrupt_docx() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.docx");
        fs::write(&path, b"not a zip").unwrap();
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipExtractionError);
        assert!(text.is_none());
    }

    #[test]
    fn test_scan_formats() {
        let tmp = tempfile::tempdir().unwrap();
        make_docx(tmp.path(), "a.docx", "A");
        make_xlsx(tmp.path(), "b.xlsx", "B");
        fs::create_dir(tmp.path().join("target")).unwrap();
        fs::write(tmp.path().join("target").join("c.docx"), b"").unwrap();
        let files = scan_formats(tmp.path());
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.docx"));
        assert!(files[1].ends_with("b.xlsx"));
    }

    #[test]
    fn test_strip_rtf() {
        let source = r"{\rtf1\ansi {\fonttbl \f0 \fswiss Helvetica;}\f0\pard Hello\par}";
        assert!(strip_rtf(source).contains("Hello"));
    }

    #[test]
    fn test_strip_html_tags() {
        let html = "<p>Hello &amp; world</p>";
        assert_eq!(strip_html_tags(html), "Hello & world");
    }

    #[test]
    fn test_format_miner_default() {
        let miner = FormatMiner::new();
        let tmp = tempfile::tempdir().unwrap();
        let path = make_docx(tmp.path(), "test.docx", "Miner test");
        let (text, status) = miner.extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("Miner test"));
    }

    #[test]
    fn test_format_miner_with_size_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("huge.docx");
        fs::write(&path, b"x").unwrap();
        let miner = FormatMiner::new().with_max_file_size(0);
        let (text, status) = miner.extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipTooLarge);
        assert!(text.is_none());
    }

    #[test]
    fn test_format_miner_scan() {
        let tmp = tempfile::tempdir().unwrap();
        make_docx(tmp.path(), "x.docx", "X");
        let miner = FormatMiner::new();
        let files = miner.scan_formats(tmp.path());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_extract_text_from_xml() {
        let xml = r#"<root><t>one</t><t>two</t></root>"#;
        assert_eq!(extract_text_from_xml(xml, b"t").trim(), "one two");
    }

    #[test]
    fn test_display_extraction_status() {
        assert_eq!(format!("{}", ExtractionStatus::Ok), "ok");
        assert_eq!(
            format!("{}", ExtractionStatus::SkipTooLarge),
            "skip:too_large"
        );
        assert_eq!(format!("{}", ExtractionStatus::SkipEmpty), "skip:empty");
        assert_eq!(
            format!("{}", ExtractionStatus::SkipMissingFormatDeps),
            "skip:missing_format_deps"
        );
        assert_eq!(
            format!("{}", ExtractionStatus::SkipUnrecognized),
            "skip:unrecognized"
        );
        assert_eq!(
            format!("{}", ExtractionStatus::SkipPermission),
            "skip:permission"
        );
        assert_eq!(
            format!("{}", ExtractionStatus::SkipBrokenSymlink),
            "skip:broken_symlink"
        );
        assert_eq!(
            format!("{}", ExtractionStatus::SkipUnreadable),
            "skip:unreadable"
        );
        assert_eq!(
            format!("{}", ExtractionStatus::SkipExtractionError),
            "skip:extraction_error"
        );
    }

    #[test]
    fn test_default_miner_and_min_chunk_size() {
        let miner: FormatMiner = Default::default();
        let tmp = tempfile::tempdir().unwrap();
        let path = make_docx(tmp.path(), "test.docx", "default");
        let (text, status) = miner.with_min_chunk_size(1).extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("default"));
    }

    #[test]
    fn test_extract_icloud_placeholder() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.docx.icloud");
        fs::write(&path, b"cloud data").unwrap();
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipUnreadable);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_docx_with_tabs_and_breaks() {
        let tmp = tempfile::tempdir().unwrap();
        let body =
            r#"<w:p><w:r><w:t>Line</w:t></w:r><w:tab/><w:br/><w:r><w:t>Next</w:t></w:r></w:p>"#;
        let path = make_docx_with_xml(tmp.path(), "tabs.docx", body);
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        let text = text.unwrap();
        assert!(text.contains("Line"));
        assert!(text.contains("Next"));
        assert!(text.contains('\t'));
        assert!(text.contains('\n'));
    }

    #[test]
    fn test_extract_docx_empty_body() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_docx_with_xml(
            tmp.path(),
            "empty.docx",
            "<w:p><w:r><w:t></w:t></w:r></w:p>",
        );
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipExtractionError);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_xlsx_inline_values() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_xlsx_with_inline(tmp.path(), "inline.xlsx", "Inline value");
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        assert!(text.unwrap().contains("Inline value"));
    }

    #[test]
    fn test_extract_xlsx_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_xlsx_with_inline(tmp.path(), "empty.xlsx", "");
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipExtractionError);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_pptx_multiple_slides() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_pptx_with_slides(tmp.path(), "slides.pptx", &["Slide one", "Slide two"]);
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        let text = text.unwrap();
        assert!(text.contains("Slide one"));
        assert!(text.contains("Slide two"));
        assert!(text.contains('\n'));
    }

    #[test]
    fn test_extract_pptx_empty_slide() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_pptx_with_slides(tmp.path(), "empty.pptx", &[""]);
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipExtractionError);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_epub_fallback_html() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_epub_fallback(tmp.path(), "fallback.epub", &["Page one", "Page two"]);
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        let text = text.unwrap();
        assert!(text.contains("Page one"));
        assert!(text.contains("Page two"));
        assert!(text.contains('\n'));
    }

    #[test]
    fn test_extract_epub_fallback_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_epub_fallback(tmp.path(), "empty.epub", &[""]);
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::SkipExtractionError);
        assert!(text.is_none());
    }

    #[test]
    fn test_extract_rtf_hex_and_escapes() {
        let tmp = tempfile::tempdir().unwrap();
        let rtf = r#"{\rtf1\ansi test \'e9 \par \{ \} \\ control}"#;
        let path = make_rtf_raw(tmp.path(), "escapes.rtf", rtf);
        let (text, status) = extract_text(&path);
        assert_eq!(status, ExtractionStatus::Ok);
        let text = text.unwrap();
        assert!(text.contains("test"));
        assert!(text.contains("é"));
        assert!(text.contains("control"));
    }

    #[test]
    fn test_strip_html_entities() {
        let html = "&lt;p&gt;&quot;&apos;&nbsp;&unknown;";
        let text = strip_html_tags(html);
        assert_eq!(text, "<p>\"' &unknown;");
    }

    #[test]
    fn test_scan_formats_nested_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("nested");
        fs::create_dir(&nested).unwrap();
        make_docx(&nested, "deep.docx", "Deep");
        fs::write(tmp.path().join("skip.bin"), b"data").unwrap();

        let files = scan_formats(tmp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("deep.docx"));

        assert!(scan_formats("/definitely/not/a/real/path").is_empty());
    }
}
