# Novel Quill Studio

A local-first desktop novel-writing studio built in Rust. Manuscripts remain ordinary Markdown files, and optional writing assistance runs through your local [Ollama](https://ollama.com/) server.

## Features

- **Local-first projects:** ordinary UTF-8 Markdown remains the manuscript source of truth. A `.novelquill/project.json` sidecar stores stable IDs, custom ordering, scene metadata, Codex entries and build settings.
- **Organizer:** nested files and folders, starter novel structure, multiple open tabs, duplicate and reorder controls, active/inactive and archived scenes, document types and statuses.
- **Planning:** scene synopsis, POV, location, story time, characters, plot threads, beats, tags and word targets. Switch the Outline workspace among Cards, Matrix and Timeline views.
- **Codex:** characters, locations, lore or custom categories with aliases, descriptions, relationships, progressions, automatic manuscript mentions and AI inclusion controls.
- **Writing:** centered Markdown editor, formatting helpers, Write/Preview/Split modes, focus mode, autosave, find/replace and manuscript/chapter word targets.
- **Safety:** atomic file replacement and up to 50 automatic pre-save revisions per document, with an in-app restore browser.
- **Project search:** search every Markdown or text document and jump directly to each result.
- **Review:** average sentence length, dialogue estimate, TODO count, frequent words, POV distribution, metadata gaps and unfinished-scene reporting.
- **Story-aware Ollama:** streamed local generation with Sentence/Paragraph continuation at the cursor, selection replacement, rewrite, brainstorm, summary, critique, scene-beat extraction and custom instructions.
- **Transparent AI context:** relevant Codex facts, scene metadata, previous summaries, plot threads and the project style guide are assembled automatically. The exact prompt remains inspectable and copyable.
- **Generation history:** reopen the last 20 responses and review or edit every suggestion before insertion.
- **Side-by-side AI review:** preserve the exact original source beside an editable AI version, then accept it as a selection replacement, insert it at the cursor, replace the document, or keep the original. Applied AI edits have a dedicated one-click undo stack.
- **Publishing:** filter manuscript content through a saved build profile and export Markdown, PDF, DOCX, ODT, EPUB or styled HTML.
- **Preferences:** Ollama endpoint/model, creativity, font size, autosave and novel word target, with automatic last-project reopening.

## Run

Install [Rust](https://rustup.rs/) and Ollama, then install at least one model:

```bash
ollama pull llama3.2
ollama serve
```

In another terminal:

```bash
cargo run --release
```

Open a folder containing `.md`, `.markdown`, or `.txt` files. The default Ollama endpoint is `http://127.0.0.1:11434` and can be changed in Settings.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl/Cmd+O` | Open project folder |
| `Ctrl/Cmd+N` | New document |
| `Ctrl/Cmd+S` | Save active document |
| `Ctrl/Cmd+P` | Toggle editor/preview |
| Middle-click a tab | Close and save it |

## Data and privacy

Novel Quill stores the manuscript and project metadata only in the folder you choose. Revision snapshots live under `.novelquill/history`. Global settings contain the last project path, Ollama endpoint, selected model and editor preferences. With the default endpoint, AI requests never leave your computer. If you configure a remote endpoint, the context shown by **Prompt preview** is sent to that server.

## Project layout

- `src/app.rs` — desktop UI and editor workflow
- `src/model.rs` — projects, documents, persistence, and filesystem safety
- `src/ollama.rs` — non-blocking Ollama API client
- `src/export.rs` — dependency-free DOCX, ODT, EPUB and HTML builders
- `src/pdf.rs` — native paginated PDF typesetter

## Current format guarantees

The editor always reads and writes plain UTF-8 text. Preview intentionally handles the Markdown constructs most useful in prose—headings, paragraphs, block quotes, lists, scene breaks, and code markers—while leaving the source untouched.

## License

Novel Quill Studio is free software licensed under the [GNU General Public License version 3](LICENSE), identified by `GPL-3.0-only`. Modified versions distributed to others must remain under the GPL and provide their corresponding source code.
