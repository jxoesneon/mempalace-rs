//! Scan project directories for manifests, project names, people, dependencies,
//! and language/framework hints.
//!
//! This is the Rust port of the Python `project_scanner` module. It uses only
//! local file parsing and `git log` — no external LLM APIs or network calls.

use crate::models::{DetectedEntity, EntityType};
use crate::shared::is_skip_dir;
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Maximum recursion depth when walking a directory tree.
const MAX_DEPTH: usize = 6;
/// Maximum number of git commits to inspect per repository.
const MAX_COMMITS_PER_REPO: usize = 1000;
/// Timeout for individual `git` invocations in seconds.
/// Known manifest file names and their relative priority (lower is stronger).
const MANIFEST_PRIORITY: &[(&str, i32)] = &[
    ("pyproject.toml", 0),
    ("package.json", 1),
    ("Cargo.toml", 2),
    ("go.mod", 3),
    ("pom.xml", 4),
    ("settings.gradle", 5),
    ("settings.gradle.kts", 6),
    ("build.gradle", 7),
    ("build.gradle.kts", 8),
    ("requirements.txt", 9),
    ("setup.py", 10),
    ("Gemfile", 11),
];

/// Java-family manifests that can describe sub-modules.
const JAVA_MANIFESTS: &[&str] = &[
    "pom.xml",
    "settings.gradle",
    "settings.gradle.kts",
    "build.gradle",
    "build.gradle.kts",
];

/// High-level project type inferred from the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Java,
    Ruby,
    Unknown,
}

impl ProjectType {
    /// Human-readable label for the project type.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectType::Rust => "rust",
            ProjectType::Node => "node",
            ProjectType::Python => "python",
            ProjectType::Go => "go",
            ProjectType::Java => "java",
            ProjectType::Ruby => "ruby",
            ProjectType::Unknown => "unknown",
        }
    }

    /// Primary programming language for the project type.
    pub fn language(&self) -> &'static str {
        match self {
            ProjectType::Rust => "Rust",
            ProjectType::Node => "JavaScript/TypeScript",
            ProjectType::Python => "Python",
            ProjectType::Go => "Go",
            ProjectType::Java => "Java/Kotlin",
            ProjectType::Ruby => "Ruby",
            ProjectType::Unknown => "Unknown",
        }
    }
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Information about a detected project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectInfo {
    pub name: String,
    pub repo_root: PathBuf,
    pub manifest: Option<String>,
    pub project_type: ProjectType,
    pub language: String,
    pub framework: Option<String>,
    pub authors: Vec<String>,
    pub dependencies: Vec<String>,
    pub has_git: bool,
    pub total_commits: usize,
    pub user_commits: usize,
    pub is_mine: bool,
}

impl ProjectInfo {
    /// Confidence score for the detection.
    pub fn confidence(&self) -> f32 {
        if self.is_mine {
            0.99
        } else if self.has_git && self.total_commits > 0 {
            0.7
        } else {
            0.85
        }
    }

    /// Short human-readable signal describing why this project was detected.
    pub fn signal(&self) -> String {
        let mut parts = Vec::new();
        if let Some(m) = &self.manifest {
            parts.push(m.to_string());
        }
        if let Some(t) = &self.framework {
            parts.push(t.to_string());
        }
        if self.has_git {
            if self.is_mine && self.user_commits > 0 {
                parts.push(format!("{} of your commits", self.user_commits));
            } else if self.user_commits > 0 {
                parts.push(format!("{}/{}", self.user_commits, self.total_commits));
            } else {
                parts.push(format!("{} commits", self.total_commits));
            }
        }
        if parts.is_empty() {
            "project".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Convert this project into a `DetectedEntity` for the registry.
    pub fn to_detected_entity(&self) -> DetectedEntity {
        DetectedEntity {
            name: self.name.clone(),
            unique_id: None,
            r#type: EntityType::Project,
            confidence: self.confidence(),
            signals: vec![self.signal()],
            aliases: vec![],
            relationship: None,
        }
    }
}

/// Information about a detected person from git history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonInfo {
    pub name: String,
    pub total_commits: usize,
    pub emails: Vec<String>,
    pub repos: Vec<String>,
}

impl PersonInfo {
    /// Confidence score for the person detection.
    pub fn confidence(&self) -> f32 {
        if self.total_commits >= 100 || self.repos.len() >= 3 {
            0.99
        } else if self.total_commits >= 20 {
            0.85
        } else {
            0.65
        }
    }

    /// Short human-readable signal describing the person's commit activity.
    pub fn signal(&self) -> String {
        format!(
            "{} commit{} across {} repo{}",
            self.total_commits,
            if self.total_commits == 1 { "" } else { "s" },
            self.repos.len(),
            if self.repos.len() == 1 { "" } else { "s" }
        )
    }

    /// Convert this person into a `DetectedEntity` for the registry.
    pub fn to_detected_entity(&self) -> DetectedEntity {
        DetectedEntity {
            name: self.name.clone(),
            unique_id: None,
            r#type: EntityType::Person,
            confidence: self.confidence(),
            signals: vec![self.signal()],
            aliases: self.emails.clone(),
            relationship: None,
        }
    }
}

/// Combined result of a project/people scan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScanResult {
    pub projects: Vec<ProjectInfo>,
    pub people: Vec<PersonInfo>,
}

impl ScanResult {
    /// Convert scan results into a flat list of `DetectedEntity` objects.
    pub fn to_detected_entities(
        &self,
        project_cap: usize,
        people_cap: usize,
    ) -> Vec<DetectedEntity> {
        let mut entities: Vec<DetectedEntity> = Vec::new();
        entities.extend(
            self.projects
                .iter()
                .take(project_cap)
                .map(ProjectInfo::to_detected_entity),
        );
        entities.extend(
            self.people
                .iter()
                .take(people_cap)
                .map(PersonInfo::to_detected_entity),
        );
        entities
    }

    /// Sort projects by ownership, then user commits, then total commits, then name.
    pub fn sort_projects(&mut self) {
        self.projects.sort_by(|a, b| {
            let a_key = (
                !a.is_mine,
                -(a.user_commits as i64),
                -(a.total_commits as i64),
                &a.name,
            );
            let b_key = (
                !b.is_mine,
                -(b.user_commits as i64),
                -(b.total_commits as i64),
                &b.name,
            );
            a_key.cmp(&b_key)
        });
    }

    /// Sort people by total commits descending.
    pub fn sort_people(&mut self) {
        self.people
            .sort_by_key(|b| std::cmp::Reverse(b.total_commits));
    }
}

/// Detect the project type from a manifest file name.
pub fn detect_project_type(manifest: &str) -> ProjectType {
    match manifest {
        "Cargo.toml" => ProjectType::Rust,
        "package.json" => ProjectType::Node,
        "pyproject.toml" | "setup.py" | "requirements.txt" => ProjectType::Python,
        "go.mod" => ProjectType::Go,
        "pom.xml"
        | "settings.gradle"
        | "settings.gradle.kts"
        | "build.gradle"
        | "build.gradle.kts" => ProjectType::Java,
        "Gemfile" => ProjectType::Ruby,
        _ => ProjectType::Unknown,
    }
}

/// Scan a single directory for projects and people.
///
/// Returns a `ScanResult` containing detected projects (from manifest files)
/// and people (from git commit history). The scan is non-recursive beyond
/// `MAX_DEPTH` and respects `SKIP_DIRS`.
pub fn scan_project<P: AsRef<Path>>(root: P) -> Result<ScanResult> {
    let root = root
        .as_ref()
        .canonicalize()
        .unwrap_or_else(|_| root.as_ref().to_path_buf());
    if !root.is_dir() {
        return Ok(ScanResult::default());
    }

    let repos = find_git_repos(&root);
    let (me_name, me_email) = git_identity(&repos);

    let mut projects: HashMap<String, ProjectInfo> = HashMap::new();
    let mut all_commits: Vec<(String, String, String)> = Vec::new();

    for repo in &repos {
        let manifests = collect_manifest_names(repo);
        let root_manifest = manifests.iter().find(|(_, _, dir)| dir == repo).cloned();

        let (manifest_file, proj_name) = if let Some((m, n, _)) = root_manifest.as_ref() {
            (Some(m.clone()), n.clone())
        } else {
            (None, repo_name(repo))
        };
        let extra_manifests: Vec<(String, String, PathBuf)> = manifests
            .into_iter()
            .filter(|entry| root_manifest.as_ref().map(|r| entry != r).unwrap_or(true))
            .collect();

        let authors = git_authors(repo);
        let non_bot_authors: Vec<(String, String)> = authors
            .into_iter()
            .filter(|(name, email)| !is_bot(name, email))
            .collect();
        let total_commits = non_bot_authors.len();
        let mut user_commits = 0;
        let mut author_counts: HashMap<String, usize> = HashMap::new();
        for (name, email) in &non_bot_authors {
            *author_counts.entry(name.clone()).or_insert(0) += 1;
            all_commits.push((
                name.clone(),
                email.clone(),
                repo.to_string_lossy().to_string(),
            ));
            if (me_name.as_ref().is_some_and(|n| n == name))
                || (me_email.as_ref().is_some_and(|e| e == email))
            {
                user_commits += 1;
            }
        }

        let is_mine = if user_commits > 0 {
            let mut sorted_authors: Vec<(String, usize)> = author_counts.into_iter().collect();
            sorted_authors.sort_by_key(|b| std::cmp::Reverse(b.1));
            let top5: HashSet<String> = sorted_authors
                .iter()
                .take(5)
                .map(|(n, _)| n.clone())
                .collect();
            if me_name.as_ref().is_some_and(|n| top5.contains(n))
                || (total_commits > 0 && user_commits as f64 / total_commits as f64 >= 0.10)
            {
                true
            } else {
                user_commits >= 20
            }
        } else {
            false
        };

        let (project_type, framework, dependencies, authors) = if let Some(ref mf) = manifest_file {
            let manifest_path = repo.join(mf);
            let manifest_text = std::fs::read_to_string(&manifest_path).unwrap_or_default();
            parse_manifest(mf, &manifest_text)
        } else {
            (ProjectType::Unknown, None, Vec::new(), Vec::new())
        };

        let proj = ProjectInfo {
            name: proj_name.clone(),
            repo_root: repo.clone(),
            manifest: manifest_file.clone(),
            project_type: project_type.clone(),
            language: project_type.language().to_string(),
            framework,
            authors,
            dependencies,
            has_git: true,
            total_commits,
            user_commits,
            is_mine,
        };

        let existing = projects.get(&proj_name);
        if existing.is_none() || proj.user_commits > existing.unwrap().user_commits {
            projects.insert(proj_name, proj);
        }

        for (extra_manifest, extra_name, extra_dir) in extra_manifests {
            if !JAVA_MANIFESTS.contains(&extra_manifest.as_str()) {
                continue;
            }
            let existing = projects.get(&extra_name);
            if existing.is_some_and(|e| e.manifest.is_some() || e.repo_root != *repo) {
                continue;
            }
            let (pt, fw, deps, auth) = {
                let p = extra_dir.join(&extra_manifest);
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                parse_manifest(&extra_manifest, &text)
            };
            projects.insert(
                extra_name.clone(),
                ProjectInfo {
                    name: extra_name,
                    repo_root: extra_dir,
                    manifest: Some(extra_manifest),
                    project_type: pt.clone(),
                    language: pt.language().to_string(),
                    framework: fw,
                    authors: auth,
                    dependencies: deps,
                    has_git: true,
                    total_commits,
                    user_commits,
                    is_mine,
                },
            );
        }
    }

    let people = dedupe_people(all_commits);

    // Handle root with manifests but no git repo.
    if repos.is_empty() {
        let manifests = collect_manifest_names(&root);
        for (manifest_file, proj_name, dirpath) in manifests {
            if projects.contains_key(&proj_name) {
                continue;
            }
            let (project_type, framework, dependencies, authors) = {
                let p = dirpath.join(&manifest_file);
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                parse_manifest(&manifest_file, &text)
            };
            projects.insert(
                proj_name.clone(),
                ProjectInfo {
                    name: proj_name,
                    repo_root: dirpath,
                    manifest: Some(manifest_file),
                    project_type: project_type.clone(),
                    language: project_type.language().to_string(),
                    framework,
                    authors,
                    dependencies,
                    has_git: false,
                    total_commits: 0,
                    user_commits: 0,
                    is_mine: false,
                },
            );
        }
    }

    let mut result = ScanResult {
        projects: projects.into_values().collect(),
        people: people.into_values().collect(),
    };
    result.sort_projects();
    result.sort_people();
    Ok(result)
}

/// Scan multiple root directories and merge the results.
pub fn scan_projects<P: AsRef<Path>>(roots: &[P]) -> Result<ScanResult> {
    let mut merged = ScanResult::default();
    let mut project_names: HashSet<String> = HashSet::new();
    let mut person_names: HashMap<String, PersonInfo> = HashMap::new();

    for root in roots {
        let partial = scan_project(root)?;
        for proj in partial.projects {
            if project_names.insert(proj.name.clone()) {
                merged.projects.push(proj);
            }
        }
        for person in partial.people {
            let entry = person_names
                .entry(person.name.clone())
                .or_insert_with(|| PersonInfo {
                    name: person.name.clone(),
                    total_commits: 0,
                    emails: Vec::new(),
                    repos: Vec::new(),
                });
            entry.total_commits += person.total_commits;
            for email in &person.emails {
                if !entry.emails.contains(email) {
                    entry.emails.push(email.clone());
                }
            }
            for repo in &person.repos {
                if !entry.repos.contains(repo) {
                    entry.repos.push(repo.clone());
                }
            }
        }
    }

    merged.people = person_names.into_values().collect();
    merged.sort_projects();
    merged.sort_people();
    Ok(merged)
}

/// Parse a manifest file and return (project_type, framework, dependencies, authors).
fn parse_manifest(
    manifest: &str,
    text: &str,
) -> (ProjectType, Option<String>, Vec<String>, Vec<String>) {
    let project_type = detect_project_type(manifest);
    let (framework, dependencies, authors) = match manifest {
        "package.json" => parse_package_json(text),
        "Cargo.toml" => parse_cargo_toml(text),
        "pyproject.toml" => parse_pyproject(text),
        "go.mod" => parse_go_mod(text),
        "pom.xml" => parse_pom_xml(text),
        "settings.gradle" | "settings.gradle.kts" | "build.gradle" | "build.gradle.kts" => {
            parse_gradle(text)
        }
        "requirements.txt" => parse_requirements(text),
        "setup.py" => parse_setup_py(text),
        "Gemfile" => parse_gemfile(text),
        _ => (None, Vec::new(), Vec::new()),
    };
    (project_type, framework, dependencies, authors)
}

fn parse_package_json(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let data: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return (None, Vec::new(), Vec::new()),
    };

    let framework = detect_framework(&data);

    let mut deps = Vec::new();
    for key in &["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(obj) = data.get(key).and_then(|v| v.as_object()) {
            for name in obj.keys() {
                if !deps.contains(name) {
                    deps.push(name.clone());
                }
            }
        }
    }

    let mut authors = Vec::new();
    if let Some(author) = data.get("author") {
        if let Some(a) = author.as_str() {
            authors.push(a.to_string());
        } else if let Some(name) = author.get("name").and_then(|v| v.as_str()) {
            authors.push(name.to_string());
        }
    }
    if let Some(arr) = data.get("contributors").and_then(|v| v.as_array()) {
        for c in arr {
            if let Some(s) = c.as_str() {
                if !authors.contains(&s.to_string()) {
                    authors.push(s.to_string());
                }
            } else if let Some(name) = c.get("name").and_then(|v| v.as_str()) {
                if !authors.contains(&name.to_string()) {
                    authors.push(name.to_string());
                }
            }
        }
    }

    (framework, deps, authors)
}

fn detect_framework(data: &serde_json::Value) -> Option<String> {
    if data
        .get("dependencies")
        .and_then(|d| d.get("react"))
        .is_some()
    {
        Some("React".to_string())
    } else if data
        .get("dependencies")
        .and_then(|d| d.get("vue"))
        .is_some()
    {
        Some("Vue".to_string())
    } else if data
        .get("dependencies")
        .and_then(|d| d.get("@angular/core"))
        .is_some()
    {
        Some("Angular".to_string())
    } else if data
        .get("dependencies")
        .and_then(|d| d.get("svelte"))
        .is_some()
    {
        Some("Svelte".to_string())
    } else if data
        .get("dependencies")
        .and_then(|d| d.get("next"))
        .is_some()
    {
        Some("Next.js".to_string())
    } else if data
        .get("devDependencies")
        .and_then(|d| d.get("vite"))
        .is_some()
    {
        Some("Vite".to_string())
    } else {
        None
    }
}

fn parse_cargo_toml(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let value: toml::Table = match text.parse() {
        Ok(v) => v,
        Err(_) => return (None, Vec::new(), Vec::new()),
    };

    let framework = None;

    let mut deps = Vec::new();
    if let Some(deps_table) = value.get("dependencies").and_then(|v| v.as_table()) {
        for name in deps_table.keys() {
            deps.push(name.clone());
        }
    }
    if let Some(deps_table) = value.get("dev-dependencies").and_then(|v| v.as_table()) {
        for name in deps_table.keys() {
            if !deps.contains(name) {
                deps.push(name.clone());
            }
        }
    }

    let mut authors = Vec::new();
    if let Some(arr) = value
        .get("package")
        .and_then(|p| p.get("authors"))
        .and_then(|v| v.as_array())
    {
        for a in arr {
            if let Some(s) = a.as_str() {
                authors.push(s.to_string());
            }
        }
    }

    (framework, deps, authors)
}

fn parse_pyproject(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let value: toml::Table = match text.parse() {
        Ok(v) => v,
        Err(_) => return (None, Vec::new(), Vec::new()),
    };

    let mut framework = None;
    let mut deps = Vec::new();
    let mut authors = Vec::new();

    if let Some(project) = value.get("project").and_then(|v| v.as_table()) {
        if let Some(dep_array) = project.get("dependencies").and_then(|v| v.as_array()) {
            for d in dep_array {
                if let Some(s) = d.as_str() {
                    deps.push(parse_requirement_name(s));
                }
            }
        }
        if let Some(auth) = project.get("authors").and_then(|v| v.as_array()) {
            for a in auth {
                if let Some(name) = a.get("name").and_then(|v| v.as_str()) {
                    authors.push(name.to_string());
                }
            }
        }
    }

    if let Some(poetry) = value
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|v| v.as_table())
    {
        if framework.is_none() {
            framework = None;
        }
        if let Some(dep_table) = poetry.get("dependencies").and_then(|v| v.as_table()) {
            for name in dep_table.keys() {
                if name != "python" && !deps.contains(name) {
                    deps.push(name.clone());
                }
            }
        }
        if let Some(auth) = poetry.get("authors").and_then(|v| v.as_array()) {
            for a in auth {
                if let Some(s) = a.as_str() {
                    if let Some(name) = s.split('<').next().map(|p| p.trim()) {
                        if !authors.contains(&name.to_string()) {
                            authors.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Common framework hints.
    if deps
        .iter()
        .any(|d| d == "django" || d == "flask" || d == "fastapi")
    {
        framework = deps
            .iter()
            .find(|d| *d == "django" || *d == "flask" || *d == "fastapi")
            .map(|f| capitalize(f));
    }

    (framework, deps, authors)
}

fn parse_requirement_name(spec: &str) -> String {
    spec.split(';')
        .next()
        .unwrap_or(spec)
        .split_whitespace()
        .next()
        .unwrap_or(spec)
        .split("==")
        .next()
        .unwrap_or(spec)
        .split(">=")
        .next()
        .unwrap_or(spec)
        .split("~=")
        .next()
        .unwrap_or(spec)
        .split('<')
        .next()
        .unwrap_or(spec)
        .split('[')
        .next()
        .unwrap_or(spec)
        .to_string()
}

fn parse_go_mod(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let mut module_name = None;
    let mut deps = Vec::new();
    let mut in_require = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("module ") {
            module_name = Some(
                trimmed
                    .strip_prefix("module ")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            );
        } else if trimmed.starts_with("require (") {
            in_require = true;
        } else if in_require && trimmed == ")" {
            in_require = false;
        } else if in_require || trimmed.starts_with("require ") {
            let spec = trimmed.trim_start_matches("require ").trim();
            if spec.starts_with('(') || spec.is_empty() {
                continue;
            }
            let parts: Vec<&str> = spec.split_whitespace().collect();
            if let Some(name) = parts.first() {
                if *name != "(" && !deps.contains(&name.to_string()) {
                    deps.push(name.to_string());
                }
            }
        }
    }

    (module_name, deps, Vec::new())
}

fn parse_pom_xml(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let artifact_re = Regex::new(r#"<artifactId>([^<]+)</artifactId>"#).unwrap();
    let mut deps = Vec::new();
    for cap in artifact_re.captures_iter(text) {
        let name = cap[1].trim().to_string();
        if !deps.contains(&name) {
            deps.push(name);
        }
    }
    (None, deps, Vec::new())
}

fn parse_gradle(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let root_re1 =
        Regex::new(r#"(?m)^\s*rootProject\.name\s*=\s*["'](?P<name>[^"']+)["']"#).unwrap();
    let root_re2 =
        Regex::new(r#"(?m)^\s*rootProject\.name\.set\(\s*["'](?P<name>[^"']+)["']\s*\)"#).unwrap();

    let _root_name = root_re1.captures(text).or_else(|| root_re2.captures(text));

    let implementation_re =
        Regex::new(r#"(?m)^\s*implementation\s*\(?\s*['"]([^'"]+)['"]\s*\)?"#).unwrap();
    let mut deps = Vec::new();
    for cap in implementation_re.captures_iter(text) {
        let spec = &cap[1];
        let name = spec.split(':').nth(1).unwrap_or(spec).to_string();
        if !deps.contains(&name) {
            deps.push(name);
        }
    }

    (None, deps, Vec::new())
}

fn parse_requirements(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let mut deps = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let name = parse_requirement_name(trimmed);
        if !name.is_empty() && !deps.contains(&name) {
            deps.push(name);
        }
    }
    (None, deps, Vec::new())
}

fn parse_setup_py(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let name_re = Regex::new(r#"(?m)name\s*=\s*['"]([^'"]+)['"]"#).unwrap();
    let mut authors = Vec::new();
    if let Some(cap) = name_re.captures(text) {
        authors.push(cap[1].to_string());
    }
    let install_re = Regex::new(r#"(?m)install_requires\s*=\s*\[([^\]]+)\]"#).unwrap();
    let mut deps = Vec::new();
    if let Some(cap) = install_re.captures(text) {
        let inner = &cap[1];
        for part in inner.split(',') {
            let name = part.trim().trim_matches(|c| c == '\'' || c == '"');
            let name = parse_requirement_name(name);
            if !name.is_empty() && !deps.contains(&name) {
                deps.push(name);
            }
        }
    }
    (None, deps, authors)
}

fn parse_gemfile(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let gem_re = Regex::new(r#"(?m)^\s*gem\s+['"]([^'"]+)['"]"#).unwrap();
    let mut deps = Vec::new();
    for cap in gem_re.captures_iter(text) {
        let name = cap[1].to_string();
        if !deps.contains(&name) {
            deps.push(name);
        }
    }
    (None, deps, Vec::new())
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => s.to_string(),
    }
}

// ==================== WALKING ====================

fn is_skipped_dir(name: &str) -> bool {
    is_skip_dir(name) || name.starts_with('.')
}

fn walk_dirs<F>(root: &Path, mut visitor: F) -> Result<()>
where
    F: FnMut(&Path, &mut Vec<String>, &[std::fs::DirEntry]) -> Result<()>,
{
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            continue;
        }
        let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {:?}", dir))?
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("collecting directory entries {:?}", dir))?;
        let mut subdirs: Vec<String> = Vec::new();
        for entry in &entries {
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !is_skipped_dir(&name) {
                        subdirs.push(name);
                    }
                }
            }
        }
        visitor(&dir, &mut subdirs, &entries)?;
        for name in subdirs {
            stack.push((dir.join(&name), depth + 1));
        }
    }
    Ok(())
}

fn has_git_marker(path: &Path) -> bool {
    let git_path = path.join(".git");
    git_path.is_dir() || git_path.is_file()
}

fn find_git_repos(root: &Path) -> Vec<PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut repos: Vec<PathBuf> = Vec::new();
    if has_git_marker(&root) {
        repos.push(root.clone());
    }
    let _ = walk_dirs(&root, |dirpath, dirs, _entries| {
        if dirpath == root.as_path() {
            return Ok(());
        }
        if has_git_marker(dirpath) {
            repos.push(dirpath.to_path_buf());
            dirs.clear(); // don't descend into this repo
        }
        Ok(())
    });
    repos
}

fn collect_manifest_names(repo_root: &Path) -> Vec<(String, String, PathBuf)> {
    let mut found: Vec<(String, String, PathBuf)> = Vec::new();
    let _ = walk_dirs(repo_root, |dirpath, dirs, entries| {
        if dirpath != repo_root && has_git_marker(dirpath) {
            dirs.clear();
            return Ok(());
        }
        for entry in entries {
            if let Ok(ft) = entry.file_type() {
                if !ft.is_file() {
                    continue;
                }
                let fname = entry.file_name().to_string_lossy().to_string();
                if let Some(name) = parse_manifest_file(&fname, &dirpath.join(&fname)) {
                    found.push((fname, name, dirpath.to_path_buf()));
                }
            }
        }
        Ok(())
    });

    found.sort_by_key(|a| manifest_sort_key(a, repo_root));
    found
}

fn parse_manifest_file(fname: &str, path: &Path) -> Option<String> {
    if !MANIFEST_PRIORITY.iter().any(|(m, _)| *m == fname) {
        return None;
    }
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let name = manifest_name(fname, &text);
    if name.as_ref().is_some_and(|n| !n.is_empty()) {
        name
    } else {
        None
    }
}

/// Extract the declared project name from a manifest file, if present.
fn manifest_name(manifest: &str, text: &str) -> Option<String> {
    match manifest {
        "package.json" => serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| {
                v.get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            }),
        "Cargo.toml" | "pyproject.toml" => {
            let v = text.parse::<toml::Table>().ok()?;
            v.get("package").or_else(|| v.get("project")).and_then(|t| {
                t.get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
        }
        "go.mod" => text
            .lines()
            .find(|l| l.split_whitespace().next() == Some("module"))
            .and_then(|l| l.split_whitespace().nth(1))
            .map(|s| s.split('/').next_back().unwrap_or(s).to_string()),
        "pom.xml" => {
            let name_re = Regex::new(r#"<name>([^<]+)</name>"#).unwrap();
            name_re
                .captures(text)
                .map(|c| c[1].to_string())
                .or_else(|| {
                    let artifact_re = Regex::new(r#"<artifactId>([^<]+)</artifactId>"#).unwrap();
                    artifact_re.captures(text).map(|c| c[1].to_string())
                })
        }
        "settings.gradle" | "settings.gradle.kts" | "build.gradle" | "build.gradle.kts" => {
            let root_re =
                Regex::new(r#"(?m)^\s*rootProject\.name\s*=\s*['\"]([^'\"]+)['\"]"#).unwrap();
            root_re.captures(text).map(|c| c[1].to_string())
        }
        "setup.py" => {
            let name_re = Regex::new(r#"(?m)name\s*=\s*['\"]([^'\"]+)['\"]"#).unwrap();
            name_re.captures(text).map(|c| c[1].to_string())
        }
        _ => None,
    }
}

fn manifest_sort_key(entry: &(String, String, PathBuf), repo_root: &Path) -> (usize, i32, String) {
    let (manifest_file, _, manifest_dir) = entry;
    let rel = manifest_dir
        .strip_prefix(repo_root)
        .unwrap_or(manifest_dir.as_path());
    let depth = rel.components().count();
    let rel_str = rel.to_string_lossy().to_string();
    let priority = MANIFEST_PRIORITY
        .iter()
        .find(|(m, _)| *m == manifest_file.as_str())
        .map(|(_, p)| *p)
        .unwrap_or(99);
    (depth, priority, rel_str)
}

fn repo_name(repo: &Path) -> String {
    repo.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string())
}

// ==================== GIT HELPERS ====================

fn git_identity(repos: &[PathBuf]) -> (Option<String>, Option<String>) {
    if let Some(repo) = repos.first() {
        let (name, email) = git_user_identity(repo);
        if name.is_some() || email.is_some() {
            return (name, email);
        }
    }
    global_git_identity()
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {:?} in {:?}", args, cwd))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Ok(String::new())
    }
}

fn git_user_identity(repo: &Path) -> (Option<String>, Option<String>) {
    let name = run_git(repo, &["config", "user.name"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let email = run_git(repo, &["config", "user.email"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (name, email)
}

fn global_git_identity() -> (Option<String>, Option<String>) {
    let name = Command::new("git")
        .args(["config", "--global", "user.name"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(o.stdout)
            } else {
                None
            }
        })
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|s| !s.is_empty());
    let email = Command::new("git")
        .args(["config", "--global", "user.email"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(o.stdout)
            } else {
                None
            }
        })
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|s| !s.is_empty());
    (name, email)
}

fn git_authors(repo: &Path) -> Vec<(String, String)> {
    let output = match run_git(
        repo,
        &[
            "log",
            &format!("--max-count={}", MAX_COMMITS_PER_REPO),
            "--format=%aN|%aE",
        ],
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut result = Vec::new();
    for line in output.lines() {
        if let Some((name, email)) = line.split_once('|') {
            result.push((name.trim().to_string(), email.trim().to_string()));
        }
    }
    result
}

// ==================== BOT / NAME FILTERING ====================

fn is_bot(name: &str, email: &str) -> bool {
    let ln = name.to_lowercase();
    let le = email.to_lowercase();
    let bot_name_patterns = [
        r"\[bot\]",
        r"^dependabot",
        r"^renovate",
        r"^github-actions",
        r"^actions-user",
        r"-bot$",
        r"\bbot$",
        r"^bot-",
        r"^snyk",
        r"^greenkeeper",
        r"^semantic-release",
        r"^allcontributors",
        r"-autoroll$",
        r"^auto-format",
        r"^pre-commit-ci",
    ];
    let bot_email_patterns = [r"bot@", r"-bot@", r"\[bot\]@"];
    bot_name_patterns
        .iter()
        .any(|p| Regex::new(p).unwrap().is_match(&ln))
        || bot_email_patterns
            .iter()
            .any(|p| Regex::new(p).unwrap().is_match(&le))
}

fn looks_like_real_name(name: &str) -> bool {
    if name.is_empty() || !name.contains(' ') {
        return false;
    }
    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.len() < 2 {
        return false;
    }
    parts
        .first()
        .unwrap()
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
        && parts
            .last()
            .unwrap()
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
}

// ==================== PEOPLE DEDUPLICATION ====================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum IdentityKey {
    Name(String),
    Email(String),
}

struct UnionFind {
    parent: HashMap<IdentityKey, IdentityKey>,
}

impl UnionFind {
    fn new() -> Self {
        UnionFind {
            parent: HashMap::new(),
        }
    }

    fn find(&mut self, x: IdentityKey) -> IdentityKey {
        if !self.parent.contains_key(&x) {
            self.parent.insert(x.clone(), x.clone());
            return x;
        }
        let mut root = x.clone();
        while let Some(p) = self.parent.get(&root) {
            if p == &root {
                break;
            }
            root = p.clone();
        }
        let mut cur = x;
        while let Some(p) = self.parent.get(&cur) {
            if p == &root {
                break;
            }
            let next = p.clone();
            self.parent.insert(cur.clone(), root.clone());
            cur = next;
        }
        root
    }

    fn union(&mut self, a: IdentityKey, b: IdentityKey) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

fn dedupe_people(all_commits: Vec<(String, String, String)>) -> HashMap<String, PersonInfo> {
    let mut uf = UnionFind::new();
    for (name, email, _repo) in &all_commits {
        let email_key = if email.is_empty() {
            IdentityKey::Name(name.clone())
        } else {
            IdentityKey::Email(email.clone())
        };
        uf.union(IdentityKey::Name(name.clone()), email_key);
    }

    #[derive(Default)]
    struct Component {
        name_counts: HashMap<String, usize>,
        emails: HashSet<String>,
        repos: HashSet<String>,
        total: usize,
    }

    let mut components: HashMap<IdentityKey, Component> = HashMap::new();
    for (name, email, repo) in all_commits {
        let key = uf.find(IdentityKey::Name(name.clone()));
        let entry = components.entry(key).or_default();
        *entry.name_counts.entry(name.clone()).or_insert(0) += 1;
        if !email.is_empty() {
            entry.emails.insert(email);
        }
        entry.repos.insert(repo);
        entry.total += 1;
    }

    let mut people: HashMap<String, PersonInfo> = HashMap::new();
    for component in components.values() {
        let mut candidates: Vec<(String, usize)> = component
            .name_counts
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        candidates.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then(b.0.len().cmp(&a.0.len()))
                .then(a.0.cmp(&b.0))
        });
        let display = candidates
            .iter()
            .find(|(n, _)| looks_like_real_name(n))
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| {
                candidates
                    .first()
                    .map(|(n, _)| n.clone())
                    .unwrap_or_default()
            });
        if !looks_like_real_name(&display) {
            continue;
        }
        let mut emails: Vec<String> = component.emails.iter().cloned().collect();
        emails.sort();
        let mut repos: Vec<String> = component.repos.iter().cloned().collect();
        repos.sort();
        if let Some(existing) = people.get_mut(&display) {
            existing.total_commits += component.total;
            for e in &emails {
                if !existing.emails.contains(e) {
                    existing.emails.push(e.clone());
                }
            }
            for r in &repos {
                if !existing.repos.contains(r) {
                    existing.repos.push(r.clone());
                }
            }
        } else {
            people.insert(
                display.clone(),
                PersonInfo {
                    name: display,
                    total_commits: component.total,
                    emails,
                    repos,
                },
            );
        }
    }
    people
}

// ==================== TESTS ====================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_file<P: AsRef<Path>>(dir: &TempDir, path: P, contents: &str) -> PathBuf {
        let full = dir.path().join(path.as_ref());
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut file = std::fs::File::create(&full).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        full
    }

    #[test]
    fn test_detect_project_type() {
        assert_eq!(detect_project_type("Cargo.toml"), ProjectType::Rust);
        assert_eq!(detect_project_type("package.json"), ProjectType::Node);
        assert_eq!(detect_project_type("pyproject.toml"), ProjectType::Python);
        assert_eq!(detect_project_type("setup.py"), ProjectType::Python);
        assert_eq!(detect_project_type("requirements.txt"), ProjectType::Python);
        assert_eq!(detect_project_type("go.mod"), ProjectType::Go);
        assert_eq!(detect_project_type("pom.xml"), ProjectType::Java);
        assert_eq!(detect_project_type("build.gradle"), ProjectType::Java);
        assert_eq!(detect_project_type("Gemfile"), ProjectType::Ruby);
        assert_eq!(detect_project_type("unknown.txt"), ProjectType::Unknown);
    }

    #[test]
    fn test_project_type_display() {
        assert_eq!(ProjectType::Rust.to_string(), "rust");
        assert_eq!(ProjectType::Node.language(), "JavaScript/TypeScript");
    }

    #[test]
    fn test_project_info_confidence_and_signal() {
        let proj = ProjectInfo {
            name: "foo".to_string(),
            repo_root: PathBuf::from("/tmp/foo"),
            manifest: Some("Cargo.toml".to_string()),
            project_type: ProjectType::Rust,
            language: "Rust".to_string(),
            framework: Some("Axum".to_string()),
            authors: vec![],
            dependencies: vec![],
            has_git: true,
            total_commits: 10,
            user_commits: 5,
            is_mine: true,
        };
        assert!((proj.confidence() - 0.99).abs() < 1e-6);
        let signal = proj.signal();
        assert!(signal.contains("Cargo.toml"));
        assert!(signal.contains("Axum"));
        assert!(signal.contains("5 of your commits"));

        let proj2 = ProjectInfo {
            name: "bar".to_string(),
            repo_root: PathBuf::from("/tmp/bar"),
            manifest: None,
            project_type: ProjectType::Unknown,
            language: "Unknown".to_string(),
            framework: None,
            authors: vec![],
            dependencies: vec![],
            has_git: false,
            total_commits: 0,
            user_commits: 0,
            is_mine: false,
        };
        assert!((proj2.confidence() - 0.85).abs() < 1e-6);
        assert_eq!(proj2.signal(), "project");
    }

    #[test]
    fn test_person_info_confidence_and_signal() {
        let p = PersonInfo {
            name: "Alice Smith".to_string(),
            total_commits: 1,
            emails: vec!["a@example.com".to_string()],
            repos: vec!["/tmp/foo".to_string()],
        };
        assert!((p.confidence() - 0.65).abs() < 1e-6);
        assert!(p.signal().contains("1 commit across 1 repo"));

        let p2 = PersonInfo {
            name: "Bob".to_string(),
            total_commits: 100,
            emails: vec![],
            repos: vec!["r1".to_string(), "r2".to_string(), "r3".to_string()],
        };
        assert!((p2.confidence() - 0.99).abs() < 1e-6);
        assert!(p2.signal().contains("100 commits across 3 repos"));
    }

    #[test]
    fn test_parse_package_json() {
        let text = r#"{
            "name": "my-app",
            "author": "Alice Smith",
            "dependencies": { "react": "^18.0.0", "lodash": "^4.0.0" },
            "devDependencies": { "vite": "^4.0.0" }
        }"#;
        let (framework, deps, authors) = parse_package_json(text);
        assert_eq!(framework, Some("React".to_string()));
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"lodash".to_string()));
        assert!(deps.contains(&"vite".to_string()));
        assert_eq!(authors, vec!["Alice Smith"]);
    }

    #[test]
    fn test_parse_cargo_toml() {
        let text = r#"
[package]
name = "mempalace-rs"
version = "0.5.0"
authors = ["Alice <a@example.com>", "Bob <b@example.com>"]

[dependencies]
tokio = "1"
serde = "1"

[dev-dependencies]
tempfile = "3"
"#;
        let (framework, deps, authors) = parse_cargo_toml(text);
        assert_eq!(framework, None);
        assert!(deps.contains(&"tokio".to_string()));
        assert!(deps.contains(&"serde".to_string()));
        assert!(deps.contains(&"tempfile".to_string()));
        assert_eq!(authors.len(), 2);
    }

    #[test]
    fn test_parse_pyproject() {
        let text = r#"
[project]
name = "my-py"
authors = [{name = "Alice Smith"}, {name = "Bob Jones"}]
dependencies = ["flask>=2.0", "requests"]

[tool.poetry.dependencies]
python = "^3.10"
django = "^4.0"
"#;
        let (framework, deps, authors) = parse_pyproject(text);
        assert_eq!(framework, Some("Flask".to_string()));
        assert!(deps.contains(&"flask".to_string()));
        assert!(deps.contains(&"requests".to_string()));
        assert!(deps.contains(&"django".to_string()));
        assert_eq!(authors.len(), 2);
    }

    #[test]
    fn test_parse_go_mod() {
        let text = r#"
module github.com/example/coolapp

go 1.21

require (
    github.com/foo/bar v1.0.0
    example.com/baz v0.0.1
)
"#;
        let (module, deps, _) = parse_go_mod(text);
        assert_eq!(module, Some("github.com/example/coolapp".to_string()));
        assert!(deps.contains(&"github.com/foo/bar".to_string()));
        assert!(deps.contains(&"example.com/baz".to_string()));
    }

    #[test]
    fn test_parse_pom_xml() {
        let text = r#"<?xml version="1.0"?>
<project>
    <artifactId>my-app</artifactId>
    <dependencies>
        <dependency>
            <artifactId>junit</artifactId>
        </dependency>
    </dependencies>
</project>"#;
        let (_, deps, _) = parse_pom_xml(text);
        assert!(deps.contains(&"my-app".to_string()));
        assert!(deps.contains(&"junit".to_string()));
    }

    #[test]
    fn test_parse_gradle() {
        let text = r#"
rootProject.name = "my-gradle"

plugins {
    id 'java'
}

implementation 'com.squareup.okhttp3:okhttp:4.0.0'
"#;
        let (_, deps, _) = parse_gradle(text);
        assert!(deps.contains(&"okhttp".to_string()));
    }

    #[test]
    fn test_parse_requirements() {
        let text = "# comment\nflask>=2.0\nrequests\n\nnumpy==1.0\n";
        let (_, deps, _) = parse_requirements(text);
        assert!(deps.contains(&"flask".to_string()));
        assert!(deps.contains(&"requests".to_string()));
        assert!(deps.contains(&"numpy".to_string()));
    }

    #[test]
    fn test_parse_setup_py() {
        let text = r#"
setup(
    name="my-setup",
    install_requires=["django>=4.0", "requests"],
)
"#;
        let (_, deps, authors) = parse_setup_py(text);
        assert!(deps.contains(&"django".to_string()));
        assert!(deps.contains(&"requests".to_string()));
        assert!(authors.contains(&"my-setup".to_string()));
    }

    #[test]
    fn test_parse_gemfile() {
        let text = r#"
source "https://rubygems.org"
gem "rails", "~> 7.0"
gem "pg"
"#;
        let (_, deps, _) = parse_gemfile(text);
        assert!(deps.contains(&"rails".to_string()));
        assert!(deps.contains(&"pg".to_string()));
    }

    #[test]
    fn test_is_bot() {
        assert!(is_bot("dependabot[bot]", "dependabot@example.com"));
        assert!(is_bot("Renovate Bot", "renovate@example.com"));
        assert!(is_bot("Actions User", "github-actions-bot@example.com"));
        assert!(!is_bot("Alice Smith", "alice@example.com"));
    }

    #[test]
    fn test_looks_like_real_name() {
        assert!(looks_like_real_name("Alice Smith"));
        assert!(looks_like_real_name("Bob van der Jones"));
        assert!(!looks_like_real_name("alice"));
        assert!(!looks_like_real_name("Alice"));
        assert!(!looks_like_real_name(""));
    }

    #[test]
    fn test_dedupe_people() {
        let commits = vec![
            (
                "Alice Smith".to_string(),
                "alice@example.com".to_string(),
                "r1".to_string(),
            ),
            (
                "Alice S".to_string(),
                "alice@example.com".to_string(),
                "r1".to_string(),
            ),
            (
                "Bob Jones".to_string(),
                "bob@example.com".to_string(),
                "r2".to_string(),
            ),
            (
                "Dependabot".to_string(),
                "bot@example.com".to_string(),
                "r1".to_string(),
            ),
        ];
        let people = dedupe_people(commits);
        assert!(people.contains_key("Alice Smith"));
        assert!(people.contains_key("Bob Jones"));
        assert!(!people.contains_key("Dependabot"));
        let alice = people.get("Alice Smith").unwrap();
        assert_eq!(alice.total_commits, 2);
        assert!(alice.emails.contains(&"alice@example.com".to_string()));
    }

    #[test]
    fn test_scan_project_with_manifests() {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "rust-project/Cargo.toml",
            r#"
[package]
name = "rust-project"
version = "0.1.0"
"#,
        );
        write_file(
            &dir,
            "node-project/package.json",
            r#"{"name": "node-project", "dependencies": {"react": "18"}}"#,
        );
        write_file(
            &dir,
            "py-project/pyproject.toml",
            r#"
[project]
name = "py-project"
"#,
        );

        let result = scan_project(dir.path()).unwrap();
        let names: Vec<String> = result.projects.iter().map(|p| p.name.clone()).collect();
        assert!(names.contains(&"rust-project".to_string()));
        assert!(names.contains(&"node-project".to_string()));
        assert!(names.contains(&"py-project".to_string()));

        let node = result
            .projects
            .iter()
            .find(|p| p.name == "node-project")
            .unwrap();
        assert_eq!(node.project_type, ProjectType::Node);
        assert_eq!(node.framework, Some("React".to_string()));
    }

    #[test]
    fn test_scan_projects_merges() {
        let dir1 = TempDir::new().unwrap();
        write_file(
            &dir1,
            "a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
        );
        let dir2 = TempDir::new().unwrap();
        write_file(&dir2, "b/package.json", r#"{"name": "b"}"#);
        let result = scan_projects(&[dir1.path(), dir2.path()]).unwrap();
        let names: Vec<String> = result.projects.iter().map(|p| p.name.clone()).collect();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    #[test]
    fn test_scan_result_to_entities() {
        let result = ScanResult {
            projects: vec![ProjectInfo {
                name: "p".to_string(),
                repo_root: PathBuf::from("/tmp/p"),
                manifest: None,
                project_type: ProjectType::Unknown,
                language: "Unknown".to_string(),
                framework: None,
                authors: vec![],
                dependencies: vec![],
                has_git: false,
                total_commits: 0,
                user_commits: 0,
                is_mine: false,
            }],
            people: vec![PersonInfo {
                name: "Alice".to_string(),
                total_commits: 1,
                emails: vec![],
                repos: vec![],
            }],
        };
        let entities = result.to_detected_entities(10, 10);
        assert_eq!(entities.len(), 2);
        assert!(entities.iter().any(|e| e.r#type == EntityType::Project));
        assert!(entities.iter().any(|e| e.r#type == EntityType::Person));
    }

    #[test]
    fn test_scan_result_sorts() {
        let mut result = ScanResult {
            projects: vec![
                ProjectInfo {
                    name: "z".to_string(),
                    repo_root: PathBuf::from("/tmp/z"),
                    manifest: None,
                    project_type: ProjectType::Unknown,
                    language: "Unknown".to_string(),
                    framework: None,
                    authors: vec![],
                    dependencies: vec![],
                    has_git: true,
                    total_commits: 1,
                    user_commits: 0,
                    is_mine: false,
                },
                ProjectInfo {
                    name: "a".to_string(),
                    repo_root: PathBuf::from("/tmp/a"),
                    manifest: None,
                    project_type: ProjectType::Unknown,
                    language: "Unknown".to_string(),
                    framework: None,
                    authors: vec![],
                    dependencies: vec![],
                    has_git: true,
                    total_commits: 10,
                    user_commits: 5,
                    is_mine: true,
                },
            ],
            people: vec![],
        };
        result.sort_projects();
        assert_eq!(result.projects[0].name, "a");
        assert_eq!(result.projects[1].name, "z");
    }

    #[test]
    fn test_scan_project_nonexistent() {
        let result = scan_project("/this/path/does/not/exist/12345").unwrap();
        assert!(result.projects.is_empty());
        assert!(result.people.is_empty());
    }

    #[test]
    fn test_parse_manifest_file_bad_json() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "package.json", "not json");
        let name = parse_manifest_file("package.json", &path);
        assert!(name.is_none());
    }

    #[test]
    fn test_project_info_to_entity() {
        let proj = ProjectInfo {
            name: "test".to_string(),
            repo_root: PathBuf::from("/tmp/test"),
            manifest: Some("Cargo.toml".to_string()),
            project_type: ProjectType::Rust,
            language: "Rust".to_string(),
            framework: None,
            authors: vec![],
            dependencies: vec![],
            has_git: true,
            total_commits: 5,
            user_commits: 5,
            is_mine: true,
        };
        let entity = proj.to_detected_entity();
        assert_eq!(entity.name, "test");
        assert_eq!(entity.r#type, EntityType::Project);
        assert!((entity.confidence - 0.99).abs() < 1e-6);
        assert!(entity.signals[0].contains("Cargo.toml"));
    }

    #[test]
    fn test_person_info_to_entity() {
        let person = PersonInfo {
            name: "Alice Smith".to_string(),
            total_commits: 50,
            emails: vec!["alice@example.com".to_string()],
            repos: vec!["repo".to_string()],
        };
        let entity = person.to_detected_entity();
        assert_eq!(entity.name, "Alice Smith");
        assert_eq!(entity.r#type, EntityType::Person);
        assert!((entity.confidence - 0.85).abs() < 1e-6);
        assert!(entity.aliases.contains(&"alice@example.com".to_string()));
    }

    #[test]
    fn test_parse_manifest() {
        // Direct coverage of the manifest dispatch function.
        let (pt, fw, deps, _authors) = parse_manifest(
            "package.json",
            r#"{"name":"x","dependencies":{"react":"1"}}"#,
        );
        assert_eq!(pt, ProjectType::Node);
        assert_eq!(fw, Some("React".to_string()));
        assert!(deps.contains(&"react".to_string()));

        let (pt, _fw, deps, _authors) = parse_manifest(
            "Cargo.toml",
            "[package]\nname=\"x\"\n[dependencies]\ntokio=\"1\"",
        );
        assert_eq!(pt, ProjectType::Rust);
        assert!(deps.contains(&"tokio".to_string()));

        let (pt, fw, deps, _authors) = parse_manifest(
            "pyproject.toml",
            "[project]\nname=\"x\"\ndependencies=[\"flask\"]",
        );
        assert_eq!(pt, ProjectType::Python);
        assert_eq!(fw, Some("Flask".to_string()));
        assert!(deps.contains(&"flask".to_string()));

        let (pt, _fw, deps, _authors) = parse_manifest(
            "go.mod",
            "module github.com/example/x\n\nrequire (\n\texample.com/y v1.0.0\n)",
        );
        assert_eq!(pt, ProjectType::Go);
        assert!(deps.contains(&"example.com/y".to_string()));

        let (pt, _fw, deps, _authors) =
            parse_manifest("pom.xml", "<project><artifactId>x</artifactId></project>");
        assert_eq!(pt, ProjectType::Java);
        assert!(deps.contains(&"x".to_string()));

        let (pt, _fw, deps, _authors) = parse_manifest(
            "build.gradle",
            "rootProject.name = 'x'\nimplementation 'com.squareup:junit:1.0'",
        );
        assert_eq!(pt, ProjectType::Java);
        assert!(deps.contains(&"junit".to_string()));

        let (pt, _fw, deps, _authors) = parse_manifest("requirements.txt", "flask\nrequests\n");
        assert_eq!(pt, ProjectType::Python);
        assert!(deps.contains(&"flask".to_string()));

        let (pt, _fw, deps, authors) =
            parse_manifest("setup.py", "setup(name='x', install_requires=['django'])");
        assert_eq!(pt, ProjectType::Python);
        assert!(deps.contains(&"django".to_string()));
        assert!(authors.contains(&"x".to_string()));

        let (pt, _fw, deps, _authors) =
            parse_manifest("Gemfile", "source 'https://rubygems.org'\ngem 'rails'");
        assert_eq!(pt, ProjectType::Ruby);
        assert!(deps.contains(&"rails".to_string()));

        // Unknown manifest falls back to no parsing.
        let (pt, fw, deps, authors) = parse_manifest("unknown.txt", "whatever");
        assert_eq!(pt, ProjectType::Unknown);
        assert!(fw.is_none());
        assert!(deps.is_empty());
        assert!(authors.is_empty());
    }

    #[test]
    fn test_project_info_signal_variants() {
        let proj = ProjectInfo {
            name: "a".to_string(),
            repo_root: PathBuf::from("/tmp/a"),
            manifest: Some("Cargo.toml".to_string()),
            project_type: ProjectType::Rust,
            language: "Rust".to_string(),
            framework: None,
            authors: vec![],
            dependencies: vec![],
            has_git: true,
            total_commits: 8,
            user_commits: 3,
            is_mine: false,
        };
        assert_eq!(proj.signal(), "Cargo.toml, 3/8");

        let proj = ProjectInfo {
            name: "b".to_string(),
            repo_root: PathBuf::from("/tmp/b"),
            manifest: None,
            project_type: ProjectType::Unknown,
            language: "Unknown".to_string(),
            framework: None,
            authors: vec![],
            dependencies: vec![],
            has_git: true,
            total_commits: 4,
            user_commits: 0,
            is_mine: false,
        };
        assert_eq!(proj.signal(), "4 commits");

        let proj = ProjectInfo {
            name: "c".to_string(),
            repo_root: PathBuf::from("/tmp/c"),
            manifest: Some("package.json".to_string()),
            project_type: ProjectType::Node,
            language: "JavaScript/TypeScript".to_string(),
            framework: Some("Next.js".to_string()),
            authors: vec![],
            dependencies: vec![],
            has_git: false,
            total_commits: 0,
            user_commits: 0,
            is_mine: false,
        };
        assert_eq!(proj.signal(), "package.json, Next.js");
    }

    #[test]
    fn test_sort_people() {
        let mut result = ScanResult {
            projects: vec![],
            people: vec![
                PersonInfo {
                    name: "Bob".to_string(),
                    total_commits: 5,
                    emails: vec![],
                    repos: vec![],
                },
                PersonInfo {
                    name: "Alice".to_string(),
                    total_commits: 20,
                    emails: vec![],
                    repos: vec![],
                },
                PersonInfo {
                    name: "Carol".to_string(),
                    total_commits: 100,
                    emails: vec![],
                    repos: vec![],
                },
            ],
        };
        result.sort_people();
        assert_eq!(result.people[0].name, "Carol");
        assert_eq!(result.people[1].name, "Alice");
        assert_eq!(result.people[2].name, "Bob");
    }

    #[test]
    fn test_parse_package_json_edge_cases() {
        // Invalid JSON returns empty results.
        let (fw, deps, authors) = parse_package_json("not json");
        assert!(fw.is_none());
        assert!(deps.is_empty());
        assert!(authors.is_empty());

        // Author as object, contributors as objects, peerDependencies, and Vue framework.
        let text = r#"{
            "name": "edge",
            "author": {"name": "Alice Smith"},
            "contributors": [{"name": "Bob Jones"}, "Charlie Day"],
            "dependencies": {"vue": "3"},
            "peerDependencies": {"react": "18"}
        }"#;
        let (fw, deps, authors) = parse_package_json(text);
        assert_eq!(fw, Some("Vue".to_string()));
        assert!(deps.contains(&"vue".to_string()));
        assert!(deps.contains(&"react".to_string()));
        assert!(authors.contains(&"Alice Smith".to_string()));
        assert!(authors.contains(&"Bob Jones".to_string()));
        assert!(authors.contains(&"Charlie Day".to_string()));

        // Framework detection for Angular, Svelte, Next.js, and Vite.
        assert_eq!(
            parse_package_json(r#"{"dependencies":{"@angular/core":"1"}}"#).0,
            Some("Angular".to_string())
        );
        assert_eq!(
            parse_package_json(r#"{"dependencies":{"svelte":"1"}}"#).0,
            Some("Svelte".to_string())
        );
        assert_eq!(
            parse_package_json(r#"{"dependencies":{"next":"1"}}"#).0,
            Some("Next.js".to_string())
        );
        assert_eq!(
            parse_package_json(r#"{"devDependencies":{"vite":"1"}}"#).0,
            Some("Vite".to_string())
        );
    }

    #[test]
    fn test_parse_cargo_toml_edge_cases() {
        // Invalid TOML.
        let (fw, deps, authors) = parse_cargo_toml("not valid toml [[");
        assert!(fw.is_none());
        assert!(deps.is_empty());
        assert!(authors.is_empty());

        // No authors.
        let (_fw, deps, authors) =
            parse_cargo_toml("[package]\nname=\"x\"\n[dependencies]\ntokio=\"1\"");
        assert!(authors.is_empty());
        assert!(deps.contains(&"tokio".to_string()));
    }

    #[test]
    fn test_parse_pyproject_edge_cases() {
        // Invalid TOML.
        let (fw, deps, authors) = parse_pyproject("not valid [[");
        assert!(fw.is_none());
        assert!(deps.is_empty());
        assert!(authors.is_empty());

        // Poetry authors as "Name <email>" strings.
        let text = r#"
[tool.poetry]
authors = ["Alice Smith <alice@example.com>"]
[tool.poetry.dependencies]
fastapi = "^0.100"
"#;
        let (fw, deps, authors) = parse_pyproject(text);
        assert_eq!(fw, Some("Fastapi".to_string()));
        assert!(deps.contains(&"fastapi".to_string()));
        assert!(authors.contains(&"Alice Smith".to_string()));
    }

    #[test]
    fn test_parse_gradle_edge_cases() {
        let text = r#"
rootProject.name = "legacy"
rootProject.name.set("ignored")
implementation 'org.jetbrains.kotlinx:kotlinx-coroutines-core:1.0'
"#;
        let (_, deps, _) = parse_gradle(text);
        assert!(deps.contains(&"kotlinx-coroutines-core".to_string()));

        // Kotlin DSL rootProject.name.set syntax.
        let text = r#"
rootProject.name.set("kotlin-dsl")
implementation("com.google.guava:guava:31.0")
"#;
        let (_, deps, _) = parse_gradle(text);
        assert!(deps.contains(&"guava".to_string()));
    }

    #[test]
    fn test_parse_requirements_edge_cases() {
        let text = "# comment\n\nflask>=2.0\nrequests[security]\n\";\n";
        let (_, deps, _) = parse_requirements(text);
        assert!(deps.contains(&"flask".to_string()));
        assert!(deps.contains(&"requests".to_string()));
    }

    #[test]
    fn test_parse_gemfile_edge_cases() {
        let text = "# comment\n\ngem 'rails', '~> 7.0'\n\";\n";
        let (_, deps, _) = parse_gemfile(text);
        assert!(deps.contains(&"rails".to_string()));

        // No gems.
        let (_, deps, _) = parse_gemfile("# empty\n");
        assert!(deps.is_empty());
    }

    #[test]
    fn test_find_git_repos() {
        let dir = TempDir::new().unwrap();
        // Root is a git repo.
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let repos = find_git_repos(dir.path());
        assert_eq!(repos.len(), 1);

        // Nested git repos are found but not descended into.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("a/.git")).unwrap();
        std::fs::create_dir_all(dir.path().join("b/.git")).unwrap();
        let repos = find_git_repos(dir.path());
        assert_eq!(repos.len(), 2);
    }

    #[test]
    fn test_collect_manifest_names_and_sort_key() {
        let dir = TempDir::new().unwrap();
        write_file(&dir, "node/package.json", r#"{"name":"node"}"#);
        write_file(&dir, "rust/Cargo.toml", "[package]\nname=\"rust\"");
        write_file(&dir, "py/pyproject.toml", "[project]\nname=\"py\"");
        write_file(&dir, "go/go.mod", "module example.com/goapp");

        let found = collect_manifest_names(dir.path());
        let names: Vec<String> = found.iter().map(|(_, n, _)| n.clone()).collect();
        assert!(names.contains(&"node".to_string()));
        assert!(names.contains(&"rust".to_string()));
        assert!(names.contains(&"py".to_string()));
        assert!(names.contains(&"goapp".to_string()));

        // Sort key: root-level manifests come before nested ones and lower priority wins.
        let root_entry = (
            "package.json".to_string(),
            "root".to_string(),
            dir.path().to_path_buf(),
        );
        let nested_entry = (
            "Cargo.toml".to_string(),
            "nested".to_string(),
            dir.path().join("rust"),
        );
        assert!(
            manifest_sort_key(&root_entry, dir.path())
                <= manifest_sort_key(&nested_entry, dir.path())
        );
    }

    #[test]
    fn test_manifest_name() {
        assert_eq!(
            manifest_name("package.json", r#"{"name":"my-app"}"#),
            Some("my-app".to_string())
        );
        assert_eq!(
            manifest_name("Cargo.toml", "[package]\nname=\"my-crate\""),
            Some("my-crate".to_string())
        );
        assert_eq!(
            manifest_name("pyproject.toml", "[project]\nname=\"my-py\""),
            Some("my-py".to_string())
        );
        assert_eq!(
            manifest_name("go.mod", "module github.com/example/coolapp\n"),
            Some("coolapp".to_string())
        );
        assert_eq!(
            manifest_name("pom.xml", "<project><name>my-maven</name></project>"),
            Some("my-maven".to_string())
        );
        assert_eq!(
            manifest_name(
                "pom.xml",
                "<project><artifactId>my-artifact</artifactId></project>"
            ),
            Some("my-artifact".to_string())
        );
        assert_eq!(
            manifest_name("settings.gradle", "rootProject.name = 'my-gradle'"),
            Some("my-gradle".to_string())
        );
        assert_eq!(
            manifest_name("setup.py", "setup(name='my-setup')"),
            Some("my-setup".to_string())
        );
        // Missing name returns None.
        assert!(manifest_name("package.json", r#"{"version":"1"}"#).is_none());
    }

    #[test]
    fn test_project_type_all_variants() {
        // Exercise every match arm in as_str and language.
        assert_eq!(ProjectType::Node.as_str(), "node");
        assert_eq!(ProjectType::Node.language(), "JavaScript/TypeScript");
        assert_eq!(ProjectType::Python.as_str(), "python");
        assert_eq!(ProjectType::Python.language(), "Python");
        assert_eq!(ProjectType::Go.as_str(), "go");
        assert_eq!(ProjectType::Go.language(), "Go");
        assert_eq!(ProjectType::Java.as_str(), "java");
        assert_eq!(ProjectType::Java.language(), "Java/Kotlin");
        assert_eq!(ProjectType::Ruby.as_str(), "ruby");
        assert_eq!(ProjectType::Ruby.language(), "Ruby");
        assert_eq!(ProjectType::Unknown.as_str(), "unknown");
        assert_eq!(ProjectType::Unknown.language(), "Unknown");
    }

    #[test]
    fn test_scan_project_with_git_repo() {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();

        fn run(repo: &std::path::Path, args: &[&str]) {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }

        run(&repo, &["init", "--quiet"]);
        run(&repo, &["config", "user.name", "Alice Smith"]);
        run(&repo, &["config", "user.email", "alice@example.com"]);
        write_file(&dir, "repo/Cargo.toml", "[package]\nname=\"git-crate\"\n");
        write_file(&dir, "repo/src/main.rs", "fn main() {}");
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-m", "initial", "--quiet"]);

        let result = scan_project(&repo).unwrap();
        assert_eq!(result.projects.len(), 1);
        let proj = &result.projects[0];
        assert_eq!(proj.name, "git-crate");
        assert_eq!(proj.project_type, ProjectType::Rust);
        assert!(proj.has_git);
        assert_eq!(proj.total_commits, 1);
        assert!(proj.is_mine);
        assert_eq!(result.people.len(), 1);
        assert_eq!(result.people[0].name, "Alice Smith");
    }

    #[test]
    fn test_project_info_confidence_git_not_mine() {
        let proj = ProjectInfo {
            name: "x".to_string(),
            repo_root: PathBuf::from("/tmp/x"),
            manifest: None,
            project_type: ProjectType::Unknown,
            language: "Unknown".to_string(),
            framework: None,
            authors: vec![],
            dependencies: vec![],
            has_git: true,
            total_commits: 5,
            user_commits: 0,
            is_mine: false,
        };
        assert!((proj.confidence() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_scan_project_git_without_manifest() {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("bare");
        std::fs::create_dir(&repo).unwrap();

        fn run(repo: &std::path::Path, args: &[&str]) {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }

        run(&repo, &["init", "--quiet"]);
        run(&repo, &["config", "user.name", "Bob Jones"]);
        run(&repo, &["config", "user.email", "bob@example.com"]);
        write_file(&dir, "bare/readme.txt", "hi");
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-m", "initial", "--quiet"]);
        // After the commit, set the repo identity to someone else so the project is not mine.
        run(&repo, &["config", "user.name", "Charlie Day"]);
        run(&repo, &["config", "user.email", "charlie@example.com"]);

        let result = scan_project(&repo).unwrap();
        assert_eq!(result.projects.len(), 1);
        let proj = &result.projects[0];
        assert_eq!(proj.name, "bare");
        assert_eq!(proj.project_type, ProjectType::Unknown);
        assert!(proj.has_git);
        assert!(!proj.is_mine);
        assert_eq!(result.people.len(), 1);
        assert_eq!(result.people[0].name, "Bob Jones");
    }

    #[test]
    fn test_scan_projects_merges_duplicate_people() {
        let dir = TempDir::new().unwrap();
        let repo1 = dir.path().join("r1");
        let repo2 = dir.path().join("r2");
        std::fs::create_dir(&repo1).unwrap();
        std::fs::create_dir(&repo2).unwrap();

        fn run(repo: &std::path::Path, args: &[&str]) {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }

        for repo in [&repo1, &repo2] {
            run(repo, &["init", "--quiet"]);
            run(repo, &["config", "user.name", "Alice Smith"]);
            run(repo, &["config", "user.email", "alice@example.com"]);
            write_file(
                &dir,
                &format!("{}/main.txt", repo.file_name().unwrap().to_string_lossy()),
                "x",
            );
            run(repo, &["add", "."]);
            run(repo, &["commit", "-m", "init", "--quiet"]);
        }

        let result = scan_projects(&[repo1.as_path(), repo2.as_path()]).unwrap();
        assert_eq!(result.people.len(), 1);
        let alice = &result.people[0];
        assert_eq!(alice.name, "Alice Smith");
        assert_eq!(alice.total_commits, 2);
        assert_eq!(alice.repos.len(), 2);
    }

    #[test]
    fn test_scan_project_extra_java_manifests() {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("multi");
        std::fs::create_dir(&repo).unwrap();

        fn run(repo: &std::path::Path, args: &[&str]) {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }

        run(&repo, &["init", "--quiet"]);
        run(&repo, &["config", "user.name", "Alice Smith"]);
        run(&repo, &["config", "user.email", "alice@example.com"]);
        write_file(
            &dir,
            "multi/pom.xml",
            "<project><name>parent</name></project>",
        );
        write_file(
            &dir,
            "multi/core/pom.xml",
            "<project><name>core</name></project>",
        );
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-m", "initial", "--quiet"]);

        let result = scan_project(&repo).unwrap();
        let names: Vec<String> = result.projects.iter().map(|p| p.name.clone()).collect();
        assert!(names.contains(&"parent".to_string()));
        assert!(names.contains(&"core".to_string()));
    }
}
