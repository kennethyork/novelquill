# Novel Quill Studio

A local-first desktop novel-writing studio built in Rust. Manuscripts remain ordinary Markdown files, and optional writing assistance runs through your local [Ollama](https://ollama.com/) server.

## Features

- **Local-first projects:** ordinary UTF-8 Markdown remains the manuscript source of truth. A `.novelquill/project.json` sidecar stores stable IDs, custom ordering, scene metadata, Codex entries and build settings.
- **Organizer:** nested files and folders, starter novel structure, multiple open tabs, duplicate controls, true drag-and-drop ordering, active/inactive and archived scenes, document types and statuses.
- **Planning:** scene synopsis, POV, location, story time, characters, plot threads, beats, tags and word targets. Switch the Outline workspace among Cards, Matrix and Timeline views.
- **Codex:** characters, locations, lore or custom categories with aliases, descriptions, relationships, progressions, automatic manuscript mentions and AI inclusion controls.
- **Writing:** centered Markdown editor, formatting helpers, Write/Preview/Split modes, focus mode, autosave, find/replace and manuscript/chapter word targets.
- **Copy comparison:** use the `⇄` action beside any manuscript file to open that document as an editable copy beside the current document. Both sides retain independent contents and autosave safely.
- **Safety:** atomic file replacement, operating-system project locks, sub-second crash snapshots, an in-app recovery browser and up to 50 automatic pre-save revisions per document.
- **Project search:** search every Markdown or text document and jump directly to each result.
- **Review:** average sentence length, dialogue estimate, TODO count, frequent words, POV distribution, metadata gaps and unfinished-scene reporting.
- **Story-aware Ollama:** streamed local generation with Sentence/Paragraph continuation at the cursor, selection replacement, rewrite, brainstorm, summary, critique, scene-beat extraction, structured project extraction and custom instructions.
- **Transparent AI context:** independently select the style guide, scene metadata, previous summaries, relevant manuscript passages and matching Codex facts. The exact assembled prompt and estimated token use remain inspectable and copyable.
- **Advanced local generation:** configure temperature, top-p, repetition penalty, context length and maximum output length. Generate one retry or a three-option batch from the exact same context.
- **Reusable prompt library:** save project-level custom instructions as named presets and reuse them without rebuilding prompts.
- **AI project extraction:** extract a scene synopsis, POV, location, time, tags, characters, plot threads, beats and new Codex candidates as structured data, then review the result before explicitly applying it.
- **Generation safety and history:** reopen the last 20 responses, edit every suggestion before insertion, undo AI edits, and prevent stale generations from overwriting a document changed since the request began.
- **Handwritten vs AI review:** preserve the complete handwritten document beside a complete, editable AI-assisted version. Cursor continuations and selection rewrites are composed into the cloned AI document first; the handwritten version is untouched until explicitly replaced. Applied AI versions have a dedicated one-click undo stack.
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

Tagged releases automatically build tested binaries for Linux and Windows through the repository's Release workflow. Continuous integration runs formatting, tests and strict linting on both platforms for every pull request and push to `main`.

Release tags produce Linux AppImage and Debian installers plus Windows NSIS and MSI installers. Every release includes SHA-256 checksums and a keyless Sigstore signature. Windows Authenticode signing activates automatically after the repository secrets `WINDOWS_CERTIFICATE_BASE64` and `WINDOWS_CERTIFICATE_PASSWORD` are configured.

Open a folder containing `.md`, `.markdown`, or `.txt` files. The default Ollama endpoint is `http://127.0.0.1:11434` and can be changed in Settings.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl/Cmd+O` | Open project folder |
| `Ctrl/Cmd+N` | New document |
| `Ctrl/Cmd+S` | Save active document |
| `Ctrl/Cmd+Shift+S` | Save all open documents |
| `Ctrl/Cmd+F` | Find and replace |
| `Ctrl/Cmd+P` | Toggle editor/preview |
| `Ctrl/Cmd+Enter` | Generate with the selected Ollama action |
| `Escape` | Stop active Ollama generation |
| `F11` | Toggle focus mode |
| Middle-click a tab | Close and save it |

## Data and privacy

Novel Quill stores the manuscript and project metadata only in the folder you choose. Revision snapshots live under `.novelquill/history`. Global settings contain the last project path, Ollama endpoint, selected model and editor preferences. With the default endpoint, AI requests never leave your computer. If you configure a remote endpoint, the context shown by **Prompt preview** is sent to that server.

When enabled, the update checker sends only the installed version and a standard application identifier to GitHub's public Releases API. It never sends project paths or manuscript content. Updates open the signed GitHub release for deliberate installation rather than silently replacing the running executable.

## Beta quality program

The [beta testing guide](BETA_TESTING.md) covers long writing sessions, large projects, crash recovery, project locking, Ollama workflows and independent validation of every publishing format. Please use the structured GitHub bug-report form and never attach private manuscript text.

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
