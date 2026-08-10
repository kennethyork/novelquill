# Novel Quill Studio Beta Testing

Novel Quill Studio targets Linux and Windows. Manuscripts should always be backed up before testing a pre-release build, even though the app uses atomic saves, revisions, recoverable Trash and crash snapshots.

## High-value test sessions

1. Open an existing Markdown novel with at least 50 scenes.
2. Write and edit for 30 minutes with autosave enabled.
3. Reorder scenes by dragging file rows and verify the Outline order.
4. Generate Ollama continuations at the beginning, middle and end of a document.
5. Generate three alternatives, apply one, undo it and inspect the exact context.
6. Extract scene metadata and verify every field before applying it.
7. Export PDF, DOCX, ODT, EPUB and HTML, then open each result in an independent reader.
8. Force-close a test session with unsaved text, reopen it and verify Recovery.
9. Try opening the same project in two application windows and verify the second is rejected.

## Reporting a problem

Use the repository's bug-report form. Include:

- Novel Quill version and installer type
- Linux distribution or Windows version
- Ollama version and model, when AI is involved
- Exact steps that reproduce the issue
- Expected and actual behavior
- Whether Recovery, Trash or Revision History protected the manuscript

Never attach a private manuscript unless you intentionally want it to be public.
