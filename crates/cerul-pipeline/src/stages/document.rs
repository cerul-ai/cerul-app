use std::{
    any::Any,
    fs::File,
    io::Read,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
};

use anyhow::Context;
use quick_xml::{
    escape::resolve_predefined_entity,
    events::{BytesRef, Event},
    Reader,
};
use serde_json::json;
use zip::ZipArchive;

const DOCUMENT_CHUNK_BUDGET: usize = 3_200;
const MAX_PLAIN_TEXT_DOCUMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PDF_DOCUMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_OFFICE_XML_PART_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExtractedDocumentChunk {
    pub(crate) text: String,
    pub(crate) page: Option<u32>,
    pub(crate) section: Option<String>,
    pub(crate) metadata: serde_json::Value,
}

pub(crate) fn extract_document_chunks(path: &Path) -> anyhow::Result<Vec<ExtractedDocumentChunk>> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .context("document path has no file extension")?;

    let chunks = match extension.as_str() {
        "txt" | "md" | "markdown" => extract_plain_text_chunks(path, &extension)?,
        "pdf" => extract_pdf_chunks(path)?,
        "docx" => extract_docx_chunks(path)?,
        "pptx" => extract_pptx_chunks(path)?,
        other => anyhow::bail!("unsupported document extension: {other}"),
    };

    let chunks = chunks
        .into_iter()
        .filter(|chunk| !chunk.text.trim().is_empty())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        !chunks.is_empty(),
        "document produced no searchable text: {}",
        path.display()
    );
    Ok(chunks)
}

fn extract_plain_text_chunks(
    path: &Path,
    extension: &str,
) -> anyhow::Result<Vec<ExtractedDocumentChunk>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to read document metadata: {}", path.display()))?;
    anyhow::ensure!(
        metadata.len() <= MAX_PLAIN_TEXT_DOCUMENT_BYTES,
        "plain text document exceeds {} byte limit: {}",
        MAX_PLAIN_TEXT_DOCUMENT_BYTES,
        path.display()
    );
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read document text: {}", path.display()))?;
    Ok(split_text_chunks(
        &text,
        Some(1),
        None,
        json!({ "format": extension }),
    ))
}

fn extract_pdf_chunks(path: &Path) -> anyhow::Result<Vec<ExtractedDocumentChunk>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to read document metadata: {}", path.display()))?;
    anyhow::ensure!(
        metadata.len() <= MAX_PDF_DOCUMENT_BYTES,
        "PDF document exceeds {} byte limit: {}",
        MAX_PDF_DOCUMENT_BYTES,
        path.display()
    );
    let pages = catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_by_pages(path)
    }))
    .map_err(|panic| {
        anyhow::anyhow!(
            "PDF text extraction panicked for {}: {}",
            path.display(),
            panic_payload_message(panic.as_ref())
        )
    })?
    .with_context(|| format!("failed to extract PDF text: {}", path.display()))?;
    let mut chunks = Vec::new();
    for (index, page_text) in pages.into_iter().enumerate() {
        let page = u32::try_from(index + 1).ok();
        chunks.extend(split_text_chunks(
            &page_text,
            page,
            None,
            json!({ "format": "pdf" }),
        ));
    }
    Ok(chunks)
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic payload".to_string())
}

fn extract_docx_chunks(path: &Path) -> anyhow::Result<Vec<ExtractedDocumentChunk>> {
    let mut archive = open_zip(path)?;
    let xml = read_zip_text(&mut archive, "word/document.xml")
        .with_context(|| format!("failed to read docx document.xml: {}", path.display()))?;
    let text = extract_xml_text(&xml)?;
    Ok(split_text_chunks(
        &text,
        None,
        None,
        json!({ "format": "docx", "part": "word/document.xml" }),
    ))
}

fn extract_pptx_chunks(path: &Path) -> anyhow::Result<Vec<ExtractedDocumentChunk>> {
    let mut archive = open_zip(path)?;
    let mut slide_names = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let name = file.name().to_string();
        if name.starts_with("ppt/slides/slide")
            && name.ends_with(".xml")
            && !name.contains("/_rels/")
        {
            slide_names.push(name);
        }
    }
    slide_names.sort_by_key(|name| slide_number(name).unwrap_or(u32::MAX));

    let mut chunks = Vec::new();
    for name in slide_names {
        let page = slide_number(&name);
        let xml = read_zip_text(&mut archive, &name)?;
        let text = extract_xml_text(&xml)?;
        chunks.extend(split_text_chunks(
            &text,
            page,
            None,
            json!({ "format": "pptx", "part": name }),
        ));
    }
    Ok(chunks)
}

fn open_zip(path: &Path) -> anyhow::Result<ZipArchive<File>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    ZipArchive::new(file).with_context(|| format!("failed to read zip package {}", path.display()))
}

fn read_zip_text(archive: &mut ZipArchive<File>, name: &str) -> anyhow::Result<String> {
    let mut file = archive
        .by_name(name)
        .with_context(|| format!("missing package part {name}"))?;
    anyhow::ensure!(
        file.size() <= MAX_OFFICE_XML_PART_BYTES,
        "office XML part {name} exceeds {} byte limit",
        MAX_OFFICE_XML_PART_BYTES
    );
    let mut text = String::new();
    file.read_to_string(&mut text)
        .with_context(|| format!("failed to read package part {name}"))?;
    Ok(text)
}

fn extract_xml_text(xml: &str) -> anyhow::Result<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut text = String::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Text(event) => append_inline_text(&mut text, &event.decode()?),
            Event::GeneralRef(event) => {
                append_inline_text(&mut text, &decode_xml_reference(&event)?)
            }
            Event::CData(event) => append_inline_text(&mut text, &String::from_utf8_lossy(&event)),
            Event::Empty(event) if is_break_element(event.name().as_ref()) => {
                ensure_paragraph_break(&mut text)
            }
            Event::End(event) if is_block_element(event.name().as_ref()) => {
                ensure_paragraph_break(&mut text)
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(normalize_text_blocks(&text))
}

fn decode_xml_reference(reference: &BytesRef<'_>) -> anyhow::Result<String> {
    if let Some(ch) = reference.resolve_char_ref()? {
        return Ok(ch.to_string());
    }
    let name = reference.decode()?;
    Ok(resolve_predefined_entity(&name)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("&{name};")))
}

fn append_inline_text(output: &mut String, text: &str) {
    let has_leading_space = text.chars().next().is_some_and(char::is_whitespace);
    let has_trailing_space = text.chars().last().is_some_and(char::is_whitespace);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return;
    }
    if has_leading_space && !output.is_empty() && !output.ends_with(char::is_whitespace) {
        output.push(' ');
    }
    output.push_str(&text);
    if has_trailing_space {
        output.push(' ');
    }
}

fn ensure_paragraph_break(output: &mut String) {
    let trimmed_len = output.trim_end().len();
    output.truncate(trimmed_len);
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push_str("\n\n");
    }
}

fn is_block_element(name: &[u8]) -> bool {
    matches!(local_name(name), b"p" | b"txBody")
}

fn is_break_element(name: &[u8]) -> bool {
    matches!(local_name(name), b"br")
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn split_text_chunks(
    text: &str,
    page: Option<u32>,
    default_section: Option<String>,
    metadata: serde_json::Value,
) -> Vec<ExtractedDocumentChunk> {
    let text = normalize_text_blocks(text);
    if text.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_section = default_section.or_else(|| first_section_title(&text));

    for paragraph in text
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let paragraph_section = heading_title(paragraph);
        let paragraph_segments = split_paragraph_segments(paragraph, DOCUMENT_CHUNK_BUDGET);
        for (segment_index, segment) in paragraph_segments.into_iter().enumerate() {
            if segment_index == 0 {
                if let Some(section) = paragraph_section.clone() {
                    if !current.is_empty() {
                        chunks.push(document_chunk(
                            std::mem::take(&mut current),
                            page,
                            current_section.clone(),
                            metadata.clone(),
                        ));
                    }
                    current_section = Some(section);
                }
            }
            let next_len = char_count(&current) + char_count(&segment) + 2;
            if !current.is_empty() && next_len > DOCUMENT_CHUNK_BUDGET {
                chunks.push(document_chunk(
                    std::mem::take(&mut current),
                    page,
                    current_section.clone(),
                    metadata.clone(),
                ));
                if let Some(section) = paragraph_section
                    .clone()
                    .or_else(|| first_section_title(&segment))
                {
                    current_section = Some(section);
                }
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(&segment);
        }
    }

    if !current.trim().is_empty() {
        chunks.push(document_chunk(
            current,
            page,
            current_section,
            metadata.clone(),
        ));
    }
    chunks
}

fn split_paragraph_segments(paragraph: &str, budget: usize) -> Vec<String> {
    if char_count(paragraph) <= budget {
        return vec![paragraph.to_string()];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    for word in paragraph.split_whitespace() {
        let word_len = char_count(word);
        if word_len > budget {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            segments.extend(split_long_word(word, budget));
            continue;
        }

        let separator = usize::from(!current.is_empty());
        if !current.is_empty() && char_count(&current) + separator + word_len > budget {
            segments.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn split_long_word(word: &str, budget: usize) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        if char_count(&current) >= budget {
            segments.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn document_chunk(
    text: String,
    page: Option<u32>,
    section: Option<String>,
    mut metadata: serde_json::Value,
) -> ExtractedDocumentChunk {
    if !metadata.is_object() {
        metadata = json!({});
    }
    ExtractedDocumentChunk {
        text: text.trim().to_string(),
        page,
        section,
        metadata,
    }
}

fn normalize_text_blocks(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

fn first_section_title(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(heading_title)
        .or_else(|| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && line.chars().count() <= 120)
                .map(ToOwned::to_owned)
        })
}

fn heading_title(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let heading = trimmed.strip_prefix('#')?.trim_start_matches('#').trim();
    (!heading.is_empty() && heading.chars().count() <= 120).then(|| heading.to_string())
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn slide_number(name: &str) -> Option<u32> {
    let file_name = PathBuf::from(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToOwned::to_owned)?;
    file_name.strip_prefix("slide")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_office_xml_text_with_paragraph_breaks() {
        let xml = r#"
            <w:document xmlns:w="w">
              <w:body>
                <w:p><w:r><w:t>Alpha &amp; beta</w:t></w:r></w:p>
                <w:p><w:r><w:t>Launch notes</w:t></w:r></w:p>
              </w:body>
            </w:document>
        "#;

        let text = extract_xml_text(xml).unwrap();

        assert!(text.contains("Alpha & beta"));
        assert!(text.contains("Launch notes"));
    }

    #[test]
    fn extracts_office_xml_text_without_synthetic_run_spaces() {
        let xml = r#"
            <w:document xmlns:w="w">
              <w:body>
                <w:p>
                  <w:r><w:t>Pro</w:t></w:r>
                  <w:r><w:t>duct</w:t></w:r>
                  <w:r><w:t xml:space="preserve"> launch</w:t></w:r>
                </w:p>
              </w:body>
            </w:document>
        "#;

        let text = extract_xml_text(xml).unwrap();

        assert!(text.contains("Product launch"));
        assert!(!text.contains("Pro duct"));
    }

    #[test]
    fn split_text_chunks_keeps_page_and_section() {
        let chunks = split_text_chunks(
            "# Roadmap\nShip document search",
            Some(3),
            None,
            json!({ "format": "md" }),
        );

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].page, Some(3));
        assert_eq!(chunks[0].section.as_deref(), Some("Roadmap"));
        assert_eq!(chunks[0].metadata["format"], "md");
    }

    #[test]
    fn split_text_chunks_flushes_before_new_heading() {
        let chunks = split_text_chunks(
            "# Section A\nalpha evidence\n# Section B\nbeta evidence",
            Some(1),
            None,
            json!({ "format": "md" }),
        );

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].section.as_deref(), Some("Section A"));
        assert!(chunks[0].text.contains("alpha evidence"));
        assert!(!chunks[0].text.contains("Section B"));
        assert_eq!(chunks[1].section.as_deref(), Some("Section B"));
        assert!(chunks[1].text.contains("beta evidence"));
    }

    #[test]
    fn split_text_chunks_splits_overlong_paragraphs() {
        let paragraph = std::iter::repeat_n("launchword", 500)
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = split_text_chunks(
            &format!("# Roadmap\n{paragraph}"),
            Some(1),
            None,
            json!({ "format": "md" }),
        );

        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| char_count(&chunk.text) <= DOCUMENT_CHUNK_BUDGET));
        assert!(chunks
            .iter()
            .all(|chunk| chunk.section.as_deref() == Some("Roadmap")));
    }

    #[test]
    fn extract_plain_text_chunks_rejects_oversized_files_before_reading() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.txt");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_PLAIN_TEXT_DOCUMENT_BYTES + 1).unwrap();

        let error = extract_plain_text_chunks(&path, "txt")
            .unwrap_err()
            .to_string();

        assert!(error.contains("plain text document exceeds"));
    }

    #[test]
    fn extract_docx_rejects_oversized_xml_part_before_reading() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.docx");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        let mut oversized = std::io::repeat(b'a').take(MAX_OFFICE_XML_PART_BYTES + 1);
        std::io::copy(&mut oversized, &mut zip).unwrap();
        zip.finish().unwrap();

        let error = extract_docx_chunks(&path).unwrap_err();
        let error_chain = format!("{error:#}");

        assert!(
            error_chain.contains("office XML part word/document.xml exceeds"),
            "unexpected error: {error_chain}"
        );
    }

    #[test]
    fn extract_pdf_chunks_rejects_oversized_files_before_extraction() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.pdf");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_PDF_DOCUMENT_BYTES + 1).unwrap();

        let error = extract_pdf_chunks(&path).unwrap_err().to_string();

        assert!(error.contains("PDF document exceeds"));
    }

    #[test]
    fn extract_pdf_chunks_reports_malformed_pdf_as_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("broken.pdf");
        std::fs::write(&path, b"not a pdf").unwrap();

        let error = extract_pdf_chunks(&path).unwrap_err().to_string();

        assert!(
            error.contains("failed to extract PDF text")
                || error.contains("PDF text extraction panicked"),
            "unexpected PDF extraction error: {error}"
        );
    }
}
