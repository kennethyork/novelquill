use anyhow::{Context, Result};
use std::{fs, path::Path};

const PAGE_WIDTH: f32 = 432.0;
const PAGE_HEIGHT: f32 = 648.0;
const MARGIN_X: f32 = 54.0;
const TOP: f32 = 594.0;
const BOTTOM: f32 = 54.0;

#[derive(Clone, Copy)]
enum Face {
    Roman,
    Bold,
    Italic,
}

impl Face {
    fn resource(self) -> &'static str {
        match self {
            Self::Roman => "F1",
            Self::Bold => "F2",
            Self::Italic => "F3",
        }
    }
}

struct Typesetter {
    pages: Vec<String>,
    y: f32,
}

impl Typesetter {
    fn new() -> Self {
        Self {
            pages: vec![String::new()],
            y: TOP,
        }
    }

    fn page(&mut self) -> &mut String {
        self.pages.last_mut().expect("a PDF always has one page")
    }

    fn new_page(&mut self) {
        self.pages.push(String::new());
        self.y = TOP;
    }

    fn ensure_space(&mut self, height: f32) {
        if self.y - height < BOTTOM {
            self.new_page();
        }
    }

    fn vertical_space(&mut self, points: f32) {
        self.ensure_space(points);
        self.y -= points;
    }

    fn text_line(&mut self, text: &str, face: Face, size: f32, indent: f32, leading: f32) {
        self.ensure_space(leading);
        let encoded = pdf_text(text);
        let y = self.y;
        self.page().push_str(&format!(
            "BT /{} {:.1} Tf 1 0 0 1 {:.1} {:.1} Tm ({encoded}) Tj ET\n",
            face.resource(),
            size,
            MARGIN_X + indent,
            y
        ));
        self.y -= leading;
    }

    fn wrapped(&mut self, text: &str, face: Face, size: f32, indent: f32, leading: f32) {
        let usable_width = PAGE_WIDTH - (MARGIN_X * 2.0) - indent;
        let max_chars = (usable_width / (size * 0.48)).max(12.0) as usize;
        for line in wrap_words(text, max_chars) {
            self.text_line(&line, face, size, indent, leading);
        }
    }

    fn scene_break(&mut self) {
        self.vertical_space(8.0);
        self.text_line("* * *", Face::Roman, 11.0, 140.0, 18.0);
        self.vertical_space(6.0);
    }

    fn add_markdown(&mut self, markdown: &str, separate_document: bool) {
        if separate_document && !self.page().is_empty() {
            self.new_page();
        }
        let mut paragraph = String::new();
        for raw_line in markdown.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                self.flush_paragraph(&mut paragraph);
                continue;
            }
            if let Some(heading) = line.strip_prefix("# ") {
                self.flush_paragraph(&mut paragraph);
                self.vertical_space(20.0);
                self.wrapped(&strip_inline_markdown(heading), Face::Bold, 20.0, 0.0, 25.0);
                self.vertical_space(12.0);
            } else if let Some(heading) = line.strip_prefix("## ") {
                self.flush_paragraph(&mut paragraph);
                self.vertical_space(14.0);
                self.wrapped(&strip_inline_markdown(heading), Face::Bold, 15.0, 0.0, 20.0);
                self.vertical_space(7.0);
            } else if let Some(heading) = line.strip_prefix("### ") {
                self.flush_paragraph(&mut paragraph);
                self.vertical_space(10.0);
                self.wrapped(&strip_inline_markdown(heading), Face::Bold, 12.0, 0.0, 17.0);
            } else if let Some(quote) = line.strip_prefix("> ") {
                self.flush_paragraph(&mut paragraph);
                self.wrapped(
                    &strip_inline_markdown(quote),
                    Face::Italic,
                    10.5,
                    18.0,
                    15.0,
                );
                self.vertical_space(5.0);
            } else if line == "---" || line == "***" || line == "* * *" {
                self.flush_paragraph(&mut paragraph);
                self.scene_break();
            } else if let Some(item) = line.strip_prefix("- ") {
                self.flush_paragraph(&mut paragraph);
                self.wrapped(
                    &format!("• {}", strip_inline_markdown(item)),
                    Face::Roman,
                    11.0,
                    14.0,
                    16.0,
                );
            } else if !line.starts_with("```") {
                if !paragraph.is_empty() {
                    paragraph.push(' ');
                }
                paragraph.push_str(line);
            }
        }
        self.flush_paragraph(&mut paragraph);
    }

    fn flush_paragraph(&mut self, paragraph: &mut String) {
        if paragraph.is_empty() {
            return;
        }
        self.wrapped(
            &strip_inline_markdown(paragraph),
            Face::Roman,
            11.0,
            0.0,
            16.0,
        );
        self.vertical_space(7.0);
        paragraph.clear();
    }
}

pub fn export_markdown_documents<'a>(
    path: &Path,
    title: &str,
    documents: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let mut typesetter = Typesetter::new();
    typesetter.text_line(title, Face::Bold, 24.0, 0.0, 30.0);
    typesetter.vertical_space(20.0);
    let mut first = true;
    for document in documents {
        typesetter.add_markdown(document, !first);
        first = false;
    }
    add_page_numbers(&mut typesetter.pages);
    let pdf = build_pdf(&typesetter.pages);
    fs::write(path, pdf).with_context(|| format!("Could not write {}", path.display()))
}

fn add_page_numbers(pages: &mut [String]) {
    for (index, page) in pages.iter_mut().enumerate() {
        let number = index + 1;
        page.push_str(&format!("BT /F1 9 Tf 1 0 0 1 211 28 Tm ({number}) Tj ET\n"));
    }
}

fn build_pdf(page_streams: &[String]) -> Vec<u8> {
    let page_count = page_streams.len();
    let mut objects = Vec::<Vec<u8>>::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids = (0..page_count)
        .map(|i| format!("{} 0 R", 6 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!("<< /Type /Pages /Count {page_count} /Kids [{kids}] >>").into_bytes());
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Italic /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    for (index, stream) in page_streams.iter().enumerate() {
        let content_id = 7 + index * 2;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {content_id} 0 R >>"
        ).into_bytes());
        let bytes = stream.as_bytes();
        let mut object = format!("<< /Length {} >>\nstream\n", bytes.len()).into_bytes();
        object.extend_from_slice(bytes);
        object.extend_from_slice(b"endstream");
        objects.push(object);
    }

    let mut output = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(output.len());
        output.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        output.extend_from_slice(object);
        output.extend_from_slice(b"\nendobj\n");
    }
    let xref = output.len();
    output.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    output.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.into_iter().skip(1) {
        output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    output.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    output
}

fn wrap_words(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > max_chars {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn strip_inline_markdown(text: &str) -> String {
    text.replace("**", "")
        .replace("__", "")
        .replace(['`', '*', '_'], "")
}

fn pdf_text(text: &str) -> String {
    let mut output = String::new();
    for character in text.chars() {
        let byte = match character {
            '\u{2018}' | '\u{2019}' => b'\'',
            '\u{201C}' | '\u{201D}' => b'"',
            '\u{2013}' | '\u{2014}' => b'-',
            '\u{2026}' => {
                output.push_str("...");
                continue;
            }
            '\u{2022}' => 0x95,
            character if character.is_ascii() => character as u8,
            character if (character as u32) <= 0xff => character as u8,
            _ => b'?',
        };
        match byte {
            b'(' | b')' | b'\\' => {
                output.push('\\');
                output.push(byte as char);
            }
            32..=126 => output.push(byte as char),
            _ => output.push_str(&format!("\\{byte:03o}")),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_pdf_with_pages_and_xref() {
        let root = std::env::temp_dir().join(format!("novel-quill-pdf-{}.pdf", std::process::id()));
        export_markdown_documents(&root, "My Novel", ["# Chapter One\n\nHello, world."]).unwrap();
        let bytes = fs::read(&root).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.windows(4).any(|window| window == b"xref"));
        fs::remove_file(root).unwrap();
    }

    #[test]
    fn wraps_long_lines() {
        let lines = wrap_words("one two three four five", 10);
        assert_eq!(lines, ["one two", "three four", "five"]);
    }
}
