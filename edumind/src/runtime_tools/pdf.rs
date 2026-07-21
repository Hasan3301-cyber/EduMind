use std::{fs, path::Path};

use serde::Serialize;

use crate::{
    config::types::ExecutionCapsConfig,
    infra::{EduMindError, Result, run_blocking},
    research::{MAX_PDF_BYTES, extract_pdf_text},
};

/// Bounded text extracted from a local PDF file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PdfTextExtraction {
    pub path: String,
    pub text: String,
    pub char_count: usize,
    pub truncated: bool,
}

/// Lightweight local analysis derived from a PDF text layer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PdfAnalysis {
    pub extraction: PdfTextExtraction,
    pub word_count: usize,
    pub headings: Vec<String>,
}

/// Extracts a PDF text layer without sending the document to a remote provider.
pub async fn extract_pdf(path: &Path, caps: &ExecutionCapsConfig) -> Result<PdfTextExtraction> {
    let text = load_pdf_text(path).await?;
    Ok(bounded_extraction(path, text, caps.max_output_bytes))
}

/// Produces deterministic heading and word-count metadata from local PDF text.
pub async fn analyze_pdf(path: &Path, caps: &ExecutionCapsConfig) -> Result<PdfAnalysis> {
    let text = load_pdf_text(path).await?;
    let word_count = text.split_whitespace().count();
    let headings = text
        .lines()
        .map(str::trim)
        .filter(|line| is_heading(line))
        .take(12)
        .map(ToOwned::to_owned)
        .collect();
    Ok(PdfAnalysis {
        extraction: bounded_extraction(path, text, caps.max_output_bytes),
        word_count,
        headings,
    })
}

async fn load_pdf_text(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    let bytes = run_blocking(move || read_bounded_pdf(&path)).await?;
    run_blocking(move || extract_pdf_text(&bytes)).await
}

fn read_bounded_pdf(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(EduMindError::Tool(
            "PDF tools require a regular local file".to_owned(),
        ));
    }
    if metadata.len() > u64::try_from(MAX_PDF_BYTES).unwrap_or(u64::MAX) {
        return Err(EduMindError::Tool(format!(
            "PDF exceeds the {} MiB runtime cap",
            MAX_PDF_BYTES / (1024 * 1024)
        )));
    }
    fs::read(path).map_err(|source| EduMindError::StorageIo {
        path: path.to_path_buf(),
        source,
    })
}

fn bounded_extraction(path: &Path, text: String, maximum: usize) -> PdfTextExtraction {
    let char_count = text.chars().count();
    let (text, truncated) = truncate_utf8(&text, maximum);
    PdfTextExtraction {
        path: path.display().to_string(),
        text,
        char_count,
        truncated,
    }
}

fn truncate_utf8(text: &str, maximum: usize) -> (String, bool) {
    if text.len() <= maximum {
        return (text.to_owned(), false);
    }
    let mut boundary = maximum;
    while !text.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    (text[..boundary].to_owned(), true)
}

fn is_heading(line: &str) -> bool {
    let words = line.split_whitespace().count();
    let ending = line.chars().last();
    !line.is_empty()
        && line.chars().count() <= 110
        && (line.chars().all(|character| !character.is_lowercase())
            || (words <= 12 && !matches!(ending, Some('.' | '!' | '?' | ';' | ':'))))
}

#[cfg(test)]
mod tests {
    use super::truncate_utf8;

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        let (text, truncated) = truncate_utf8("abc🙂def", 5);

        assert_eq!(text, "abc");
        assert!(truncated);
    }
}
