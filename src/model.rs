use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub path: PathBuf,
    pub content: String,
    pub saved_content: String,
    pub last_saved: Option<SystemTime>,
}

impl Document {
    pub fn open(path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        let last_saved = fs::metadata(&path).and_then(|m| m.modified()).ok();
        Ok(Self {
            path,
            saved_content: content.clone(),
            content,
            last_saved,
        })
    }

    pub fn title(&self) -> String {
        self.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_owned()
    }

    pub fn is_dirty(&self) -> bool {
        self.content != self.saved_content
    }

    pub fn save(&mut self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&self.path, self.content.as_bytes())?;
        self.saved_content.clone_from(&self.content);
        self.last_saved = Some(SystemTime::now());
        Ok(())
    }

    pub fn word_count(&self) -> usize {
        self.content.split_whitespace().count()
    }
}

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub files: Vec<FileEntry>,
    pub manifest: ProjectManifest,
}

impl Project {
    pub fn open(root: PathBuf) -> Result<Self> {
        if !root.is_dir() {
            bail!("{} is not a folder", root.display());
        }
        let manifest_path = root.join(".novelquill/project.json");
        let manifest = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| ProjectManifest {
                name: root
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Novel")
                    .to_owned(),
                ..Default::default()
            });
        let mut project = Self {
            root,
            files: vec![],
            manifest,
        };
        project.refresh()?;
        project.save_manifest()?;
        Ok(project)
    }

    pub fn name(&self) -> String {
        self.manifest.name.clone()
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.files.clear();
        scan_dir(&self.root, 0, &mut self.files, &self.manifest)?;
        let paths = self
            .files
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let mut changed = false;
        for path in paths {
            let relative = self.relative_string(&path);
            if !self
                .manifest
                .documents
                .iter()
                .any(|meta| meta.path == relative)
            {
                let order = self.manifest.documents.len();
                self.manifest.documents.push(DocumentMeta {
                    id: new_id("doc"),
                    path: relative,
                    order,
                    ..Default::default()
                });
                changed = true;
            }
        }
        if changed {
            self.save_manifest()?;
            self.files.clear();
            scan_dir(&self.root, 0, &mut self.files, &self.manifest)?;
        }
        Ok(())
    }

    pub fn create_document(&mut self, relative: &str) -> Result<PathBuf> {
        let mut relative = relative.trim().trim_start_matches('/').to_owned();
        if relative.is_empty() {
            relative = "Untitled.md".into();
        }
        if Path::new(&relative).extension().is_none() {
            relative.push_str(".md");
        }
        let path = self.root.join(relative);
        ensure_inside(&self.root, &path)?;
        if path.exists() {
            bail!("That document already exists");
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, "# Untitled\n\n")?;
        self.refresh()?;
        Ok(path)
    }

    pub fn create_folder(&mut self, relative: &str) -> Result<()> {
        let path = self.root.join(relative.trim().trim_start_matches('/'));
        ensure_inside(&self.root, &path)?;
        fs::create_dir_all(path)?;
        self.refresh()
    }

    pub fn create_starter_structure(&mut self) -> Result<()> {
        for folder in [
            "Manuscript",
            "Characters",
            "Locations",
            "Plot",
            "Worldbuilding",
            "Research",
            "Archive",
        ] {
            fs::create_dir_all(self.root.join(folder))?;
        }
        let chapter = self.root.join("Manuscript/Chapter 01.md");
        if !chapter.exists() {
            fs::write(&chapter, "# Chapter One\n\n## Scene One\n\n")?;
        }
        self.refresh()
    }

    pub fn duplicate_document(&mut self, source: &Path) -> Result<PathBuf> {
        ensure_inside(&self.root, source)?;
        let parent = source.parent().unwrap_or(&self.root);
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Copy");
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("md");
        let mut number = 1;
        let target = loop {
            let suffix = if number == 1 {
                " Copy".to_owned()
            } else {
                format!(" Copy {number}")
            };
            let candidate = parent.join(format!("{stem}{suffix}.{extension}"));
            if !candidate.exists() {
                break candidate;
            }
            number += 1;
        };
        fs::copy(source, &target)?;
        self.refresh()?;
        if let Some(source_meta) = self.document_meta(source).cloned()
            && let Some(target_meta) = self.document_meta_mut(&target)
        {
            let id = target_meta.id.clone();
            let path = target_meta.path.clone();
            let order = target_meta.order;
            *target_meta = source_meta;
            target_meta.id = id;
            target_meta.path = path;
            target_meta.order = order;
            target_meta.status = "Draft".into();
            self.save_manifest()?;
        }
        Ok(target)
    }

    pub fn rename_document(&mut self, source: &Path, new_name: &str) -> Result<PathBuf> {
        ensure_inside(&self.root, source)?;
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name.contains('/') || new_name.contains('\\') {
            bail!("Enter a file name without folder separators");
        }
        let mut target = source.with_file_name(new_name);
        if target.extension().is_none() {
            target.set_extension(
                source
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("md"),
            );
        }
        if target.exists() && target != source {
            bail!("A document with that name already exists");
        }
        let old_relative = self.relative_string(source);
        fs::rename(source, &target)?;
        let new_relative = self.relative_string(&target);
        if let Some(meta) = self
            .manifest
            .documents
            .iter_mut()
            .find(|meta| meta.path == old_relative)
        {
            meta.path = new_relative;
        }
        self.save_manifest()?;
        self.refresh()?;
        Ok(target)
    }

    pub fn trash_document(&mut self, source: &Path) -> Result<()> {
        ensure_inside(&self.root, source)?;
        let original_path = self.relative_string(source);
        let id = self
            .document_meta(source)
            .map(|meta| meta.id.clone())
            .unwrap_or_else(|| new_id("trash"));
        let file_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("document.md");
        let stored_path = format!(".novelquill/trash/{id}-{file_name}");
        let target = self.root.join(&stored_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(source, &target)?;
        self.manifest.trash.push(TrashItem {
            id,
            original_path,
            stored_path,
        });
        self.save_manifest()?;
        self.refresh()
    }

    pub fn restore_trash(&mut self, index: usize) -> Result<PathBuf> {
        let item = self
            .manifest
            .trash
            .get(index)
            .context("Trash item not found")?
            .clone();
        let source = self.root.join(&item.stored_path);
        let target = self.root.join(&item.original_path);
        if target.exists() {
            bail!("The original path is already occupied");
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(source, &target)?;
        self.manifest.trash.remove(index);
        self.save_manifest()?;
        self.refresh()?;
        Ok(target)
    }

    pub fn split_document_at_headings(&mut self, source: &Path) -> Result<Vec<PathBuf>> {
        let text = fs::read_to_string(source)?;
        let mut sections = Vec::<(String, String)>::new();
        let mut title = String::new();
        let mut body = String::new();
        let mut seen_scene_heading = false;
        for line in text.lines() {
            if line.starts_with("## ") && seen_scene_heading && !body.trim().is_empty() {
                sections.push((std::mem::take(&mut title), std::mem::take(&mut body)));
            }
            if line.starts_with("## ") {
                seen_scene_heading = true;
                title = line.trim_start_matches("## ").trim().to_owned();
            }
            body.push_str(line);
            body.push('\n');
        }
        if !body.trim().is_empty() {
            sections.push((title, body));
        }
        if sections.len() < 2 {
            bail!("Add at least two level-two headings (`## Scene`) before splitting");
        }
        let parent = source.parent().unwrap_or(&self.root);
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Scene");
        let mut created = vec![];
        for (index, (title, body)) in sections.into_iter().enumerate() {
            let safe_title = title
                .chars()
                .map(|character| {
                    if character.is_alphanumeric() || character == ' ' {
                        character
                    } else {
                        '-'
                    }
                })
                .collect::<String>();
            let name = if safe_title.trim().is_empty() {
                format!("{stem} Part {}.md", index + 1)
            } else {
                format!("{} - {}.md", stem, safe_title.trim())
            };
            let path = parent.join(name);
            if path.exists() {
                bail!("Split target {} already exists", path.display());
            }
            atomic_write(&path, body.as_bytes())?;
            created.push(path);
        }
        self.trash_document(source)?;
        self.refresh()?;
        Ok(created)
    }

    pub fn save_manifest(&self) -> Result<()> {
        let path = self.root.join(".novelquill/project.json");
        let bytes = serde_json::to_vec_pretty(&self.manifest)?;
        atomic_write(&path, &bytes)
    }

    pub fn document_meta(&self, path: &Path) -> Option<&DocumentMeta> {
        let relative = self.relative_string(path);
        self.manifest
            .documents
            .iter()
            .find(|meta| meta.path == relative)
    }

    pub fn document_meta_mut(&mut self, path: &Path) -> Option<&mut DocumentMeta> {
        let relative = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        self.manifest
            .documents
            .iter_mut()
            .find(|meta| meta.path == relative)
    }

    pub fn move_document(&mut self, path: &Path, direction: isize) -> Result<()> {
        let relative = self.relative_string(path);
        let mut ordered = self
            .manifest
            .documents
            .iter()
            .enumerate()
            .filter(|(_, meta)| !meta.archived)
            .map(|(index, meta)| (index, meta.order))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(_, order)| *order);
        let Some(position) = ordered
            .iter()
            .position(|(index, _)| self.manifest.documents[*index].path == relative)
        else {
            return Ok(());
        };
        let target = if direction < 0 {
            position.checked_sub(1)
        } else if position + 1 < ordered.len() {
            Some(position + 1)
        } else {
            None
        };
        if let Some(target) = target {
            let first = ordered[position].0;
            let second = ordered[target].0;
            let order = self.manifest.documents[first].order;
            self.manifest.documents[first].order = self.manifest.documents[second].order;
            self.manifest.documents[second].order = order;
            self.save_manifest()?;
            self.refresh()?;
        }
        Ok(())
    }

    pub fn save_document(&self, document: &mut Document) -> Result<()> {
        if document.is_dirty() && !document.saved_content.is_empty() {
            self.snapshot(document)?;
        }
        document.save()
    }

    pub fn snapshot(&self, document: &Document) -> Result<()> {
        let id = self
            .document_meta(&document.path)
            .map(|meta| meta.id.clone())
            .unwrap_or_else(|| new_id("doc"));
        let directory = self.root.join(".novelquill/history").join(id);
        fs::create_dir_all(&directory)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        atomic_write(
            &directory.join(format!("{timestamp}.md")),
            document.saved_content.as_bytes(),
        )?;
        let mut snapshots = fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        snapshots.sort();
        let remove_count = snapshots.len().saturating_sub(50);
        for old in snapshots.into_iter().take(remove_count) {
            let _ = fs::remove_file(old);
        }
        Ok(())
    }

    pub fn revisions(&self, path: &Path) -> Vec<PathBuf> {
        let Some(meta) = self.document_meta(path) else {
            return vec![];
        };
        let directory = self.root.join(".novelquill/history").join(&meta.id);
        let mut revisions: Vec<PathBuf> = fs::read_dir(directory)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default();
        revisions.sort_by(|a, b| b.cmp(a));
        revisions
    }

    fn relative_string(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

fn ensure_inside(root: &Path, path: &Path) -> Result<()> {
    let normalized = path.components().collect::<PathBuf>();
    if !normalized.starts_with(root)
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("The path must stay inside the project");
    }
    Ok(())
}

fn scan_dir(
    dir: &Path,
    depth: usize,
    output: &mut Vec<FileEntry>,
    manifest: &ProjectManifest,
) -> Result<()> {
    let mut entries = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        let path = entry.path();
        let order = manifest
            .documents
            .iter()
            .find(|meta| path.ends_with(&meta.path))
            .map(|meta| meta.order)
            .unwrap_or(usize::MAX);
        (!path.is_dir(), order, entry.file_name())
    });

    for entry in entries {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            output.push(FileEntry {
                path: path.clone(),
                is_dir: true,
                depth,
            });
            scan_dir(&path, depth + 1, output, manifest)?;
        } else if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("md" | "markdown" | "txt")
        ) {
            output.push(FileEntry {
                path,
                is_dir: false,
                depth,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    #[default]
    Scene,
    Chapter,
    Part,
    Note,
    Research,
}

impl DocumentKind {
    pub const ALL: [Self; 5] = [
        Self::Scene,
        Self::Chapter,
        Self::Part,
        Self::Note,
        Self::Research,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Scene => "Scene",
            Self::Chapter => "Chapter",
            Self::Part => "Part",
            Self::Note => "Note",
            Self::Research => "Research",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentMeta {
    pub id: String,
    pub path: String,
    pub order: usize,
    pub kind: DocumentKind,
    pub active: bool,
    pub archived: bool,
    pub status: String,
    pub synopsis: String,
    pub pov: String,
    pub location: String,
    pub story_time: String,
    pub tags: Vec<String>,
    pub characters: Vec<String>,
    pub plot_threads: Vec<String>,
    pub beats: String,
    pub ai_include: bool,
    pub word_target: usize,
}

impl Default for DocumentMeta {
    fn default() -> Self {
        Self {
            id: String::new(),
            path: String::new(),
            order: 0,
            kind: DocumentKind::Scene,
            active: true,
            archived: false,
            status: "Draft".into(),
            synopsis: String::new(),
            pov: String::new(),
            location: String::new(),
            story_time: String::new(),
            tags: vec![],
            characters: vec![],
            plot_threads: vec![],
            beats: String::new(),
            ai_include: true,
            word_target: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CodexEntry {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub notes: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub relationships: String,
    pub progression: String,
    pub ai_include: bool,
}

impl Default for CodexEntry {
    fn default() -> Self {
        Self {
            id: new_id("codex"),
            name: "New Entry".into(),
            category: "Character".into(),
            description: String::new(),
            notes: String::new(),
            aliases: vec![],
            tags: vec![],
            relationships: String::new(),
            progression: String::new(),
            ai_include: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildProfile {
    pub name: String,
    pub include_inactive: bool,
    pub include_notes: bool,
    pub page_break_documents: bool,
    pub include_comments: bool,
}

impl Default for BuildProfile {
    fn default() -> Self {
        Self {
            name: "Submission Manuscript".into(),
            include_inactive: false,
            include_notes: false,
            page_break_documents: true,
            include_comments: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectManifest {
    pub version: u32,
    pub name: String,
    pub title: String,
    pub author: String,
    pub language: String,
    pub style_guide: String,
    pub documents: Vec<DocumentMeta>,
    pub codex: Vec<CodexEntry>,
    pub build_profiles: Vec<BuildProfile>,
    pub chat_messages: Vec<ChatMessage>,
    pub trash: Vec<TrashItem>,
}

impl Default for ProjectManifest {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Novel".into(),
            title: String::new(),
            author: String::new(),
            language: "English".into(),
            style_guide: String::new(),
            documents: vec![],
            codex: vec![],
            build_profiles: vec![BuildProfile::default()],
            chat_messages: vec![],
            trash: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashItem {
    pub id: String,
    pub original_path: String,
    pub stored_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

fn new_id(prefix: &str) -> String {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{time:x}-{counter:x}")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("file")
    ));
    fs::write(&temporary, bytes)
        .with_context(|| format!("Could not write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("Could not save {}", path.display()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ollama_url: String,
    pub model: String,
    pub temperature: f32,
    pub autosave: bool,
    pub last_project: Option<PathBuf>,
    pub target_words: usize,
    pub font_size: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ollama_url: "http://127.0.0.1:11434".into(),
            model: String::new(),
            temperature: 0.7,
            autosave: true,
            last_project: None,
            target_words: 80_000,
            font_size: 18.0,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path().context("Could not locate the settings folder")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

fn settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("studio", "NovelQuill", "Novel Quill Studio")
        .map(|dirs| dirs.config_dir().join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("novel-quill-{name}-{}", std::process::id()))
    }

    #[test]
    fn project_discovers_only_writing_files() {
        let root = test_root("scan");
        fs::create_dir_all(root.join("Chapters")).unwrap();
        fs::write(root.join("Chapters/One.md"), "hello world").unwrap();
        fs::write(root.join("cover.png"), "not really an image").unwrap();
        let project = Project::open(root.clone()).unwrap();
        assert!(project.files.iter().any(|f| f.path.ends_with("One.md")));
        assert!(!project.files.iter().any(|f| f.path.ends_with("cover.png")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn document_tracks_dirty_and_saved_content() {
        let root = test_root("document");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chapter.md");
        fs::write(&path, "first draft").unwrap();
        let mut document = Document::open(path.clone()).unwrap();
        assert!(!document.is_dirty());
        document.content.push_str(" revised");
        assert!(document.is_dirty());
        document.save().unwrap();
        assert!(!document.is_dirty());
        assert_eq!(fs::read_to_string(path).unwrap(), "first draft revised");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_rejects_parent_directory_escape() {
        let root = test_root("escape");
        fs::create_dir_all(&root).unwrap();
        let mut project = Project::open(root.clone()).unwrap();
        assert!(project.create_document("../outside.md").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_persists_metadata_and_revision_snapshots() {
        let root = test_root("metadata");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("scene.md");
        fs::write(&path, "original").unwrap();
        let mut project = Project::open(root.clone()).unwrap();
        project.document_meta_mut(&path).unwrap().pov = "Mara".into();
        project.save_manifest().unwrap();
        let mut document = Document::open(path.clone()).unwrap();
        document.content = "revised".into();
        project.save_document(&mut document).unwrap();
        assert_eq!(project.revisions(&path).len(), 1);
        let reopened = Project::open(root.clone()).unwrap();
        assert_eq!(reopened.document_meta(&path).unwrap().pov, "Mara");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_document_copies_content_and_metadata() {
        let root = test_root("duplicate");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("scene.md");
        fs::write(&path, "scene text").unwrap();
        let mut project = Project::open(root.clone()).unwrap();
        project.document_meta_mut(&path).unwrap().synopsis = "A discovery".into();
        let copy = project.duplicate_document(&path).unwrap();
        assert_eq!(fs::read_to_string(&copy).unwrap(), "scene text");
        assert_eq!(
            project.document_meta(&copy).unwrap().synopsis,
            "A discovery"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn split_sources_are_recoverable_from_trash() {
        let root = test_root("split-trash");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chapter.md");
        fs::write(&path, "# Chapter\n\n## One\nFirst.\n\n## Two\nSecond.\n").unwrap();
        let mut project = Project::open(root.clone()).unwrap();
        let created = project.split_document_at_headings(&path).unwrap();
        assert_eq!(created.len(), 2);
        assert!(!path.exists());
        assert_eq!(project.manifest.trash.len(), 1);
        let restored = project.restore_trash(0).unwrap();
        assert_eq!(restored, path);
        assert!(restored.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
