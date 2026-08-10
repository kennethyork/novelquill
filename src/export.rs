use anyhow::{Context, Result};
use std::{fs, path::Path};

pub fn html(path: &Path, title: &str, author: &str, documents: &[String]) -> Result<()> {
    let body = documents
        .iter()
        .map(|document| markdown_to_html(document))
        .collect::<Vec<_>>()
        .join("\n<hr class=\"document-break\">\n");
    let output = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body><main><h1>{}</h1><p class=\"author\">{}</p>{body}</main></body></html>",
        xml(title),
        "body{font-family:Georgia,serif;line-height:1.6;margin:0;background:#eee;color:#222}main{max-width:46rem;margin:2rem auto;padding:4rem;background:white}h1,h2,h3{line-height:1.2}p{text-indent:1.5em;margin:.25em 0}.author{text-indent:0}.document-break{border:0;break-before:page;margin:3rem 0}blockquote{font-style:italic;color:#555}",
        xml(title),
        xml(author)
    );
    fs::write(path, output).with_context(|| format!("Could not write {}", path.display()))
}

pub fn docx(path: &Path, title: &str, author: &str, documents: &[String]) -> Result<()> {
    let mut paragraphs = format!(
        "<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr><w:r><w:t>{}</w:t></w:r></w:p><w:p><w:r><w:t>{}</w:t></w:r></w:p>",
        xml(title),
        xml(author)
    );
    for (index, document) in documents.iter().enumerate() {
        if index > 0 {
            paragraphs.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        }
        for line in document.lines() {
            let trimmed = line.trim();
            let (style, text) = if let Some(value) = trimmed.strip_prefix("# ") {
                (Some("Heading1"), value)
            } else if let Some(value) = trimmed.strip_prefix("## ") {
                (Some("Heading2"), value)
            } else if let Some(value) = trimmed.strip_prefix("### ") {
                (Some("Heading3"), value)
            } else {
                (None, trimmed)
            };
            if text.is_empty() {
                paragraphs.push_str("<w:p/>");
                continue;
            }
            paragraphs.push_str("<w:p>");
            if let Some(style) = style {
                paragraphs.push_str(&format!("<w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>"));
            }
            paragraphs.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                xml(&strip_markdown(text))
            ));
        }
    }
    let document_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{paragraphs}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr></w:body></w:document>"
    );
    let entries = vec![
        ("[Content_Types].xml", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".to_owned()),
        ("_rels/.rels", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>".to_owned()),
        ("word/document.xml", document_xml),
    ];
    write_zip(path, &entries)
}

pub fn odt(path: &Path, title: &str, author: &str, documents: &[String]) -> Result<()> {
    let mut body = format!(
        "<text:h text:outline-level=\"1\">{}</text:h><text:p>{}</text:p>",
        xml(title),
        xml(author)
    );
    for document in documents {
        for line in document.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("# ") {
                body.push_str(&format!(
                    "<text:h text:outline-level=\"1\">{}</text:h>",
                    xml(value)
                ));
            } else if let Some(value) = trimmed.strip_prefix("## ") {
                body.push_str(&format!(
                    "<text:h text:outline-level=\"2\">{}</text:h>",
                    xml(value)
                ));
            } else {
                body.push_str(&format!(
                    "<text:p>{}</text:p>",
                    xml(&strip_markdown(trimmed))
                ));
            }
        }
    }
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" office:version=\"1.3\"><office:body><office:text>{body}</office:text></office:body></office:document-content>"
    );
    let entries = vec![
        ("mimetype", "application/vnd.oasis.opendocument.text".to_owned()),
        ("content.xml", content),
        ("META-INF/manifest.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>".to_owned()),
    ];
    write_zip(path, &entries)
}

pub fn epub(path: &Path, title: &str, author: &str, documents: &[String]) -> Result<()> {
    let mut entries = vec![
        ("mimetype", "application/epub+zip".to_owned()),
        ("META-INF/container.xml", "<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>".to_owned()),
    ];
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut navigation = String::new();
    for (index, document) in documents.iter().enumerate() {
        let number = index + 1;
        let id = format!("chapter{number}");
        let filename = format!("{id}.xhtml");
        manifest.push_str(&format!(
            "<item id=\"{id}\" href=\"{filename}\" media-type=\"application/xhtml+xml\"/>"
        ));
        spine.push_str(&format!("<itemref idref=\"{id}\"/>"));
        navigation.push_str(&format!(
            "<li><a href=\"{filename}\">Chapter {number}</a></li>"
        ));
        entries.push((
            Box::leak(format!("OEBPS/{filename}").into_boxed_str()),
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{}</title></head><body>{}</body></html>", xml(title), markdown_to_html(document)),
        ));
    }
    entries.push(("OEBPS/nav.xhtml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>Contents</title></head><body><nav epub:type=\"toc\"><ol>{navigation}</ol></nav></body></html>")));
    entries.push(("OEBPS/content.opf", format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"book-id\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"book-id\">novel-quill-export</dc:identifier><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:language>en</dc:language></metadata><manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>{manifest}</manifest><spine>{spine}</spine></package>", xml(title), xml(author))));
    write_zip(path, &entries)
}

fn markdown_to_html(markdown: &str) -> String {
    let mut output = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("# ") {
            output.push_str(&format!("<h1>{}</h1>", xml(value)));
        } else if let Some(value) = trimmed.strip_prefix("## ") {
            output.push_str(&format!("<h2>{}</h2>", xml(value)));
        } else if let Some(value) = trimmed.strip_prefix("### ") {
            output.push_str(&format!("<h3>{}</h3>", xml(value)));
        } else if let Some(value) = trimmed.strip_prefix("> ") {
            output.push_str(&format!("<blockquote>{}</blockquote>", xml(value)));
        } else if trimmed == "---" || trimmed == "***" {
            output.push_str("<hr>");
        } else if trimmed.is_empty() {
            output.push_str("<p></p>");
        } else {
            output.push_str(&format!("<p>{}</p>", xml(&strip_markdown(trimmed))));
        }
    }
    output
}

fn strip_markdown(text: &str) -> String {
    text.replace("**", "")
        .replace("__", "")
        .replace(['*', '_', '`'], "")
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn write_zip(path: &Path, entries: &[(&str, String)]) -> Result<()> {
    let mut output = Vec::new();
    let mut central = Vec::new();
    for (name, content) in entries {
        let name = name.as_bytes();
        let data = content.as_bytes();
        let crc = crc32(data);
        let offset = output.len() as u32;
        output.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        output.extend_from_slice(&20u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&crc.to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(name.len() as u16).to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(name);
        output.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let central_offset = output.len() as u32;
    output.extend_from_slice(&central);
    output.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&(central.len() as u32).to_le_bytes());
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    fs::write(path, output).with_context(|| format!("Could not write {}", path.display()))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(bytes: &[u8], needle: &[u8]) -> bool {
        bytes.windows(needle.len()).any(|window| window == needle)
    }

    #[test]
    fn crc_matches_standard_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn office_export_is_a_zip() {
        let path = std::env::temp_dir().join(format!("novel-quill-{}.docx", std::process::id()));
        docx(&path, "Novel", "Author", &["# One\n\nText".into()]).unwrap();
        assert!(fs::read(&path).unwrap().starts_with(b"PK\x03\x04"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn docx_contains_required_openxml_parts_and_escaped_text() {
        let path = std::env::temp_dir().join(format!(
            "novel-quill-conformance-{}.docx",
            std::process::id()
        ));
        docx(&path, "A & B", "Author", &["# One\n\n<Conflict>".into()]).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(contains(&bytes, b"[Content_Types].xml"));
        assert!(contains(&bytes, b"word/document.xml"));
        assert!(contains(&bytes, b"A &amp; B"));
        assert!(contains(&bytes, b"&lt;Conflict&gt;"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn odt_contains_mimetype_content_and_manifest() {
        let path = std::env::temp_dir().join(format!(
            "novel-quill-conformance-{}.odt",
            std::process::id()
        ));
        odt(&path, "Novel", "Author", &["# One\n\nText".into()]).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(contains(&bytes, b"application/vnd.oasis.opendocument.text"));
        assert!(contains(&bytes, b"content.xml"));
        assert!(contains(&bytes, b"META-INF/manifest.xml"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn epub_contains_epub3_navigation_package_and_chapters() {
        let path = std::env::temp_dir().join(format!(
            "novel-quill-conformance-{}.epub",
            std::process::id()
        ));
        epub(
            &path,
            "Novel",
            "Author",
            &["# One\n\nText".into(), "# Two\n\nMore".into()],
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(contains(&bytes, b"application/epub+zip"));
        assert!(contains(&bytes, b"META-INF/container.xml"));
        assert!(contains(&bytes, b"OEBPS/content.opf"));
        assert!(contains(&bytes, b"OEBPS/nav.xhtml"));
        assert!(contains(&bytes, b"chapter2.xhtml"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn html_export_has_utf8_metadata_and_escapes_user_content() {
        let path = std::env::temp_dir().join(format!(
            "novel-quill-conformance-{}.html",
            std::process::id()
        ));
        html(&path, "A & B", "<Author>", &["Danger & hope".into()]).unwrap();
        let output = fs::read_to_string(&path).unwrap();
        assert!(output.contains("<meta charset=\"utf-8\">"));
        assert!(output.contains("A &amp; B"));
        assert!(output.contains("&lt;Author&gt;"));
        assert!(output.contains("Danger &amp; hope"));
        fs::remove_file(path).unwrap();
    }
}
