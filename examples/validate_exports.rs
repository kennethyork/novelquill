#[path = "../src/export.rs"]
mod export;
#[path = "../src/pdf.rs"]
mod pdf;

use anyhow::{Context, Result};
use std::{fs, path::PathBuf};

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("novel-quill-export-validation"));
    fs::create_dir_all(&output)?;
    let documents = vec![
        "# Chapter One\n\n## Arrival\n\nMara entered the observatory. “The compass is awake,” she said.\n\n> Nothing stays buried forever.\n\n* * *\n\n- Preserve continuity\n- Test punctuation".to_owned(),
        "# Chapter Two\n\nThe storm crossed the valley while Mara followed the signal.".to_owned(),
    ];
    export::docx(
        &output.join("validation.docx"),
        "Novel Quill Validation",
        "Kenneth York",
        &documents,
    )?;
    export::odt(
        &output.join("validation.odt"),
        "Novel Quill Validation",
        "Kenneth York",
        &documents,
    )?;
    export::epub(
        &output.join("validation.epub"),
        "Novel Quill Validation",
        "Kenneth York",
        &documents,
    )?;
    export::html(
        &output.join("validation.html"),
        "Novel Quill Validation",
        "Kenneth York",
        &documents,
    )?;
    pdf::export_markdown_documents(
        &output.join("validation.pdf"),
        "Novel Quill Validation",
        documents.iter().map(String::as_str),
    )?;
    for name in [
        "validation.docx",
        "validation.odt",
        "validation.epub",
        "validation.html",
        "validation.pdf",
    ] {
        let path = output.join(name);
        let size = fs::metadata(&path)
            .with_context(|| format!("Missing {}", path.display()))?
            .len();
        println!("{}\t{} bytes", path.display(), size);
    }
    Ok(())
}
