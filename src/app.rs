use crate::{
    model::{
        ChatMessage, CodexEntry, Document, DocumentKind, Project, PromptPreset, RecoveryItem,
        Settings,
    },
    ollama::{GenerateRequest, OllamaClient, OllamaRequest, OllamaResponse},
    updates::{UpdateChecker, UpdateStatus},
};
use eframe::egui::{self, Color32, FontId, RichText, TextStyle};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const SYSTEM_PROMPT: &str = "You are a careful fiction-writing collaborator. Preserve the author's voice, characters, point of view, tense, world facts, and Markdown formatting. Never add commentary before or after requested prose. Do not censor ordinary fictional conflict. Return only useful manuscript text unless the author asks for analysis.";

#[derive(Clone, Copy, PartialEq)]
enum CenterView {
    Editor,
    Preview,
    Split,
}

#[derive(Clone, Copy, PartialEq)]
enum AiAction {
    Continue,
    Rewrite,
    Brainstorm,
    Summarize,
    Critique,
    SceneBeats,
    Extract,
    Chat,
    Custom,
}

impl AiAction {
    fn label(self) -> &'static str {
        match self {
            Self::Continue => "Continue at cursor",
            Self::Rewrite => "Rewrite selection",
            Self::Brainstorm => "Brainstorm ideas",
            Self::Summarize => "Summarize scene",
            Self::Critique => "Critique scene",
            Self::SceneBeats => "Extract scene beats",
            Self::Extract => "Update story data",
            Self::Chat => "Story chat",
            Self::Custom => "Custom instruction",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ContinueLength {
    Sentence,
    Paragraph,
}

#[derive(Clone, Copy, PartialEq)]
enum SidebarView {
    Files,
    Outline,
    Search,
    Codex,
    Review,
}

#[derive(Clone, Copy, PartialEq)]
enum PlannerView {
    Cards,
    Matrix,
    Timeline,
}

#[derive(Clone)]
struct SearchHit {
    path: PathBuf,
    line: usize,
    excerpt: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct SceneExtraction {
    synopsis: String,
    pov: String,
    location: String,
    story_time: String,
    tags: Vec<String>,
    characters: Vec<String>,
    plot_threads: Vec<String>,
    beats: String,
    codex: Vec<ExtractedCodex>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ExtractedCodex {
    name: String,
    category: String,
    description: String,
}

pub struct NovelQuillApp {
    project: Option<Project>,
    documents: Vec<Document>,
    active: Option<usize>,
    settings: Settings,
    ollama: OllamaClient,
    updates: UpdateChecker,
    update_available: Option<(String, String)>,
    manual_update_check: bool,
    models: Vec<String>,
    ai_prompt: String,
    ai_output: String,
    ai_comparison_original: String,
    ai_comparison_ai: String,
    ai_generation_action: AiAction,
    ai_action: AiAction,
    continue_length: ContinueLength,
    cursor_char: Option<usize>,
    selection_chars: Option<(usize, usize)>,
    pinned_document: Option<PathBuf>,
    ai_insert_target: Option<(PathBuf, usize)>,
    ai_replace_target: Option<(PathBuf, usize, usize)>,
    ai_target_snapshot: Option<(PathBuf, String)>,
    ai_busy: bool,
    ai_panel: bool,
    left_panel: bool,
    center_view: CenterView,
    status: String,
    search: String,
    sidebar_view: SidebarView,
    planner_view: PlannerView,
    global_search: String,
    search_hits: Vec<SearchHit>,
    spelling_issues: Vec<String>,
    selected_codex: Option<usize>,
    new_item_name: String,
    show_new_document: bool,
    show_new_folder: bool,
    show_settings: bool,
    show_document_meta: bool,
    show_project_settings: bool,
    show_revisions: bool,
    show_rename: bool,
    show_trash: bool,
    show_recovery: bool,
    show_find_replace: bool,
    show_prompt_preview: bool,
    show_ai_compare: bool,
    pending_trash: Option<PathBuf>,
    find_text: String,
    replace_text: String,
    rename_name: String,
    prompt_preview: String,
    generation_history: Vec<String>,
    generation_was_chat: bool,
    ai_undo_stack: Vec<(PathBuf, String)>,
    last_generation_request: Option<GenerateRequest>,
    alternatives_remaining: usize,
    selected_prompt_preset: Option<usize>,
    prompt_preset_name: String,
    focus_mode: bool,
    last_edit: Instant,
    last_recovery: Instant,
    session_started: Instant,
    session_start_words: usize,
}

impl NovelQuillApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        let settings = Settings::load();
        let project = settings
            .last_project
            .clone()
            .and_then(|path| Project::open(path).ok());
        let session_start_words = project
            .as_ref()
            .map(|project| {
                project
                    .files
                    .iter()
                    .filter(|entry| !entry.is_dir)
                    .filter_map(|entry| fs::read_to_string(&entry.path).ok())
                    .map(|text| text.split_whitespace().count())
                    .sum()
            })
            .unwrap_or(0);
        let show_recovery = project
            .as_ref()
            .and_then(|project| project.recovery_items().ok())
            .is_some_and(|items| !items.is_empty());
        let ollama = OllamaClient::new();
        ollama.send(OllamaRequest::ListModels {
            base_url: settings.ollama_url.clone(),
        });
        let updates = UpdateChecker::new();
        if settings.check_updates {
            updates.check();
        }
        Self {
            project,
            documents: vec![],
            active: None,
            settings,
            ollama,
            updates,
            update_available: None,
            manual_update_check: false,
            models: vec![],
            ai_prompt: String::new(),
            ai_output: String::new(),
            ai_comparison_original: String::new(),
            ai_comparison_ai: String::new(),
            ai_generation_action: AiAction::Continue,
            ai_action: AiAction::Continue,
            continue_length: ContinueLength::Paragraph,
            cursor_char: None,
            selection_chars: None,
            pinned_document: None,
            ai_insert_target: None,
            ai_replace_target: None,
            ai_target_snapshot: None,
            ai_busy: false,
            ai_panel: true,
            left_panel: true,
            center_view: CenterView::Editor,
            status: "Ready".into(),
            search: String::new(),
            sidebar_view: SidebarView::Files,
            planner_view: PlannerView::Cards,
            global_search: String::new(),
            search_hits: vec![],
            spelling_issues: vec![],
            selected_codex: None,
            new_item_name: String::new(),
            show_new_document: false,
            show_new_folder: false,
            show_settings: false,
            show_document_meta: false,
            show_project_settings: false,
            show_revisions: false,
            show_rename: false,
            show_trash: false,
            show_recovery,
            show_find_replace: false,
            show_prompt_preview: false,
            show_ai_compare: false,
            pending_trash: None,
            find_text: String::new(),
            replace_text: String::new(),
            rename_name: String::new(),
            prompt_preview: String::new(),
            generation_history: vec![],
            generation_was_chat: false,
            ai_undo_stack: vec![],
            last_generation_request: None,
            alternatives_remaining: 0,
            selected_prompt_preset: None,
            prompt_preset_name: String::new(),
            focus_mode: false,
            last_edit: Instant::now(),
            last_recovery: Instant::now(),
            session_started: Instant::now(),
            session_start_words,
        }
    }

    fn open_project(&mut self, path: PathBuf) {
        if self
            .project
            .as_ref()
            .is_some_and(|project| project.root == path)
        {
            if let Some(project) = &mut self.project {
                match project.refresh() {
                    Ok(()) => self.status = format!("Refreshed {}", project.name()),
                    Err(error) => self.status = error.to_string(),
                }
            }
            return;
        }
        match Project::open(path) {
            Ok(project) => {
                self.save_all();
                self.documents.clear();
                self.active = None;
                self.settings.last_project = Some(project.root.clone());
                let _ = self.settings.save();
                self.status = format!("Opened {}", project.name());
                self.project = Some(project);
                self.show_recovery = self
                    .project
                    .as_ref()
                    .and_then(|project| project.recovery_items().ok())
                    .is_some_and(|items| !items.is_empty());
                self.session_started = Instant::now();
                self.session_start_words = self.project_word_count();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn open_document(&mut self, path: PathBuf) {
        if let Some(index) = self.documents.iter().position(|doc| doc.path == path) {
            self.active = Some(index);
            self.cursor_char = None;
            self.selection_chars = None;
            return;
        }
        match Document::open(path) {
            Ok(document) => {
                self.documents.push(document);
                self.active = Some(self.documents.len() - 1);
                self.cursor_char = None;
                self.selection_chars = None;
                self.status = "Document opened".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn save_active(&mut self) {
        let Some(index) = self.active else { return };
        let result = if let Some(project) = &self.project {
            project.save_document(&mut self.documents[index])
        } else {
            self.documents[index].save()
        };
        match result {
            Ok(()) => self.status = format!("Saved {}", self.documents[index].title()),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn save_all(&mut self) {
        for document in &mut self.documents {
            if document.is_dirty() {
                if let Some(project) = &self.project {
                    let _ = project.save_document(document);
                } else {
                    let _ = document.save();
                }
            }
        }
    }

    fn close_document(&mut self, index: usize) {
        if index >= self.documents.len() {
            return;
        }
        if self.documents[index].is_dirty() {
            let result = if let Some(project) = &self.project {
                project.save_document(&mut self.documents[index])
            } else {
                self.documents[index].save()
            };
            if let Err(error) = result {
                self.status = error.to_string();
                return;
            }
        }
        self.documents.remove(index);
        self.active = match self.active {
            None => None,
            Some(_) if self.documents.is_empty() => None,
            Some(active) if active == index => Some(index.min(self.documents.len() - 1)),
            Some(active) if active > index => Some(active - 1),
            value => value,
        };
    }

    fn create_document(&mut self) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.create_document(&self.new_item_name) {
            Ok(path) => {
                self.new_item_name.clear();
                self.show_new_document = false;
                self.open_document(path);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn create_folder(&mut self) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.create_folder(&self.new_item_name) {
            Ok(()) => {
                self.new_item_name.clear();
                self.show_new_folder = false;
                self.status = "Folder created".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn save_prompt_preset(&mut self) {
        let name = self.prompt_preset_name.trim();
        if name.is_empty() || self.ai_prompt.trim().is_empty() {
            self.status = "Enter a preset name and custom instructions first".into();
            return;
        }
        let Some(project) = &mut self.project else {
            return;
        };
        project.manifest.prompt_presets.push(PromptPreset {
            name: name.to_owned(),
            instructions: self.ai_prompt.trim().to_owned(),
        });
        self.selected_prompt_preset = Some(project.manifest.prompt_presets.len() - 1);
        match project.save_manifest() {
            Ok(()) => self.status = format!("Saved prompt preset “{name}”"),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn delete_prompt_preset(&mut self) {
        let Some(index) = self.selected_prompt_preset else {
            return;
        };
        let Some(project) = &mut self.project else {
            return;
        };
        if index < project.manifest.prompt_presets.len() {
            project.manifest.prompt_presets.remove(index);
            self.selected_prompt_preset = None;
            self.prompt_preset_name.clear();
            match project.save_manifest() {
                Ok(()) => self.status = "Deleted prompt preset".into(),
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    fn request_ai(&mut self) {
        if self.ai_busy {
            return;
        }
        if self.settings.model.is_empty() {
            self.status = "Choose an Ollama model first".into();
            return;
        }
        let Some(document) = self.active.and_then(|i| self.documents.get(i)) else {
            return;
        };
        let cursor_char = self
            .cursor_char
            .unwrap_or_else(|| document.content.chars().count())
            .min(document.content.chars().count());
        let cursor_byte = char_to_byte_index(&document.content, cursor_char);
        let selection = self.selection_chars.and_then(|(a, b)| {
            let start = a.min(b);
            let end = a.max(b);
            (start < end).then_some((start, end))
        });
        let selected_text = selection.map(|(start, end)| {
            let start = char_to_byte_index(&document.content, start);
            let end = char_to_byte_index(&document.content, end);
            document.content[start..end].to_owned()
        });
        self.ai_comparison_original = document.content.clone();
        self.ai_comparison_ai.clear();
        self.ai_generation_action = self.ai_action;
        let mut manuscript_at_cursor = document.content.clone();
        manuscript_at_cursor.insert_str(cursor_byte, "<<<CURSOR>>>");
        self.ai_insert_target = Some((document.path.clone(), cursor_char));
        self.ai_replace_target = selection.map(|(start, end)| (document.path.clone(), start, end));
        self.ai_target_snapshot = Some((document.path.clone(), document.content.clone()));
        let story_context = self.story_context(&document.path, &document.content);
        let task = match self.ai_action {
            AiAction::Continue => {
                let length = match self.continue_length {
                    ContinueLength::Sentence => "exactly one complete sentence",
                    ContinueLength::Paragraph => "exactly one cohesive paragraph",
                };
                format!(
                    "Write {length} at the <<<CURSOR>>> marker. It must connect naturally to the text on both sides, match the existing voice and tense, and avoid repeating nearby prose. Return only the new text to insert; do not include the marker or any explanation.\n\nCHAPTER:\n{manuscript_at_cursor}"
                )
            }
            AiAction::Rewrite => format!(
                "Rewrite the text below to improve clarity, rhythm, imagery, and emotional impact while preserving every plot fact, voice, tense, and Markdown structure. Return only the replacement text.\n\nTEXT:\n{}",
                selected_text.as_deref().unwrap_or(&document.content)
            ),
            AiAction::Brainstorm => format!(
                "Suggest five distinct, specific possibilities for what could happen next. Be concise and respect the established chapter.\n\nCHAPTER:\n{}",
                document.content
            ),
            AiAction::Summarize => format!(
                "Summarize this scene in 2-4 factual sentences for use as continuity context. Include important decisions, discoveries, injuries, relationship changes, and unresolved threads.\n\nSCENE:\n{}",
                document.content
            ),
            AiAction::Critique => format!(
                "Review this scene without rewriting it. Identify the five highest-impact issues involving character motivation, pacing, continuity, POV, dialogue, or prose, then give specific revision advice.\n\nSCENE:\n{}",
                document.content
            ),
            AiAction::SceneBeats => format!(
                "Extract the scene into an ordered list of concise dramatic beats. State the goal, conflict, turning point, and outcome.\n\nSCENE:\n{}",
                document.content
            ),
            AiAction::Extract => format!(
                "Extract structured story information from this scene. Return only valid JSON with this exact shape and no Markdown fence: {{\"synopsis\":\"2-4 factual sentences\",\"pov\":\"POV character or viewpoint\",\"location\":\"primary location\",\"story_time\":\"time information or empty\",\"tags\":[\"tag\"],\"characters\":[\"name\"],\"plot_threads\":[\"thread\"],\"beats\":\"ordered concise beats\",\"codex\":[{{\"name\":\"name\",\"category\":\"Character|Location|Object|Faction|Lore\",\"description\":\"facts established in this scene\"}}]}}. Include only facts supported by the scene.\n\nSCENE:\n{}",
                document.content
            ),
            AiAction::Chat => {
                let conversation = self
                    .project
                    .as_ref()
                    .map(|project| {
                        project
                            .manifest
                            .chat_messages
                            .iter()
                            .rev()
                            .take(12)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .map(|message| format!("{}: {}", message.role, message.content))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                format!(
                    "Continue this story-development conversation. Give specific, practical help grounded in the provided context.\n\nCONVERSATION:\n{conversation}\nAuthor: {}",
                    self.ai_prompt.trim()
                )
            }
            AiAction::Custom => format!(
                "AUTHOR REQUEST:\n{}\n\nCURRENT CHAPTER (the insertion point is marked if relevant):\n{manuscript_at_cursor}",
                self.ai_prompt.trim(),
            ),
        };
        let prompt = format!("{story_context}\n\nTASK\n{task}");
        self.prompt_preview = format!("SYSTEM\n{SYSTEM_PROMPT}\n\n{prompt}");
        self.generation_was_chat = self.ai_action == AiAction::Chat;
        if self.generation_was_chat
            && let Some(project) = &mut self.project
        {
            project.manifest.chat_messages.push(ChatMessage {
                role: "Author".into(),
                content: self.ai_prompt.trim().to_owned(),
            });
            let _ = project.save_manifest();
            self.ai_prompt.clear();
        }
        self.ai_output.clear();
        self.ai_busy = true;
        self.status = "Ollama is writing…".into();
        let request = GenerateRequest {
            base_url: self.settings.ollama_url.clone(),
            model: self.settings.model.clone(),
            system: SYSTEM_PROMPT.into(),
            prompt,
            temperature: self.settings.temperature,
            top_p: self.settings.top_p,
            repeat_penalty: self.settings.repeat_penalty,
            context_length: self.settings.context_length,
            max_output_tokens: self.settings.max_output_tokens,
        };
        self.last_generation_request = Some(request.clone());
        self.ollama.send(OllamaRequest::Generate(request));
    }

    fn regenerate_ai(&mut self) {
        let Some(request) = self.last_generation_request.clone() else {
            self.status = "Generate a suggestion before requesting an alternative".into();
            return;
        };
        if self.ai_busy {
            return;
        }
        self.ai_output.clear();
        self.ai_busy = true;
        self.status = "Ollama is generating an alternative…".into();
        self.ollama.send(OllamaRequest::Generate(request));
    }

    fn generate_alternative_batch(&mut self, count: usize) {
        if self.last_generation_request.is_none() {
            self.status = "Generate a suggestion before requesting alternatives".into();
            return;
        }
        if self.ai_busy || count == 0 {
            return;
        }
        self.alternatives_remaining = count;
        self.start_next_alternative();
    }

    fn start_next_alternative(&mut self) {
        let Some(request) = self.last_generation_request.clone() else {
            self.alternatives_remaining = 0;
            return;
        };
        if self.alternatives_remaining == 0 {
            return;
        }
        self.alternatives_remaining -= 1;
        self.ai_output.clear();
        self.ai_busy = true;
        self.generation_was_chat = false;
        self.status = format!(
            "Ollama is generating alternatives… {} remaining after this one",
            self.alternatives_remaining
        );
        self.ollama.send(OllamaRequest::Generate(request));
    }

    fn ai_target_is_unchanged(&mut self) -> bool {
        let Some((path, snapshot)) = &self.ai_target_snapshot else {
            return true;
        };
        let unchanged = self
            .documents
            .iter()
            .find(|document| document.path == *path)
            .is_some_and(|document| document.content == *snapshot);
        if !unchanged {
            self.status =
                "The target document changed after generation. Generate again before applying AI text."
                    .into();
        }
        unchanged
    }

    fn story_context(&self, path: &std::path::Path, content: &str) -> String {
        let Some(project) = &self.project else {
            return "No additional story context is available.".into();
        };
        let mut context = String::from("STORY CONTEXT\n");
        if !project.manifest.title.is_empty() {
            context.push_str(&format!("Title: {}\n", project.manifest.title));
        }
        if self.settings.include_style_guide && !project.manifest.style_guide.is_empty() {
            context.push_str(&format!("Style guide: {}\n", project.manifest.style_guide));
        }
        if let Some(meta) = project.document_meta(path) {
            if self.settings.include_scene_metadata {
                context.push_str(&format!(
                    "Current {} — POV: {}; location: {}; story time: {}; status: {}\nSynopsis: {}\nScene beats: {}\nPlot threads: {}\n",
                    meta.kind.label(),
                    value_or_unknown(&meta.pov),
                    value_or_unknown(&meta.location),
                    value_or_unknown(&meta.story_time),
                    meta.status,
                    value_or_unknown(&meta.synopsis),
                    value_or_unknown(&meta.beats),
                    meta.plot_threads.join(", ")
                ));
            }
            if self.settings.include_previous_summaries {
                let mut earlier = project
                    .manifest
                    .documents
                    .iter()
                    .filter(|other| other.order < meta.order && !other.synopsis.is_empty())
                    .collect::<Vec<_>>();
                earlier.sort_by_key(|other| other.order);
                if !earlier.is_empty() {
                    context.push_str("Earlier scene summaries:\n");
                    for other in earlier
                        .into_iter()
                        .rev()
                        .take(self.settings.previous_summary_limit)
                        .rev()
                    {
                        context.push_str(&format!("- {}: {}\n", other.path, other.synopsis));
                    }
                }
            }
        }
        if self.settings.include_relevant_scenes {
            let passages = self.relevant_scene_passages(path, content);
            if !passages.is_empty() {
                context.push_str("Relevant manuscript passages:\n");
                for (label, excerpt, score) in passages {
                    context.push_str(&format!("- {label} (relevance {score}):\n{excerpt}\n"));
                }
            }
        }
        let lowercase = content.to_lowercase();
        let relevant = project.manifest.codex.iter().filter(|entry| {
            entry.ai_include
                && (lowercase.contains(&entry.name.to_lowercase())
                    || entry
                        .aliases
                        .iter()
                        .any(|alias| lowercase.contains(&alias.to_lowercase())))
        });
        let mut wrote_codex = false;
        for entry in relevant.take(if self.settings.include_codex {
            self.settings.codex_limit
        } else {
            0
        }) {
            if !wrote_codex {
                context.push_str("Relevant Codex entries:\n");
                wrote_codex = true;
            }
            context.push_str(&format!(
                "- {} [{}]: {} Relations: {} Current progression: {}\n",
                entry.name,
                entry.category,
                entry.description,
                entry.relationships,
                entry.progression
            ));
        }
        context
    }

    fn relevant_scene_passages(
        &self,
        current_path: &std::path::Path,
        current_content: &str,
    ) -> Vec<(String, String, usize)> {
        let Some(project) = &self.project else {
            return vec![];
        };
        let query_terms = meaningful_terms(current_content);
        if query_terms.is_empty() {
            return vec![];
        }
        let mut passages = project
            .files
            .iter()
            .filter(|entry| !entry.is_dir && entry.path != current_path)
            .filter(|entry| {
                project
                    .document_meta(&entry.path)
                    .is_none_or(|meta| meta.ai_include && !meta.archived)
            })
            .filter_map(|entry| {
                let text = self
                    .documents
                    .iter()
                    .find(|document| document.path == entry.path)
                    .map(|document| document.content.clone())
                    .or_else(|| fs::read_to_string(&entry.path).ok())?;
                let (score, excerpt) = best_relevant_excerpt(
                    &text,
                    &query_terms,
                    self.settings.relevant_scene_excerpt_chars,
                );
                (score > 0).then(|| {
                    let label = entry
                        .path
                        .strip_prefix(&project.root)
                        .unwrap_or(&entry.path)
                        .display()
                        .to_string();
                    (label, excerpt, score)
                })
            })
            .collect::<Vec<_>>();
        passages.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));
        passages.truncate(self.settings.relevant_scene_limit);
        passages
    }

    fn insert_ai_at_cursor(&mut self) {
        if !self.ai_target_is_unchanged() {
            return;
        }
        let Some((path, cursor_char)) = self.ai_insert_target.clone() else {
            self.status = "Generate a suggestion for the current cursor first".into();
            return;
        };
        let Some(index) = self.documents.iter().position(|doc| doc.path == path) else {
            self.status = "The document used for this suggestion is no longer open".into();
            return;
        };
        self.ai_undo_stack
            .push((path.clone(), self.documents[index].content.clone()));
        let document = &mut self.documents[index];
        let byte_index = char_to_byte_index(&document.content, cursor_char);
        let before = &document.content[..byte_index];
        let after = &document.content[byte_index..];
        let generated = self.ai_output.trim();
        let mut insertion = String::new();
        match self.continue_length {
            ContinueLength::Sentence => {
                if !before.is_empty() && !before.ends_with(char::is_whitespace) {
                    insertion.push(' ');
                }
                insertion.push_str(generated);
                if !after.is_empty() && !after.starts_with(char::is_whitespace) {
                    insertion.push(' ');
                }
            }
            ContinueLength::Paragraph => {
                if !before.is_empty() && !before.ends_with("\n\n") {
                    insertion.push_str(if before.ends_with('\n') { "\n" } else { "\n\n" });
                }
                insertion.push_str(generated);
                if !after.is_empty() && !after.starts_with("\n\n") {
                    insertion.push_str(if after.starts_with('\n') {
                        "\n"
                    } else {
                        "\n\n"
                    });
                }
            }
        }
        document.content.insert_str(byte_index, &insertion);
        self.active = Some(index);
        self.cursor_char = Some(cursor_char + insertion.chars().count());
        self.last_edit = Instant::now();
        self.status = format!("Inserted suggestion into {}", document.title());
    }

    fn replace_ai_target(&mut self) {
        if !self.ai_target_is_unchanged() {
            return;
        }
        if let Some((path, start_char, end_char)) = self.ai_replace_target.clone()
            && let Some(index) = self
                .documents
                .iter()
                .position(|document| document.path == path)
        {
            self.ai_undo_stack
                .push((path.clone(), self.documents[index].content.clone()));
            let start = char_to_byte_index(&self.documents[index].content, start_char);
            let end = char_to_byte_index(&self.documents[index].content, end_char);
            self.documents[index]
                .content
                .replace_range(start..end, self.ai_output.trim());
            self.active = Some(index);
            self.cursor_char = Some(start_char + self.ai_output.trim().chars().count());
            self.selection_chars = None;
            self.last_edit = Instant::now();
            self.status = "Selection replaced; use History or Undo if needed".into();
            return;
        }
        if let Some(index) = self.ai_insert_target.as_ref().and_then(|(path, _)| {
            self.documents
                .iter()
                .position(|document| document.path == *path)
        }) {
            let path = self.documents[index].path.clone();
            self.ai_undo_stack
                .push((path, self.documents[index].content.clone()));
            let document = &mut self.documents[index];
            document.content = self.ai_output.trim().to_owned() + "\n";
            self.last_edit = Instant::now();
            self.status = "Document replaced; use History or Undo if needed".into();
        }
    }

    fn undo_last_ai_edit(&mut self) {
        let Some((path, content)) = self.ai_undo_stack.pop() else {
            self.status = "No AI edit to undo".into();
            return;
        };
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path == path)
        {
            self.documents[index].content = content;
            self.active = Some(index);
            self.last_edit = Instant::now();
            self.status = "Restored the document from before the AI edit".into();
        } else {
            self.status = "The document for that AI edit is no longer open".into();
        }
    }

    fn apply_scene_extraction(&mut self) {
        if !self.ai_target_is_unchanged() {
            return;
        }
        let Some((path, _)) = self.ai_insert_target.clone() else {
            self.status = "Generate scene extraction for an open document first".into();
            return;
        };
        let text = self.ai_output.trim();
        let json = text
            .find('{')
            .zip(text.rfind('}'))
            .and_then(|(start, end)| (start <= end).then_some(&text[start..=end]));
        let Some(json) = json else {
            self.status = "The extraction did not contain valid JSON; regenerate it".into();
            return;
        };
        let extraction = match serde_json::from_str::<SceneExtraction>(json) {
            Ok(extraction) => extraction,
            Err(error) => {
                self.status = format!("Could not read extraction: {error}");
                return;
            }
        };
        let Some(project) = &mut self.project else {
            return;
        };
        if let Some(meta) = project.document_meta_mut(&path) {
            meta.synopsis = extraction.synopsis;
            meta.pov = extraction.pov;
            meta.location = extraction.location;
            meta.story_time = extraction.story_time;
            meta.tags = extraction.tags;
            meta.characters = extraction.characters;
            meta.plot_threads = extraction.plot_threads;
            meta.beats = extraction.beats;
        }
        let mut added = 0;
        for item in extraction.codex {
            if item.name.trim().is_empty()
                || project
                    .manifest
                    .codex
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(item.name.trim()))
            {
                continue;
            }
            let entry = CodexEntry {
                name: item.name.trim().to_owned(),
                category: if item.category.trim().is_empty() {
                    "Lore".into()
                } else {
                    item.category.trim().to_owned()
                },
                description: item.description.trim().to_owned(),
                ..CodexEntry::default()
            };
            project.manifest.codex.push(entry);
            added += 1;
        }
        match project.save_manifest() {
            Ok(()) => {
                self.status = format!(
                    "Applied scene metadata and added {added} new Codex entr{}",
                    if added == 1 { "y" } else { "ies" }
                )
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn compose_ai_document_version(&mut self) {
        let mut version = self.ai_comparison_original.clone();
        if self.ai_generation_action == AiAction::Continue {
            if let Some((_, cursor_char)) = &self.ai_insert_target {
                let byte = char_to_byte_index(&version, *cursor_char);
                let insertion =
                    ai_insertion_text(&version, byte, self.ai_output.trim(), self.continue_length);
                version.insert_str(byte, &insertion);
            }
        } else if let Some((_, start_char, end_char)) = &self.ai_replace_target {
            let start = char_to_byte_index(&version, *start_char);
            let end = char_to_byte_index(&version, *end_char);
            version.replace_range(start..end, self.ai_output.trim());
        } else {
            version = self.ai_output.clone();
        }
        self.ai_comparison_ai = version;
    }

    fn accept_ai_document_version(&mut self) {
        if !self.ai_target_is_unchanged() {
            return;
        }
        let path = self
            .ai_replace_target
            .as_ref()
            .map(|target| target.0.clone())
            .or_else(|| {
                self.ai_insert_target
                    .as_ref()
                    .map(|target| target.0.clone())
            })
            .or_else(|| {
                self.active
                    .and_then(|index| self.documents.get(index))
                    .map(|document| document.path.clone())
            });
        let Some(path) = path else { return };
        let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path == path)
        else {
            self.status = "The handwritten document is no longer open".into();
            return;
        };
        self.ai_undo_stack
            .push((path, self.documents[index].content.clone()));
        self.documents[index]
            .content
            .clone_from(&self.ai_comparison_ai);
        self.active = Some(index);
        self.last_edit = Instant::now();
        self.status =
            "Accepted the complete AI-assisted version; Undo AI edit remains available".into();
    }

    fn poll_ollama(&mut self, ctx: &egui::Context) {
        while let Some(response) = self.ollama.try_recv() {
            match response {
                OllamaResponse::Models(models) => {
                    self.models = models;
                    if self.settings.model.is_empty() {
                        self.settings.model = self.models.first().cloned().unwrap_or_default();
                    }
                    self.status = if self.models.is_empty() {
                        "Ollama is running, but no models are installed".into()
                    } else {
                        "Connected to Ollama".into()
                    };
                }
                OllamaResponse::GeneratedChunk(output) => {
                    self.ai_output.push_str(&output);
                }
                OllamaResponse::Finished => {
                    self.ai_busy = false;
                    if !self.ai_output.trim().is_empty() {
                        self.generation_history.push(self.ai_output.clone());
                        if self.generation_history.len() > 20 {
                            self.generation_history.remove(0);
                        }
                        if self.generation_was_chat
                            && let Some(project) = &mut self.project
                        {
                            project.manifest.chat_messages.push(ChatMessage {
                                role: "Assistant".into(),
                                content: self.ai_output.clone(),
                            });
                            let _ = project.save_manifest();
                        }
                    }
                    self.generation_was_chat = false;
                    if self.alternatives_remaining > 0 {
                        self.start_next_alternative();
                    } else {
                        self.status = "Suggestion ready".into();
                    }
                }
                OllamaResponse::Cancelled => {
                    self.ai_busy = false;
                    self.alternatives_remaining = 0;
                    self.status = "Generation stopped".into();
                }
                OllamaResponse::Error(error) => {
                    self.ai_busy = false;
                    self.alternatives_remaining = 0;
                    self.status = error;
                }
            }
            ctx.request_repaint();
        }
        if self.ai_busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn poll_updates(&mut self, ctx: &egui::Context) {
        while let Some(update) = self.updates.try_recv() {
            match update {
                UpdateStatus::UpToDate => {
                    if self.manual_update_check {
                        self.status = "Novel Quill Studio is up to date".into();
                    }
                }
                UpdateStatus::Available { version, url } => {
                    self.status = format!("Novel Quill Studio {version} is available");
                    self.update_available = Some((version, url));
                }
                UpdateStatus::Error(error) => {
                    if self.manual_update_check {
                        self.status = format!("Could not check for updates: {error}");
                    }
                }
            }
            self.manual_update_check = false;
            ctx.request_repaint();
        }
        if self.updates.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn export_manuscript(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}-manuscript.md", project.name()))
            .add_filter("Markdown", &["md"])
            .save_file()
        else {
            return;
        };
        let mut output = String::new();
        for text in self.manuscript_documents() {
            if !output.is_empty() {
                output.push_str("\n\n---\n\n");
            }
            output.push_str(&text);
        }
        match fs::write(path, output) {
            Ok(()) => self.status = "Manuscript exported".into(),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn export_pdf(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}-manuscript.pdf", project.name()))
            .add_filter("PDF document", &["pdf"])
            .save_file()
        else {
            return;
        };
        let documents = self.manuscript_documents();
        match crate::pdf::export_markdown_documents(
            &path,
            &project.name(),
            documents.iter().map(String::as_str),
        ) {
            Ok(()) => self.status = format!("PDF exported to {}", path.display()),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn export_portable(&mut self, format: &str) {
        let Some(project) = &self.project else { return };
        let extension = format.to_lowercase();
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}-manuscript.{extension}", project.name()))
            .add_filter(format, &[extension.as_str()])
            .save_file()
        else {
            return;
        };
        let title = if project.manifest.title.is_empty() {
            project.name()
        } else {
            project.manifest.title.clone()
        };
        let author = project.manifest.author.clone();
        let documents = self.manuscript_documents();
        let result = match format {
            "DOCX" => crate::export::docx(&path, &title, &author, &documents),
            "ODT" => crate::export::odt(&path, &title, &author, &documents),
            "EPUB" => crate::export::epub(&path, &title, &author, &documents),
            "HTML" => crate::export::html(&path, &title, &author, &documents),
            _ => return,
        };
        match result {
            Ok(()) => self.status = format!("{format} exported to {}", path.display()),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn manuscript_documents(&self) -> Vec<String> {
        let Some(project) = &self.project else {
            return vec![];
        };
        project
            .files
            .iter()
            .filter(|entry| !entry.is_dir)
            .filter(|entry| {
                let profile = project.manifest.build_profiles.first();
                project.document_meta(&entry.path).is_none_or(|meta| {
                    !meta.archived
                        && (meta.active || profile.is_some_and(|profile| profile.include_inactive))
                        && (!matches!(meta.kind, DocumentKind::Note | DocumentKind::Research)
                            || profile.is_some_and(|profile| profile.include_notes))
                })
            })
            .filter_map(|entry| {
                self.documents
                    .iter()
                    .find(|document| document.path == entry.path)
                    .map(|document| document.content.clone())
                    .or_else(|| fs::read_to_string(&entry.path).ok())
            })
            .map(|text| {
                if project
                    .manifest
                    .build_profiles
                    .first()
                    .is_some_and(|profile| profile.include_comments)
                {
                    text
                } else {
                    remove_html_comments(&text)
                }
            })
            .collect()
    }

    fn project_word_count(&self) -> usize {
        self.manuscript_documents()
            .iter()
            .map(|text| text.split_whitespace().count())
            .sum()
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|input| {
            input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            )
        }) {
            self.save_all();
            self.status = "Saved all open documents".into();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save_active();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F))
            && self.active.is_some()
        {
            self.show_find_replace = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            self.open_project(path);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N))
            && self.project.is_some()
        {
            self.show_new_document = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::P)) {
            self.center_view = if self.center_view == CenterView::Preview {
                CenterView::Editor
            } else {
                CenterView::Preview
            };
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            self.focus_mode = !self.focus_mode;
        }
        let ai_ready = self.active.is_some()
            && !self.ai_busy
            && !self.settings.model.is_empty()
            && (!matches!(self.ai_action, AiAction::Custom | AiAction::Chat)
                || !self.ai_prompt.trim().is_empty());
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
            && ai_ready
        {
            self.request_ai();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if self.ai_busy {
                self.ollama.cancel();
                self.status = "Stopping generation…".into();
            } else if self.pinned_document.take().is_some() {
                self.center_view = CenterView::Editor;
                self.status = "Closed side-by-side document comparison".into();
            }
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open project…   Ctrl+O").clicked() {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.open_project(path);
                        }
                    }
                    if ui
                        .add_enabled(
                            self.project.is_some(),
                            egui::Button::new("New document   Ctrl+N"),
                        )
                        .clicked()
                    {
                        self.show_new_document = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.active.is_some(), egui::Button::new("Save   Ctrl+S"))
                        .clicked()
                    {
                        self.save_active();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.project.is_some(),
                            egui::Button::new("Export combined manuscript…"),
                        )
                        .clicked()
                    {
                        self.export_manuscript();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.project.is_some(),
                            egui::Button::new("Export manuscript as PDF…"),
                        )
                        .clicked()
                    {
                        self.export_pdf();
                        ui.close_menu();
                    }
                    ui.menu_button("Export publishing formats", |ui| {
                        for format in ["DOCX", "ODT", "EPUB", "HTML"] {
                            if ui.button(format).clicked() {
                                self.export_portable(format);
                                ui.close_menu();
                            }
                        }
                    });
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.left_panel, "Project sidebar");
                    ui.checkbox(&mut self.ai_panel, "Ollama assistant");
                    ui.checkbox(&mut self.focus_mode, "Focus mode");
                    if ui
                        .add_enabled(
                            self.pinned_document.is_some(),
                            egui::Button::new("Close document comparison"),
                        )
                        .clicked()
                    {
                        self.pinned_document = None;
                        self.center_view = CenterView::Editor;
                        self.status = "Closed side-by-side document comparison".into();
                        ui.close_menu();
                    }
                });
                ui.menu_button("Project", |ui| {
                    if ui
                        .add_enabled(
                            self.project.is_some(),
                            egui::Button::new("Project settings…"),
                        )
                        .clicked()
                    {
                        self.show_project_settings = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.active.is_some(), egui::Button::new("Scene metadata…"))
                        .clicked()
                    {
                        self.show_document_meta = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.active.is_some(),
                            egui::Button::new("Revision history…"),
                        )
                        .clicked()
                    {
                        self.show_revisions = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.project
                                .as_ref()
                                .and_then(|project| project.recovery_items().ok())
                                .is_some_and(|items| !items.is_empty()),
                            egui::Button::new("Recover unsaved work…"),
                        )
                        .clicked()
                    {
                        self.show_recovery = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(
                            self.active.is_some(),
                            egui::Button::new("Find and replace…"),
                        )
                        .clicked()
                    {
                        self.show_find_replace = true;
                        ui.close_menu();
                    }
                });
                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }
                ui.menu_button("Help", |ui| {
                    if ui
                        .add_enabled(
                            !self.updates.is_busy(),
                            egui::Button::new("Check for updates"),
                        )
                        .clicked()
                    {
                        self.manual_update_check = true;
                        self.updates.check();
                        self.status = "Checking for updates…".into();
                        ui.close_menu();
                    }
                    if let Some((version, url)) = &self.update_available
                        && ui.button(format!("Download {version}")).clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
                        ui.close_menu();
                    }
                    if ui.button("GitHub repository").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(
                            "https://github.com/kennethyork/novelquill",
                        ));
                        ui.close_menu();
                    }
                });
                ui.separator();
                ui.selectable_value(&mut self.center_view, CenterView::Editor, "Write");
                ui.selectable_value(&mut self.center_view, CenterView::Preview, "Preview");
                ui.selectable_value(&mut self.center_view, CenterView::Split, "Split");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.focus_mode {
                        ui.label(
                            RichText::new("FOCUS MODE · F11 to exit")
                                .small()
                                .color(Color32::from_rgb(205, 174, 102)),
                        );
                    } else if let Some(project) = &self.project {
                        ui.label(RichText::new(project.name()).strong());
                    }
                });
            });
        });
    }

    fn left_sidebar(&mut self, ctx: &egui::Context) {
        if !self.left_panel || self.focus_mode {
            return;
        }
        let mut open_path = None;
        egui::SidePanel::left("project")
            .default_width(310.0)
            .min_width(230.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.sidebar_view, SidebarView::Files, "Files");
                    ui.selectable_value(&mut self.sidebar_view, SidebarView::Outline, "Outline");
                    ui.selectable_value(&mut self.sidebar_view, SidebarView::Search, "Search");
                    ui.selectable_value(&mut self.sidebar_view, SidebarView::Codex, "Codex");
                    ui.selectable_value(&mut self.sidebar_view, SidebarView::Review, "Review");
                });
                ui.separator();
                if self.project.is_none() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.0);
                        ui.label(
                            "Open a folder containing your Markdown chapters, notes, and research.",
                        );
                        ui.add_space(10.0);
                        if ui.button("Open project folder").clicked()
                            && let Some(path) = rfd::FileDialog::new().pick_folder()
                        {
                            self.open_project(path);
                        }
                    });
                } else {
                    open_path = match self.sidebar_view {
                        SidebarView::Files => self.sidebar_files(ui),
                        SidebarView::Outline => self.sidebar_outline(ui),
                        SidebarView::Search => self.sidebar_search(ui),
                        SidebarView::Codex => {
                            self.sidebar_codex(ui);
                            None
                        }
                        SidebarView::Review => {
                            self.sidebar_review(ui);
                            None
                        }
                    };
                }
            });
        if let Some(path) = open_path {
            self.open_document(path);
        }
    }

    fn sidebar_files(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let mut open_path = None;
        ui.horizontal(|ui| {
            ui.heading("Manuscript");
            if ui
                .small_button("＋")
                .on_hover_text("New document")
                .clicked()
            {
                self.show_new_document = true;
            }
            ui.menu_button("•••", |ui| {
                if ui.button("New folder…").clicked() {
                    self.show_new_folder = true;
                    ui.close_menu();
                }
                if ui.button("Project Trash…").clicked() {
                    self.show_trash = true;
                    ui.close_menu();
                }
                let has_recovery = self
                    .project
                    .as_ref()
                    .and_then(|project| project.recovery_items().ok())
                    .is_some_and(|items| !items.is_empty());
                if ui
                    .add_enabled(has_recovery, egui::Button::new("Recover unsaved writing…"))
                    .clicked()
                {
                    self.show_recovery = true;
                    ui.close_menu();
                }
                if ui.button("Refresh files").clicked() {
                    if let Some(project) = &mut self.project
                        && let Err(error) = project.refresh()
                    {
                        self.status = error.to_string();
                    }
                    ui.close_menu();
                }
            });
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Filter files…")
                .desired_width(f32::INFINITY),
        );
        if self
            .project
            .as_ref()
            .is_some_and(|project| project.files.is_empty())
            && ui.button("Create novel starter structure").clicked()
            && let Some(project) = &mut self.project
        {
            match project.create_starter_structure() {
                Ok(()) => {
                    self.status =
                        "Created manuscript, story bible, research and archive folders".into()
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        let query = self.search.to_lowercase();
        let entries = self
            .project
            .as_ref()
            .map(|project| project.files.clone())
            .unwrap_or_default();
        let mut move_action = None;
        let mut duplicate_path = None;
        let mut pin_path = None;
        let mut trash_active = false;
        let mut reorder_action = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in entries {
                let name = entry
                    .path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("?")
                    .to_owned();
                if !query.is_empty() && !name.to_lowercase().contains(&query) {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.add_space(entry.depth as f32 * 12.0);
                    if entry.is_dir {
                        ui.label(
                            RichText::new(format!("▾ {name}"))
                                .color(Color32::from_rgb(180, 160, 110)),
                        );
                        return;
                    }
                    let active = self
                        .active
                        .and_then(|index| self.documents.get(index))
                        .is_some_and(|document| document.path == entry.path);
                    let meta = self
                        .project
                        .as_ref()
                        .and_then(|project| project.document_meta(&entry.path));
                    let icon = meta.map_or("·", |meta| match meta.kind {
                        DocumentKind::Scene => "S",
                        DocumentKind::Chapter => "C",
                        DocumentKind::Part => "P",
                        DocumentKind::Note => "N",
                        DocumentKind::Research => "R",
                    });
                    let enabled = meta.is_none_or(|meta| meta.active && !meta.archived);
                    let label = if enabled {
                        format!("{icon} {name}")
                    } else {
                        format!("○ {name}")
                    };
                    let document_response = ui.add(
                        egui::Button::new(label)
                            .selected(active)
                            .frame(false)
                            .sense(egui::Sense::click_and_drag()),
                    );
                    if document_response.clicked() {
                        open_path = Some(entry.path.clone());
                    }
                    document_response.dnd_set_drag_payload(entry.path.clone());
                    if let Some(source) = document_response.dnd_release_payload::<PathBuf>()
                        && source.as_path() != entry.path
                    {
                        reorder_action = Some((source.as_ref().clone(), entry.path.clone()));
                    }
                    ui.menu_button("⋯", |ui| {
                        let comparison_label = if self.pinned_document.as_ref() == Some(&entry.path)
                        {
                            "Close side-by-side comparison"
                        } else {
                            "Compare side by side"
                        };
                        if ui.button(comparison_label).clicked() {
                            pin_path = Some(entry.path.clone());
                            ui.close_menu();
                        }
                        if ui.button("Duplicate document").clicked() {
                            duplicate_path = Some(entry.path.clone());
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Move earlier").clicked() {
                            move_action = Some((entry.path.clone(), -1));
                            ui.close_menu();
                        }
                        if ui.button("Move later").clicked() {
                            move_action = Some((entry.path.clone(), 1));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(
                                RichText::new("Move to Project Trash…")
                                    .color(Color32::from_rgb(231, 137, 126)),
                            )
                            .clicked()
                        {
                            if active {
                                trash_active = true;
                            } else {
                                self.pending_trash = Some(entry.path.clone());
                            }
                            ui.close_menu();
                        }
                    });
                });
            }
        });
        if trash_active {
            self.pending_trash = self
                .active
                .and_then(|index| self.documents.get(index))
                .map(|document| document.path.clone());
            return None;
        }
        if let Some((source, target)) = reorder_action
            && let Some(project) = &mut self.project
        {
            match project.reorder_document_before(&source, &target) {
                Ok(()) => self.status = "Reordered document by drag and drop".into(),
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some((path, direction)) = move_action
            && let Some(project) = &mut self.project
            && let Err(error) = project.move_document(&path, direction)
        {
            self.status = error.to_string();
        }
        if let Some(path) = duplicate_path
            && let Some(project) = &mut self.project
        {
            match project.duplicate_document(&path) {
                Ok(path) => open_path = Some(path),
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(path) = pin_path {
            self.open_side_by_side(path);
        }
        open_path
    }

    fn open_side_by_side(&mut self, path: PathBuf) {
        if self.pinned_document.as_ref() == Some(&path) {
            self.pinned_document = None;
            self.center_view = CenterView::Editor;
            self.status = "Closed side-by-side document comparison".into();
            return;
        }
        let previous_active = self.active;
        if !self.documents.iter().any(|document| document.path == path) {
            match Document::open(path.clone()) {
                Ok(document) => self.documents.push(document),
                Err(error) => {
                    self.status = error.to_string();
                    return;
                }
            }
        }
        self.active = previous_active.or_else(|| {
            self.documents
                .iter()
                .position(|document| document.path == path)
        });
        self.pinned_document = Some(path);
        self.center_view = CenterView::Split;
        self.status = "Opened editable document copies side by side".into();
    }

    fn sidebar_outline(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        ui.heading("Story Outline");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.planner_view, PlannerView::Cards, "Cards");
            ui.selectable_value(&mut self.planner_view, PlannerView::Matrix, "Matrix");
            ui.selectable_value(&mut self.planner_view, PlannerView::Timeline, "Timeline");
        });
        ui.separator();
        let Some(project) = &self.project else {
            return None;
        };
        let mut rows = project
            .manifest
            .documents
            .iter()
            .filter(|meta| {
                !meta.archived && !matches!(meta.kind, DocumentKind::Note | DocumentKind::Research)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|meta| meta.order);
        if self.planner_view == PlannerView::Timeline {
            rows.sort_by_key(|meta| {
                (
                    meta.story_time.is_empty(),
                    meta.story_time.clone(),
                    meta.order,
                )
            });
        }
        let root = project.root.clone();
        let rows = rows
            .into_iter()
            .map(|meta| {
                let path = root.join(&meta.path);
                let words = self
                    .documents
                    .iter()
                    .find(|document| document.path == path)
                    .map(Document::word_count)
                    .or_else(|| {
                        fs::read_to_string(&path)
                            .ok()
                            .map(|text| text.split_whitespace().count())
                    })
                    .unwrap_or(0);
                (meta, path, words)
            })
            .collect::<Vec<_>>();
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| match self.planner_view {
            PlannerView::Cards => {
                for (meta, path, words) in rows {
                    ui.group(|ui| {
                        if ui
                            .link(format!("{}  ·  {} words", meta.path, words))
                            .clicked()
                        {
                            open = Some(path.clone());
                        }
                        ui.small(format!(
                            "{} · {} · POV {}",
                            meta.kind.label(),
                            meta.status,
                            value_or_unknown(&meta.pov)
                        ));
                        if !meta.synopsis.is_empty() {
                            ui.label(&meta.synopsis);
                        }
                    });
                }
            }
            PlannerView::Matrix => {
                egui::Grid::new("outline-matrix")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Scene");
                        ui.strong("POV");
                        ui.strong("Status");
                        ui.strong("Words");
                        ui.end_row();
                        for (meta, path, words) in rows {
                            if ui.link(&meta.path).clicked() {
                                open = Some(path);
                            }
                            ui.label(value_or_unknown(&meta.pov));
                            ui.label(&meta.status);
                            ui.label(words.to_string());
                            ui.end_row();
                        }
                    });
            }
            PlannerView::Timeline => {
                for (meta, path, _) in rows {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(value_or_unknown(&meta.story_time)).monospace());
                        if ui.link(&meta.path).clicked() {
                            open = Some(path);
                        }
                    });
                    if !meta.synopsis.is_empty() {
                        ui.small(&meta.synopsis);
                    }
                    ui.separator();
                }
            }
        });
        open
    }

    fn sidebar_search(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        ui.heading("Project Search");
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.global_search)
                .hint_text("Search manuscript and notes…")
                .desired_width(f32::INFINITY),
        );
        if ui.button("Search all files").clicked()
            || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
        {
            self.run_project_search();
        }
        ui.label(format!("{} matches", self.search_hits.len()));
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for hit in &self.search_hits {
                if ui
                    .link(format!(
                        "{}:{}",
                        hit.path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("?"),
                        hit.line
                    ))
                    .clicked()
                {
                    open = Some(hit.path.clone());
                }
                ui.small(&hit.excerpt);
                ui.separator();
            }
        });
        open
    }

    fn run_project_search(&mut self) {
        self.search_hits.clear();
        let query = self.global_search.trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let Some(project) = &self.project else { return };
        for entry in project.files.iter().filter(|entry| !entry.is_dir) {
            let text = self
                .documents
                .iter()
                .find(|document| document.path == entry.path)
                .map(|document| document.content.clone())
                .or_else(|| fs::read_to_string(&entry.path).ok())
                .unwrap_or_default();
            for (line_index, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&query) {
                    self.search_hits.push(SearchHit {
                        path: entry.path.clone(),
                        line: line_index + 1,
                        excerpt: line.trim().chars().take(180).collect(),
                    });
                }
            }
        }
    }

    fn sidebar_codex(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Codex");
            if ui.button("＋ Entry").clicked()
                && let Some(project) = &mut self.project
            {
                project.manifest.codex.push(CodexEntry::default());
                self.selected_codex = Some(project.manifest.codex.len() - 1);
                let _ = project.save_manifest();
            }
        });
        let entries = self
            .project
            .as_ref()
            .map(|project| {
                project
                    .manifest
                    .codex
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| (index, entry.name.clone(), entry.category.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("codex-entry")
            .selected_text(
                self.selected_codex
                    .and_then(|index| entries.iter().find(|entry| entry.0 == index))
                    .map(|entry| entry.1.as_str())
                    .unwrap_or("Select an entry"),
            )
            .show_ui(ui, |ui| {
                for (index, name, category) in entries {
                    ui.selectable_value(
                        &mut self.selected_codex,
                        Some(index),
                        format!("{name} · {category}"),
                    );
                }
            });
        let Some(index) = self.selected_codex else {
            ui.small("Codex entries become automatic context when their names or aliases appear in a scene.");
            return;
        };
        let mut save = false;
        let mut delete = false;
        let mut selected_entry = None;
        if let Some(project) = &mut self.project
            && let Some(entry) = project.manifest.codex.get_mut(index)
        {
            ui.separator();
            ui.label("Name");
            save |= ui.text_edit_singleline(&mut entry.name).changed();
            ui.label("Category");
            save |= ui.text_edit_singleline(&mut entry.category).changed();
            ui.label("Aliases (comma-separated)");
            let mut aliases = entry.aliases.join(", ");
            if ui.text_edit_singleline(&mut aliases).changed() {
                entry.aliases = split_csv(&aliases);
                save = true;
            }
            ui.label("Description / story facts");
            save |= ui
                .add(egui::TextEdit::multiline(&mut entry.description).desired_rows(6))
                .changed();
            ui.label("Relationships");
            save |= ui
                .add(egui::TextEdit::multiline(&mut entry.relationships).desired_rows(3))
                .changed();
            ui.label("Current progression");
            save |= ui
                .add(egui::TextEdit::multiline(&mut entry.progression).desired_rows(3))
                .changed();
            save |= ui
                .checkbox(&mut entry.ai_include, "Include in AI context")
                .changed();
            delete = ui.button("Delete entry").clicked();
            selected_entry = Some(entry.clone());
        }
        if let Some(project) = &mut self.project {
            if delete && index < project.manifest.codex.len() {
                project.manifest.codex.remove(index);
                self.selected_codex = None;
                save = true;
            }
            if save {
                let _ = project.save_manifest();
            }
        }
        if let Some(entry) = selected_entry {
            let mut terms = entry.aliases.clone();
            terms.push(entry.name.clone());
            let terms = terms
                .into_iter()
                .map(|term| term.to_lowercase())
                .collect::<Vec<_>>();
            let mentions = self
                .project
                .as_ref()
                .map(|project| {
                    project
                        .files
                        .iter()
                        .filter(|file| !file.is_dir)
                        .filter_map(|file| {
                            let text = self
                                .documents
                                .iter()
                                .find(|document| document.path == file.path)
                                .map(|document| document.content.clone())
                                .or_else(|| fs::read_to_string(&file.path).ok())?;
                            let lowercase = text.to_lowercase();
                            let count = terms
                                .iter()
                                .map(|term| lowercase.matches(term).count())
                                .sum::<usize>();
                            (count > 0).then(|| (file.path.clone(), count))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            ui.separator();
            ui.label(RichText::new(format!("Mentions · {} documents", mentions.len())).strong());
            let mut open_mention = None;
            for (path, count) in mentions {
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("?");
                if ui.link(format!("{name} · {count}")).clicked() {
                    open_mention = Some(path);
                }
            }
            if let Some(path) = open_mention {
                self.open_document(path);
            }
        }
    }

    fn sidebar_review(&mut self, ui: &mut egui::Ui) {
        ui.heading("Story Review");
        if ui.button("Check active document spelling").clicked() {
            self.check_spelling();
        }
        if !self.spelling_issues.is_empty() {
            ui.small(format!(
                "Possible misspellings: {}",
                self.spelling_issues
                    .iter()
                    .take(30)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let documents = self.manuscript_documents();
        let text = documents.join("\n");
        let words = text.split_whitespace().collect::<Vec<_>>();
        let sentences = text.matches(['.', '!', '?']).count().max(1);
        let dialogue_words = text
            .lines()
            .filter(|line| line.contains('"') || line.contains('“'))
            .flat_map(str::split_whitespace)
            .count();
        let dialogue_percent = if words.is_empty() {
            0.0
        } else {
            dialogue_words as f32 / words.len() as f32 * 100.0
        };
        ui.label(format!("Documents: {}", documents.len()));
        ui.label(format!("Words: {}", words.len()));
        ui.label(format!(
            "Average sentence: {:.1} words",
            words.len() as f32 / sentences as f32
        ));
        ui.label(format!("Dialogue estimate: {dialogue_percent:.0}%"));
        ui.label(format!("TODO markers: {}", text.matches("TODO").count()));
        let mut frequency = std::collections::BTreeMap::<String, usize>::new();
        let stop = [
            "the", "and", "that", "with", "this", "from", "were", "have", "into", "their", "they",
            "then", "when", "what", "there", "would", "could", "about",
        ];
        for word in &words {
            let normalized = word
                .trim_matches(|character: char| !character.is_alphanumeric())
                .to_lowercase();
            if normalized.len() >= 4 && !stop.contains(&normalized.as_str()) {
                *frequency.entry(normalized).or_default() += 1;
            }
        }
        let mut frequent = frequency.into_iter().collect::<Vec<_>>();
        frequent.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        ui.label(RichText::new("Frequent words").strong());
        ui.small(
            frequent
                .into_iter()
                .take(10)
                .map(|(word, count)| format!("{word} ({count})"))
                .collect::<Vec<_>>()
                .join(" · "),
        );
        ui.separator();
        ui.label(RichText::new("POV distribution").strong());
        if let Some(project) = &self.project {
            let mut povs = std::collections::BTreeMap::<String, usize>::new();
            for meta in &project.manifest.documents {
                if !meta.pov.is_empty() && meta.active && !meta.archived {
                    *povs.entry(meta.pov.clone()).or_default() += 1;
                }
            }
            for (pov, count) in povs {
                ui.label(format!("{pov}: {count} scenes"));
            }
            ui.separator();
            ui.label(RichText::new("Metadata gaps").strong());
            for meta in project.manifest.documents.iter().filter(|meta| {
                meta.active
                    && !meta.archived
                    && matches!(meta.kind, DocumentKind::Scene)
                    && (meta.pov.is_empty() || meta.synopsis.is_empty())
            }) {
                let gaps = match (meta.pov.is_empty(), meta.synopsis.is_empty()) {
                    (true, true) => "missing POV and synopsis",
                    (true, false) => "missing POV",
                    (false, true) => "missing synopsis",
                    _ => "",
                };
                ui.small(format!("{} — {gaps}", meta.path));
            }
            ui.separator();
            ui.label(RichText::new("Unresolved work").strong());
            for meta in project
                .manifest
                .documents
                .iter()
                .filter(|meta| meta.active && meta.status != "Final")
            {
                ui.small(format!("{} — {}", meta.path, meta.status));
            }
        }
    }

    fn check_spelling(&mut self) {
        self.spelling_issues.clear();
        let Some(document) = self.active.and_then(|index| self.documents.get(index)) else {
            self.status = "Open a document before checking spelling".into();
            return;
        };
        let child = Command::new("aspell")
            .args(["list", "--lang=en_US", "--encoding=utf-8"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            self.status =
                "Spell checker unavailable; install aspell and an English dictionary".into();
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(document.content.as_bytes());
        }
        match child.wait_with_output() {
            Ok(output) => {
                let mut words = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(str::to_lowercase)
                    .collect::<Vec<_>>();
                words.sort();
                words.dedup();
                self.spelling_issues = words;
                self.status = format!(
                    "Spell check found {} unique possible issues",
                    self.spelling_issues.len()
                );
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let mut activate = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, doc) in self.documents.iter().enumerate() {
                        let label =
                            format!("{}{}", doc.title(), if doc.is_dirty() { " •" } else { "" });
                        let response = ui.selectable_label(self.active == Some(index), label);
                        if response.clicked() {
                            activate = Some(index);
                        }
                        if response.middle_clicked() {
                            close = Some(index);
                        }
                        if self.active == Some(index)
                            && ui
                                .small_button("×")
                                .on_hover_text("Close document")
                                .clicked()
                        {
                            close = Some(index);
                        }
                    }
                });
            });
        if let Some(index) = activate {
            self.active = Some(index);
            self.cursor_char = None;
            self.selection_chars = None;
        }
        if let Some(index) = close {
            self.close_document(index);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.active else {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("A quiet place for your next story");
                    ui.label(
                        "Open a chapter from the manuscript sidebar or create a new document.",
                    );
                });
            });
            return;
        };
        let mut close_comparison = false;
        if self.center_view != CenterView::Preview {
            let mut insert = None;
            let mut split = false;
            let mut trash = false;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.documents[index].title())
                        .strong()
                        .size(17.0),
                );
                if self.documents[index].is_dirty() {
                    ui.label(
                        RichText::new("UNSAVED")
                            .small()
                            .color(Color32::from_rgb(224, 181, 91)),
                    );
                } else {
                    ui.label(
                        RichText::new("SAVED")
                            .small()
                            .color(Color32::from_rgb(120, 177, 143)),
                    );
                }
                ui.separator();
                if ui
                    .small_button("H1")
                    .on_hover_text("Chapter heading")
                    .clicked()
                {
                    insert = Some("# ");
                }
                if ui
                    .small_button("H2")
                    .on_hover_text("Scene heading")
                    .clicked()
                {
                    insert = Some("## ");
                }
                if ui.small_button("—").on_hover_text("Scene break").clicked() {
                    insert = Some("\n\n* * *\n\n");
                }
                if ui
                    .small_button("TODO")
                    .on_hover_text("Revision marker")
                    .clicked()
                {
                    insert = Some("<!-- TODO:  -->");
                }
                ui.separator();
                if ui
                    .button("Find")
                    .on_hover_text("Find and replace · Ctrl+F")
                    .clicked()
                {
                    self.show_find_replace = true;
                }
                ui.menu_button("Document", |ui| {
                    if ui.button("Scene details…").clicked() {
                        self.show_document_meta = true;
                        ui.close_menu();
                    }
                    if ui.button("Rename…").clicked() {
                        self.rename_name = self.documents[index]
                            .path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("Untitled.md")
                            .to_owned();
                        self.show_rename = true;
                        ui.close_menu();
                    }
                    if ui.button("Revision history…").clicked() {
                        self.show_revisions = true;
                        ui.close_menu();
                    }
                    if ui.button("Split at scene headings…").clicked() {
                        split = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button(
                            RichText::new("Move to Project Trash…")
                                .color(Color32::from_rgb(231, 137, 126)),
                        )
                        .clicked()
                    {
                        trash = true;
                        ui.close_menu();
                    }
                });
                if self.center_view == CenterView::Split && self.pinned_document.is_some() {
                    ui.separator();
                    if ui.button("Close comparison ×").clicked() {
                        close_comparison = true;
                    }
                }
            });
            if close_comparison {
                self.pinned_document = None;
                self.center_view = CenterView::Editor;
                self.status = "Closed side-by-side document comparison".into();
            }
            if let Some(text) = insert {
                self.insert_editor_text(text);
            }
            if split {
                self.split_active_document();
                return;
            }
            if trash {
                self.pending_trash = Some(self.documents[index].path.clone());
                return;
            }
        }
        let font_size = self.settings.font_size;
        match self.center_view {
            CenterView::Editor => self.editor_pane(ui, index, font_size, "main-editor", true),
            CenterView::Preview => {
                let content = self.documents[index].content.clone();
                markdown_preview(ui, &content);
            }
            CenterView::Split => {
                let pinned_index = self.pinned_document.as_ref().and_then(|path| {
                    self.documents
                        .iter()
                        .position(|document| document.path == path.as_path())
                });
                if let Some(pinned_index) = pinned_index.filter(|pinned| *pinned != index) {
                    ui.columns(2, |columns| {
                        columns[0].label(
                            RichText::new(format!("Current: {}", self.documents[index].title()))
                                .strong(),
                        );
                        self.editor_pane(
                            &mut columns[0],
                            index,
                            font_size,
                            "left-copy-editor",
                            true,
                        );
                        columns[1].label(
                            RichText::new(format!(
                                "Compared copy: {}",
                                self.documents[pinned_index].title()
                            ))
                            .strong(),
                        );
                        self.editor_pane(
                            &mut columns[1],
                            pinned_index,
                            font_size,
                            "right-copy-editor",
                            false,
                        );
                    });
                } else {
                    let content = self.documents[index].content.clone();
                    ui.columns(2, |columns| {
                        self.editor_pane(&mut columns[0], index, font_size, "left-editor", true);
                        markdown_preview(&mut columns[1], &content);
                    });
                }
            }
        }
    }

    fn insert_editor_text(&mut self, text: &str) {
        let Some(index) = self.active else { return };
        let cursor = self
            .cursor_char
            .unwrap_or_else(|| self.documents[index].content.chars().count());
        let byte = char_to_byte_index(&self.documents[index].content, cursor);
        self.documents[index].content.insert_str(byte, text);
        self.cursor_char = Some(cursor + text.chars().count());
        self.last_edit = Instant::now();
    }

    fn trash_active_document(&mut self) {
        let Some(index) = self.active else { return };
        self.save_active();
        let path = self.documents[index].path.clone();
        if let Some(project) = &mut self.project {
            match project.trash_document(&path) {
                Ok(()) => {
                    self.documents.remove(index);
                    self.active = (!self.documents.is_empty())
                        .then_some(index.min(self.documents.len().saturating_sub(1)));
                    self.status = "Moved document to recoverable project trash".into();
                }
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    fn trash_document(&mut self, path: &PathBuf) {
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| &document.path == path)
        {
            self.active = Some(index);
            self.trash_active_document();
            return;
        }
        if let Some(project) = &mut self.project {
            match project.trash_document(path) {
                Ok(()) => self.status = "Moved document to recoverable Project Trash".into(),
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    fn restore_recovery_item(&mut self, item: &RecoveryItem) {
        let Some(project) = &self.project else {
            return;
        };
        match project.restore_recovery(item) {
            Ok(path) => {
                if let Some(index) = self
                    .documents
                    .iter()
                    .position(|document| document.path == path)
                {
                    match Document::open(path) {
                        Ok(document) => {
                            self.documents[index] = document;
                            self.active = Some(index);
                        }
                        Err(error) => {
                            self.status = error.to_string();
                            return;
                        }
                    }
                } else {
                    self.open_document(path);
                }
                self.status = "Recovered unsaved writing from the previous session".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn discard_recovery_item(&mut self, item: &RecoveryItem) {
        let Some(project) = &self.project else {
            return;
        };
        match project.discard_recovery(item) {
            Ok(()) => self.status = "Discarded recovery snapshot".into(),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn split_active_document(&mut self) {
        let Some(index) = self.active else { return };
        self.save_active();
        let path = self.documents[index].path.clone();
        if let Some(project) = &mut self.project {
            match project.split_document_at_headings(&path) {
                Ok(created) => {
                    self.documents.remove(index);
                    self.active = None;
                    if let Some(first) = created.first() {
                        self.open_document(first.clone());
                    }
                    self.status = format!(
                        "Split document into {} scenes; original moved to Trash",
                        created.len()
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    fn editor_pane(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        font_size: f32,
        scroll_id: &str,
        track_cursor: bool,
    ) {
        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .show(ui, |ui| {
                let width = (ui.available_width() - 48.0).clamp(300.0, 820.0);
                ui.horizontal(|ui| {
                    let margin = ((ui.available_width() - width) / 2.0).max(0.0);
                    ui.add_space(margin);
                    let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                        let mut job = markdown_layout_job(text, font_size);
                        job.wrap.max_width = wrap_width;
                        ui.fonts(|fonts| fonts.layout_job(job))
                    };
                    let output = egui::Frame::new()
                        .fill(Color32::from_rgb(31, 32, 35))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(50, 52, 57)))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(26, 28))
                        .show(ui, |ui| {
                            egui::TextEdit::multiline(&mut self.documents[index].content)
                                .font(FontId::new(font_size, egui::FontFamily::Proportional))
                                .desired_width(width - 52.0)
                                .desired_rows(35)
                                .lock_focus(true)
                                .frame(false)
                                .layouter(&mut layouter)
                                .show(ui)
                        })
                        .inner;
                    if output.response.changed() {
                        self.last_edit = Instant::now();
                    }
                    if track_cursor && let Some(cursor_range) = output.cursor_range {
                        self.cursor_char = Some(cursor_range.primary.ccursor.index);
                        let primary = cursor_range.primary.ccursor.index;
                        let secondary = cursor_range.secondary.ccursor.index;
                        self.selection_chars =
                            (primary != secondary).then_some((primary, secondary));
                    }
                });
            });
    }

    fn ai_sidebar(&mut self, ctx: &egui::Context) {
        if !self.ai_panel || self.focus_mode {
            return;
        }
        let prompt_presets = self
            .project
            .as_ref()
            .map(|project| project.manifest.prompt_presets.clone())
            .unwrap_or_default();
        egui::SidePanel::right("assistant").default_width(350.0).min_width(280.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Ollama Assistant");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Close assistant").clicked() {
                        self.ai_panel = false;
                    }
                });
            });
            ui.label(
                RichText::new("Local writing help · active document only")
                    .small()
                    .color(Color32::from_rgb(145, 150, 160)),
            );
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("model").selected_text(if self.settings.model.is_empty() { "No model" } else { &self.settings.model }).show_ui(ui, |ui| {
                    for model in &self.models { ui.selectable_value(&mut self.settings.model, model.clone(), model); }
                });
                if ui.small_button("↻").on_hover_text("Reconnect and refresh models").clicked() {
                    self.ollama.send(OllamaRequest::ListModels { base_url: self.settings.ollama_url.clone() });
                    self.status = "Connecting to Ollama…".into();
                }
            });
            ui.separator();
            ui.label(RichText::new("ACTION").small().strong());
            egui::ComboBox::from_id_salt("ai-action")
                .selected_text(self.ai_action.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for action in [
                        AiAction::Continue,
                        AiAction::Rewrite,
                        AiAction::Brainstorm,
                        AiAction::Summarize,
                        AiAction::Critique,
                        AiAction::SceneBeats,
                        AiAction::Extract,
                        AiAction::Chat,
                        AiAction::Custom,
                    ] {
                        ui.selectable_value(&mut self.ai_action, action, action.label());
                    }
            });
            if self.ai_action == AiAction::Continue {
                ui.horizontal(|ui| {
                    ui.label("Length:");
                    ui.selectable_value(
                        &mut self.continue_length,
                        ContinueLength::Sentence,
                        "Sentence",
                    );
                    ui.selectable_value(
                        &mut self.continue_length,
                        ContinueLength::Paragraph,
                        "Paragraph",
                    );
                });
                ui.small("Generation begins at the current editor cursor.");
            }
            if self.ai_action == AiAction::Chat {
                let messages = self
                    .project
                    .as_ref()
                    .map(|project| project.manifest.chat_messages.clone())
                    .unwrap_or_default();
                egui::ScrollArea::vertical()
                    .id_salt("story-chat")
                    .max_height(220.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for message in messages.iter().rev().take(12).rev() {
                            ui.label(RichText::new(&message.role).strong());
                            ui.label(&message.content);
                            ui.add_space(5.0);
                        }
                    });
                ui.add(
                    egui::TextEdit::multiline(&mut self.ai_prompt)
                        .hint_text("Ask about this scene, character, plot, or continuity…")
                        .desired_rows(3),
                );
            } else if self.ai_action == AiAction::Custom {
                egui::ComboBox::from_id_salt("prompt-presets")
                    .selected_text(
                        self.selected_prompt_preset
                            .and_then(|index| prompt_presets.get(index))
                            .map(|preset| preset.name.as_str())
                            .unwrap_or("Reusable prompt presets"),
                    )
                    .show_ui(ui, |ui| {
                        for (index, preset) in prompt_presets.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.selected_prompt_preset == Some(index),
                                    &preset.name,
                                )
                                .clicked()
                            {
                                self.selected_prompt_preset = Some(index);
                                self.prompt_preset_name.clone_from(&preset.name);
                                self.ai_prompt.clone_from(&preset.instructions);
                            }
                        }
                    });
                ui.add(egui::TextEdit::multiline(&mut self.ai_prompt).hint_text("Describe the change, scene, tone, or problem…").desired_rows(4));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.prompt_preset_name)
                            .hint_text("Preset name"),
                    );
                    if ui.button("Save preset").clicked() {
                        self.save_prompt_preset();
                    }
                    if ui
                        .add_enabled(
                            self.selected_prompt_preset.is_some(),
                            egui::Button::new("Delete"),
                        )
                        .clicked()
                    {
                        self.delete_prompt_preset();
                    }
                });
            }
            let mut context_changed = false;
            ui.collapsing("Context controls", |ui| {
                context_changed |= ui
                    .checkbox(&mut self.settings.include_style_guide, "Style guide")
                    .changed();
                context_changed |= ui
                    .checkbox(&mut self.settings.include_scene_metadata, "Scene metadata")
                    .changed();
                context_changed |= ui
                    .checkbox(
                        &mut self.settings.include_previous_summaries,
                        "Earlier scene summaries",
                    )
                    .changed();
                context_changed |= ui
                    .checkbox(
                        &mut self.settings.include_relevant_scenes,
                        "Relevant manuscript passages",
                    )
                    .changed();
                context_changed |= ui
                    .checkbox(&mut self.settings.include_codex, "Relevant Codex entries")
                    .changed();
                context_changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.settings.previous_summary_limit)
                            .range(0..=50)
                            .prefix("Summary limit: "),
                    )
                    .changed();
                context_changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.settings.codex_limit)
                            .range(0..=100)
                            .prefix("Codex limit: "),
                    )
                    .changed();
                context_changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.settings.relevant_scene_limit)
                            .range(0..=10)
                            .prefix("Relevant passages: "),
                    )
                    .changed();
                if !self.prompt_preview.is_empty() {
                    let estimated_tokens = self.prompt_preview.chars().count().div_ceil(4);
                    ui.small(format!(
                        "Last assembled prompt: {} characters · approximately {estimated_tokens} tokens",
                        self.prompt_preview.chars().count()
                    ));
                }
            });
            if context_changed {
                let _ = self.settings.save();
            }
            let ready = self.active.is_some() && !self.ai_busy && !self.settings.model.is_empty()
                && (!matches!(self.ai_action, AiAction::Custom | AiAction::Chat)
                    || !self.ai_prompt.trim().is_empty());
            ui.horizontal(|ui| {
                let generate = egui::Button::new(
                    RichText::new(if self.ai_busy { "Writing…" } else { "Generate" }).strong(),
                )
                .fill(Color32::from_rgb(102, 82, 45))
                .min_size([112.0, 32.0].into());
                if ui.add_enabled(ready, generate).clicked() { self.request_ai(); }
                if ui.add_enabled(self.ai_busy, egui::Button::new("Stop")).clicked() {
                    self.ollama.cancel();
                    self.status = "Stopping generation…".into();
                }
                if ui
                    .add_enabled(
                        !self.ai_busy && self.last_generation_request.is_some(),
                        egui::Button::new("Alternative"),
                    )
                    .on_hover_text("Regenerate from the exact same prompt and context")
                    .clicked()
                {
                    self.regenerate_ai();
                }
                if ui
                    .add_enabled(
                        !self.ai_busy
                            && self.last_generation_request.is_some()
                            && self.ai_generation_action != AiAction::Chat,
                        egui::Button::new("3 options"),
                    )
                    .on_hover_text("Generate three alternatives from the exact same context")
                    .clicked()
                {
                    self.generate_alternative_batch(3);
                }
            });
            if ui
                .small_button("Inspect exact prompt and context")
                .on_hover_text("Review everything that will be sent to Ollama")
                .clicked()
            {
                self.show_prompt_preview = true;
            }
            if self.ai_busy { ui.add(egui::Spinner::new()); }
            ui.separator();
            ui.label(RichText::new("Suggestion").strong());
            ui.add(egui::TextEdit::multiline(&mut self.ai_output).hint_text("Generated text will appear here. You can edit it before inserting.").desired_rows(18));
            ui.horizontal_wrapped(|ui| {
                let has_output = !self.ai_output.trim().is_empty() && self.active.is_some();
                let manuscript_output = matches!(
                    self.ai_generation_action,
                    AiAction::Continue | AiAction::Rewrite | AiAction::Custom
                );
                if manuscript_output {
                    if ui
                        .add_enabled(has_output, egui::Button::new("Insert at cursor"))
                        .clicked()
                    {
                        self.insert_ai_at_cursor();
                    }
                    let replace_label = if self.ai_replace_target.is_some() { "Replace selection" } else { "Replace document" };
                    if ui.add_enabled(has_output, egui::Button::new(replace_label)).clicked() {
                        self.replace_ai_target();
                    }
                    if ui
                        .add_enabled(has_output, egui::Button::new("Compare side by side"))
                        .clicked()
                    {
                        self.compose_ai_document_version();
                        self.show_ai_compare = true;
                    }
                }
                if self.ai_generation_action == AiAction::Extract
                    && ui
                        .add_enabled(has_output, egui::Button::new("Apply to scene & Codex"))
                        .clicked()
                {
                    self.apply_scene_extraction();
                }
                if ui
                    .add_enabled(!self.ai_undo_stack.is_empty(), egui::Button::new("Undo AI edit"))
                    .clicked()
                {
                    self.undo_last_ai_edit();
                }
                if ui.add_enabled(has_output, egui::Button::new("Copy")).clicked() { ui.ctx().copy_text(self.ai_output.clone()); }
                if ui.add_enabled(has_output, egui::Button::new("Clear")).clicked() { self.ai_output.clear(); }
            });
            if !self.generation_history.is_empty() {
                egui::ComboBox::from_id_salt("generation-history")
                    .selected_text("Generation history")
                    .show_ui(ui, |ui| {
                        for (index, item) in self.generation_history.iter().enumerate().rev() {
                            let label = format!("Result {} · {} chars", index + 1, item.chars().count());
                            if ui.button(label).clicked() { self.ai_output.clone_from(item); }
                        }
                    });
            }
            ui.add_space(8.0);
            ui.small("Your manuscript is sent only to the configured Ollama server. With the default URL, it remains on this computer.");
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let chapter_words = self
            .active
            .and_then(|i| self.documents.get(i))
            .map(Document::word_count)
            .unwrap_or(0);
        let total_words = self.project_word_count();
        let session_words = total_words.saturating_sub(self.session_start_words);
        let session_minutes = self.session_started.elapsed().as_secs() / 60;
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let status_color = if self.status.to_lowercase().contains("error")
                    || self.status.to_lowercase().contains("could not")
                    || self.status.to_lowercase().contains("failed")
                {
                    Color32::from_rgb(231, 137, 126)
                } else if self.ai_busy {
                    Color32::from_rgb(224, 181, 91)
                } else {
                    Color32::from_rgb(151, 187, 164)
                };
                ui.label(RichText::new("●").color(status_color));
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!(
                        "Novel: {total_words} / {} words",
                        self.settings.target_words
                    ));
                    ui.separator();
                    ui.label(format!("Chapter: {chapter_words} words"));
                    ui.separator();
                    ui.label(format!("Session: +{session_words} · {session_minutes}m"));
                });
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.pending_trash.clone() {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("this document")
                .to_owned();
            egui::Window::new("Move document to Project Trash?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(format!("“{name}” will leave the manuscript."));
                    ui.label("You can restore it later from Project Trash.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                RichText::new("Move to Trash")
                                    .color(Color32::from_rgb(231, 137, 126)),
                            )
                            .clicked()
                        {
                            self.trash_document(&path);
                            self.pending_trash = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_trash = None;
                        }
                    });
                });
        }
        if self.show_new_document {
            egui::Window::new("New document")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Path inside project (folders are allowed)");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.new_item_name)
                            .hint_text("Chapters/Chapter 01.md")
                            .desired_width(340.0),
                    );
                    response.request_focus();
                    let enter =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() || enter {
                            self.create_document();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_document = false;
                            self.new_item_name.clear();
                        }
                    });
                });
        }
        if self.show_new_folder {
            egui::Window::new("New folder")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Folder path");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_item_name)
                            .hint_text("Characters")
                            .desired_width(300.0),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            self.create_folder();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_folder = false;
                            self.new_item_name.clear();
                        }
                    });
                });
        }
        if self.show_settings {
            let mut open = self.show_settings;
            egui::Window::new("Settings")
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Ollama server URL");
                    ui.text_edit_singleline(&mut self.settings.ollama_url);
                    ui.add(
                        egui::Slider::new(&mut self.settings.temperature, 0.0..=1.5)
                            .text("Creativity"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.settings.top_p, 0.05..=1.0)
                            .text("Top-p diversity"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.settings.repeat_penalty, 0.8..=2.0)
                            .text("Repeat penalty"),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.context_length)
                            .range(2_048..=262_144)
                            .prefix("Context: ")
                            .suffix(" tokens"),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.max_output_tokens)
                            .range(64..=32_768)
                            .prefix("Maximum output: ")
                            .suffix(" tokens"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.settings.font_size, 13.0..=28.0)
                            .text("Editor font"),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.target_words)
                            .range(1_000..=1_000_000)
                            .prefix("Novel target: ")
                            .suffix(" words"),
                    );
                    ui.checkbox(&mut self.settings.autosave, "Autosave after a short pause");
                    ui.checkbox(
                        &mut self.settings.check_updates,
                        "Check GitHub for updates when the app starts",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Save and reconnect").clicked() {
                            let _ = self.settings.save();
                            self.ollama.send(OllamaRequest::ListModels {
                                base_url: self.settings.ollama_url.clone(),
                            });
                            self.status = "Settings saved; connecting…".into();
                        }
                    });
                });
            self.show_settings = open;
        }
        if self.show_project_settings {
            let mut open = self.show_project_settings;
            let mut changed = false;
            egui::Window::new("Project settings")
                .open(&mut open)
                .default_width(480.0)
                .show(ctx, |ui| {
                    if let Some(project) = &mut self.project {
                        ui.label("Project name");
                        changed |= ui.text_edit_singleline(&mut project.manifest.name).changed();
                        ui.label("Novel title");
                        changed |= ui.text_edit_singleline(&mut project.manifest.title).changed();
                        ui.label("Author / pen name");
                        changed |= ui.text_edit_singleline(&mut project.manifest.author).changed();
                        ui.label("Language");
                        changed |= ui.text_edit_singleline(&mut project.manifest.language).changed();
                        ui.label("Style guide sent to AI");
                        changed |= ui
                            .add(
                                egui::TextEdit::multiline(&mut project.manifest.style_guide)
                                    .hint_text("Voice, tense, POV, tone, forbidden clichés, formatting rules…")
                                    .desired_rows(8),
                            )
                            .changed();
                        ui.separator();
                        ui.heading("Manuscript build profile");
                        if let Some(profile) = project.manifest.build_profiles.first_mut() {
                            changed |= ui.text_edit_singleline(&mut profile.name).changed();
                            changed |= ui
                                .checkbox(&mut profile.include_inactive, "Include inactive documents")
                                .changed();
                            changed |= ui
                                .checkbox(&mut profile.include_notes, "Include notes and research")
                                .changed();
                            changed |= ui
                                .checkbox(&mut profile.page_break_documents, "Start each document on a new page")
                                .changed();
                            changed |= ui
                                .checkbox(&mut profile.include_comments, "Include TODO comments")
                                .changed();
                        }
                    }
                });
            if changed && let Some(project) = &self.project {
                let _ = project.save_manifest();
            }
            self.show_project_settings = open;
        }
        if self.show_document_meta {
            let mut open = self.show_document_meta;
            let active_path = self
                .active
                .and_then(|index| self.documents.get(index))
                .map(|document| document.path.clone());
            let mut changed = false;
            egui::Window::new("Scene metadata")
                .open(&mut open)
                .default_width(520.0)
                .show(ctx, |ui| {
                    if let (Some(project), Some(path)) = (&mut self.project, &active_path)
                        && let Some(meta) = project.document_meta_mut(path)
                    {
                        ui.horizontal(|ui| {
                            ui.label("Type");
                            egui::ComboBox::from_id_salt("document-kind")
                                .selected_text(meta.kind.label())
                                .show_ui(ui, |ui| {
                                    for kind in DocumentKind::ALL {
                                        changed |= ui
                                            .selectable_value(&mut meta.kind, kind, kind.label())
                                            .changed();
                                    }
                                });
                            changed |= ui
                                .checkbox(&mut meta.active, "Active in manuscript")
                                .changed();
                            changed |= ui.checkbox(&mut meta.ai_include, "AI context").changed();
                        });
                        ui.label("Status");
                        changed |= ui.text_edit_singleline(&mut meta.status).changed();
                        ui.columns(2, |columns| {
                            columns[0].label("POV character");
                            changed |= columns[0].text_edit_singleline(&mut meta.pov).changed();
                            columns[1].label("Location");
                            changed |= columns[1]
                                .text_edit_singleline(&mut meta.location)
                                .changed();
                        });
                        ui.label("Story date / time");
                        changed |= ui.text_edit_singleline(&mut meta.story_time).changed();
                        ui.label("Synopsis");
                        changed |= ui
                            .add(egui::TextEdit::multiline(&mut meta.synopsis).desired_rows(4))
                            .changed();
                        ui.label("Scene beats");
                        changed |= ui
                            .add(egui::TextEdit::multiline(&mut meta.beats).desired_rows(5))
                            .changed();
                        ui.label("Characters (comma-separated)");
                        let mut characters = meta.characters.join(", ");
                        if ui.text_edit_singleline(&mut characters).changed() {
                            meta.characters = split_csv(&characters);
                            changed = true;
                        }
                        ui.label("Plot threads (comma-separated)");
                        let mut plots = meta.plot_threads.join(", ");
                        if ui.text_edit_singleline(&mut plots).changed() {
                            meta.plot_threads = split_csv(&plots);
                            changed = true;
                        }
                        ui.label("Tags (comma-separated)");
                        let mut tags = meta.tags.join(", ");
                        if ui.text_edit_singleline(&mut tags).changed() {
                            meta.tags = split_csv(&tags);
                            changed = true;
                        }
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut meta.word_target).prefix("Word target: "),
                            )
                            .changed();
                        if ui
                            .checkbox(&mut meta.archived, "Archive this document")
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
            if changed && let Some(project) = &mut self.project {
                let _ = project.save_manifest();
                let _ = project.refresh();
            }
            self.show_document_meta = open;
        }
        if self.show_find_replace {
            let mut open = self.show_find_replace;
            egui::Window::new("Find and replace")
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Find");
                    ui.text_edit_singleline(&mut self.find_text);
                    ui.label("Replace with");
                    ui.text_edit_singleline(&mut self.replace_text);
                    if ui
                        .add_enabled(
                            !self.find_text.is_empty() && self.active.is_some(),
                            egui::Button::new("Replace all in document"),
                        )
                        .clicked()
                        && let Some(document) =
                            self.active.and_then(|index| self.documents.get_mut(index))
                    {
                        let count = document.content.matches(&self.find_text).count();
                        document.content = document
                            .content
                            .replace(&self.find_text, &self.replace_text);
                        self.last_edit = Instant::now();
                        self.status = format!("Replaced {count} occurrences");
                    }
                });
            self.show_find_replace = open;
        }
        if self.show_rename {
            let mut open = self.show_rename;
            let mut rename = false;
            egui::Window::new("Rename document")
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("New file name");
                    let response = ui.text_edit_singleline(&mut self.rename_name);
                    rename = ui.button("Rename").clicked()
                        || (response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                });
            if rename && let Some(index) = self.active {
                self.save_active();
                let source = self.documents[index].path.clone();
                if let Some(project) = &mut self.project {
                    match project.rename_document(&source, &self.rename_name) {
                        Ok(target) => {
                            self.documents[index].path = target;
                            self.status = "Document renamed".into();
                            open = false;
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            self.show_rename = open;
        }
        if self.show_recovery {
            let mut open = self.show_recovery;
            let items = self
                .project
                .as_ref()
                .and_then(|project| project.recovery_items().ok())
                .unwrap_or_default();
            let mut restore = None;
            let mut discard = None;
            egui::Window::new("Recover unsaved writing")
                .open(&mut open)
                .default_width(560.0)
                .show(ctx, |ui| {
                    ui.label(
                        "These snapshots were captured before the documents were safely saved.",
                    );
                    if items.is_empty() {
                        ui.label("No recovery snapshots remain.");
                    }
                    for (index, item) in items.iter().enumerate() {
                        ui.group(|ui| {
                            ui.strong(&item.relative_path);
                            ui.small(format!(
                                "{} words captured",
                                item.content.split_whitespace().count()
                            ));
                            ui.horizontal(|ui| {
                                if ui.button("Restore").clicked() {
                                    restore = Some(index);
                                }
                                if ui.button("Discard").clicked() {
                                    discard = Some(index);
                                }
                            });
                        });
                    }
                });
            if let Some(index) = restore {
                self.restore_recovery_item(&items[index]);
            } else if let Some(index) = discard {
                self.discard_recovery_item(&items[index]);
            }
            self.show_recovery = open
                && self
                    .project
                    .as_ref()
                    .and_then(|project| project.recovery_items().ok())
                    .is_some_and(|items| !items.is_empty());
        }
        if self.show_trash {
            let mut open = self.show_trash;
            let items = self
                .project
                .as_ref()
                .map(|project| project.manifest.trash.clone())
                .unwrap_or_default();
            let mut restore = None;
            egui::Window::new("Project Trash")
                .open(&mut open)
                .default_width(460.0)
                .show(ctx, |ui| {
                    ui.label("Deleted and split source documents remain recoverable here.");
                    if items.is_empty() {
                        ui.label("Trash is empty.");
                    }
                    for (index, item) in items.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(&item.original_path);
                            if ui.button("Restore").clicked() {
                                restore = Some(index);
                            }
                        });
                    }
                });
            if let Some(index) = restore
                && let Some(project) = &mut self.project
            {
                match project.restore_trash(index) {
                    Ok(path) => {
                        self.open_document(path);
                        self.status = "Document restored from Trash".into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            self.show_trash = open;
        }
        if self.show_revisions {
            let mut open = self.show_revisions;
            let active_path = self
                .active
                .and_then(|index| self.documents.get(index))
                .map(|document| document.path.clone());
            let revisions = active_path
                .as_ref()
                .and_then(|path| self.project.as_ref().map(|project| project.revisions(path)))
                .unwrap_or_default();
            let mut restore = None;
            egui::Window::new("Revision history")
                .open(&mut open)
                .default_width(480.0)
                .show(ctx, |ui| {
                    ui.label("A snapshot is created before each changed document is saved.");
                    if revisions.is_empty() {
                        ui.label("No earlier revisions yet.");
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for revision in &revisions {
                            let label = revision
                                .file_stem()
                                .and_then(|value| value.to_str())
                                .unwrap_or("snapshot");
                            ui.horizontal(|ui| {
                                ui.label(label);
                                if ui.button("Restore").clicked() {
                                    restore = Some(revision.clone());
                                }
                            });
                        }
                    });
                });
            if let Some(revision) = restore
                && let Ok(content) = fs::read_to_string(revision)
                && let Some(document) = self.active.and_then(|index| self.documents.get_mut(index))
            {
                document.content = content;
                self.last_edit = Instant::now();
                self.status = "Revision restored; save to keep it".into();
                open = false;
            }
            self.show_revisions = open;
        }
        if self.show_prompt_preview {
            let mut open = self.show_prompt_preview;
            egui::Window::new("Exact AI prompt")
                .open(&mut open)
                .default_width(700.0)
                .default_height(600.0)
                .show(ctx, |ui| {
                    ui.small("This is the context and instruction sent to Ollama for the most recent generation.");
                    let characters = self.prompt_preview.chars().count();
                    ui.label(format!(
                        "Model: {} · {characters} characters · approximately {} tokens · context limit {}",
                        if self.settings.model.is_empty() {
                            "not selected"
                        } else {
                            &self.settings.model
                        },
                        characters.div_ceil(4),
                        self.settings.context_length
                    ));
                    ui.add(
                        egui::TextEdit::multiline(&mut self.prompt_preview)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(30)
                            .interactive(false),
                    );
                    if ui.button("Copy prompt").clicked() {
                        ui.ctx().copy_text(self.prompt_preview.clone());
                    }
                });
            self.show_prompt_preview = open;
        }
        if self.show_ai_compare {
            let mut open = self.show_ai_compare;
            let mut action = None;
            let mut original = self.ai_comparison_original.clone();
            egui::Window::new("Handwritten ↔ AI-assisted comparison")
                .open(&mut open)
                .default_width(1100.0)
                .default_height(700.0)
                .show(ctx, |ui| {
                    ui.columns(2, |columns| {
                        columns[0].heading("Handwritten version — protected");
                        columns[0].add(
                            egui::TextEdit::multiline(&mut original)
                                .desired_width(f32::INFINITY)
                                .desired_rows(30)
                                .interactive(false),
                        );
                        columns[1].heading("AI-assisted version — editable");
                        columns[1].add(
                            egui::TextEdit::multiline(&mut self.ai_comparison_ai)
                                .desired_width(f32::INFINITY)
                                .desired_rows(30),
                        );
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Use AI-assisted version").clicked() {
                            action = Some(1);
                        }
                        if ui.button("Keep handwritten version").clicked() {
                            action = Some(3);
                        }
                        if ui.button("Copy AI-assisted version").clicked() {
                            ui.ctx().copy_text(self.ai_comparison_ai.clone());
                        }
                    });
                });
            match action {
                Some(1) => {
                    self.accept_ai_document_version();
                    open = false;
                }
                Some(3) => open = false,
                _ => {}
            }
            self.show_ai_compare = open;
        }
    }
}

impl eframe::App for NovelQuillApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_ollama(ctx);
        self.poll_updates(ctx);
        self.handle_shortcuts(ctx);
        if self.last_recovery.elapsed() > Duration::from_millis(500) {
            if let Some(project) = &self.project {
                for document in self.documents.iter().filter(|document| document.is_dirty()) {
                    if let Err(error) = project.write_recovery(document) {
                        self.status = format!("Could not write recovery snapshot: {error}");
                        break;
                    }
                }
            }
            self.last_recovery = Instant::now();
        }
        if self.settings.autosave && self.last_edit.elapsed() > Duration::from_secs(2) {
            if self.documents.iter().any(Document::is_dirty) {
                self.save_all();
                self.status = "Autosaved all edited document copies".into();
            }
            self.last_edit = Instant::now();
        }
        self.top_bar(ctx);
        self.status_bar(ctx);
        self.left_sidebar(ctx);
        self.ai_sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            self.tabs(ui);
            ui.separator();
            ui.add_space(2.0);
            self.editor(ui);
        });
        self.dialogs(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_all();
        let _ = self.settings.save();
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(23, 24, 27);
    style.visuals.window_fill = Color32::from_rgb(30, 31, 35);
    style.visuals.extreme_bg_color = Color32::from_rgb(18, 19, 22);
    style.visuals.faint_bg_color = Color32::from_rgb(35, 36, 40);
    style.visuals.selection.bg_fill = Color32::from_rgb(105, 84, 48);
    style.visuals.selection.stroke.color = Color32::from_rgb(244, 222, 174);
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(9.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    ctx.set_style(style);
}

fn markdown_preview(ui: &mut egui::Ui, markdown: &str) {
    egui::ScrollArea::vertical()
        .id_salt("preview-scroll")
        .show(ui, |ui| {
            ui.set_max_width(850.0);
            for line in markdown.lines() {
                let trimmed = line.trim();
                if let Some(text) = trimmed.strip_prefix("# ") {
                    ui.add_space(12.0);
                    ui.heading(RichText::new(text).size(32.0));
                } else if let Some(text) = trimmed.strip_prefix("## ") {
                    ui.add_space(9.0);
                    ui.heading(RichText::new(text).size(25.0));
                } else if let Some(text) = trimmed.strip_prefix("### ") {
                    ui.add_space(7.0);
                    ui.heading(RichText::new(text).size(20.0));
                } else if let Some(text) = trimmed.strip_prefix("> ") {
                    ui.label(
                        RichText::new(text)
                            .italics()
                            .color(Color32::from_rgb(170, 180, 195)),
                    );
                } else if let Some(text) = trimmed.strip_prefix("- ") {
                    ui.label(format!("  •  {text}"));
                } else if trimmed == "---" || trimmed == "***" {
                    ui.separator();
                } else if trimmed.starts_with("```") {
                    ui.label(RichText::new(trimmed).monospace().color(Color32::GRAY));
                } else if trimmed.is_empty() {
                    ui.add_space(10.0);
                } else {
                    ui.label(RichText::new(line).text_style(TextStyle::Body).size(18.0));
                }
            }
        });
}

fn char_to_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(text.len())
}

fn ai_insertion_text(
    document: &str,
    byte_index: usize,
    generated: &str,
    length: ContinueLength,
) -> String {
    let before = &document[..byte_index];
    let after = &document[byte_index..];
    let mut insertion = String::new();
    match length {
        ContinueLength::Sentence => {
            if !before.is_empty() && !before.ends_with(char::is_whitespace) {
                insertion.push(' ');
            }
            insertion.push_str(generated);
            if !after.is_empty() && !after.starts_with(char::is_whitespace) {
                insertion.push(' ');
            }
        }
        ContinueLength::Paragraph => {
            if !before.is_empty() && !before.ends_with("\n\n") {
                insertion.push_str(if before.ends_with('\n') { "\n" } else { "\n\n" });
            }
            insertion.push_str(generated);
            if !after.is_empty() && !after.starts_with("\n\n") {
                insertion.push_str(if after.starts_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                });
            }
        }
    }
    insertion
}

fn markdown_layout_job(text: &str, font_size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let mut format = egui::TextFormat {
            font_id: FontId::new(font_size, egui::FontFamily::Proportional),
            color: Color32::from_rgb(220, 220, 215),
            ..Default::default()
        };
        if trimmed.starts_with("# ") {
            format.font_id.size = font_size + 8.0;
            format.color = Color32::from_rgb(225, 190, 125);
        } else if trimmed.starts_with("## ") {
            format.font_id.size = font_size + 4.0;
            format.color = Color32::from_rgb(205, 180, 125);
        } else if trimmed.starts_with("### ") {
            format.font_id.size = font_size + 2.0;
            format.color = Color32::from_rgb(190, 170, 125);
        } else if trimmed.starts_with('>') {
            format.italics = true;
            format.color = Color32::from_rgb(160, 180, 205);
        } else if trimmed.starts_with("<!--") {
            format.color = Color32::from_rgb(125, 145, 135);
            format.italics = true;
        } else if matches!(trimmed.trim(), "---" | "***" | "* * *") {
            format.color = Color32::from_rgb(190, 150, 100);
        }
        job.append(line, 0.0, format);
    }
    job
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn meaningful_terms(text: &str) -> HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "about", "after", "again", "also", "before", "being", "could", "every", "first", "from",
        "have", "into", "just", "more", "other", "over", "said", "some", "than", "that", "their",
        "there", "these", "they", "this", "through", "very", "what", "when", "where", "which",
        "while", "with", "would", "your",
    ];
    text.split(|character: char| !character.is_alphanumeric() && character != '\'')
        .map(str::to_lowercase)
        .filter(|term| term.chars().count() >= 4)
        .filter(|term| !STOP_WORDS.contains(&term.as_str()))
        .collect()
}

fn best_relevant_excerpt(
    text: &str,
    query_terms: &HashSet<String>,
    maximum_characters: usize,
) -> (usize, String) {
    let mut best = (0, String::new());
    for paragraph in text
        .split("\n\n")
        .filter(|paragraph| !paragraph.trim().is_empty())
    {
        let terms = meaningful_terms(paragraph);
        let score = terms.intersection(query_terms).count();
        if score > best.0 {
            let mut excerpt = paragraph
                .trim()
                .chars()
                .take(maximum_characters)
                .collect::<String>();
            if paragraph.trim().chars().count() > maximum_characters {
                excerpt.push('…');
            }
            best = (score, excerpt);
        }
    }
    best
}

fn value_or_unknown(value: &str) -> &str {
    if value.trim().is_empty() {
        "unspecified"
    } else {
        value
    }
}

fn remove_html_comments(text: &str) -> String {
    let mut output = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("<!--") {
        output.push_str(&remaining[..start]);
        let Some(end) = remaining[start + 4..].find("-->") else {
            remaining = "";
            break;
        };
        remaining = &remaining[start + 4 + end + 3..];
    }
    output.push_str(remaining);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevant_excerpt_prefers_shared_story_terms() {
        let query =
            meaningful_terms("Mara entered the obsidian observatory with the star compass.");
        let text = "A baker prepared bread for the village.\n\nMara hid the star compass beneath the obsidian observatory floor.";
        let (score, excerpt) = best_relevant_excerpt(text, &query, 500);
        assert!(score >= 4);
        assert!(excerpt.contains("star compass"));
        assert!(!excerpt.contains("baker"));
    }

    #[test]
    fn relevant_excerpt_respects_character_limit() {
        let query = meaningful_terms("observatory compass");
        let (_, excerpt) = best_relevant_excerpt(
            "The observatory contained a compass and many ancient instruments.",
            &query,
            20,
        );
        assert!(excerpt.chars().count() <= 21);
        assert!(excerpt.ends_with('…'));
    }
}
