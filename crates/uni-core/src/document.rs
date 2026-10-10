//! Turns a downloaded file into text the model can read: plain text as it is,
//! PDFs page by page.

use std::ops::RangeInclusive;
use std::panic;

use crate::model::Document;
use crate::{Error, Result};

/// Largest file a connector downloads.
pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
/// Most text handed to the model in one answer.
pub const MAX_TEXT_BYTES: usize = 200 * 1024;

/// For refusing a download by its announced size, before fetching it.
pub fn check_size(bytes: u64) -> Result<()> {
    if bytes > MAX_FILE_BYTES as u64 {
        return Err(Error::InvalidInput(format!(
            "the file is larger than {} MB",
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

/// `pages` selects pages of a PDF, as `3` or `2-5`, counted from one.
pub fn read(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
    check_size(bytes.len() as u64)?;
    if bytes.starts_with(b"%PDF") {
        return read_pdf(bytes, pages);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::InvalidInput("only text and PDF files can be read".into()))?;
    let kept = fitting(text);
    Ok(Document {
        text: kept.to_owned(),
        page_count: None,
        pages: None,
        truncated: kept.len() < text.len(),
    })
}

fn read_pdf(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
    let unreadable = |why: String| Error::InvalidInput(format!("could not read this PDF: {why}"));
    // The PDF library panics on some damaged files.
    let all = panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(bytes))
        .map_err(|_| unreadable("it is damaged or uses an unsupported feature".into()))?
        .map_err(|e| unreadable(e.to_string()))?;

    let wanted = match pages {
        Some(pages) => parse_pages(pages, all.len())?,
        None => 1..=all.len(),
    };
    if all[wanted.start() - 1..*wanted.end()]
        .iter()
        .all(|page| page.trim().is_empty())
    {
        return Err(Error::InvalidInput(
            "this PDF contains no text; it is probably a scan".into(),
        ));
    }

    let mut text = String::new();
    let mut last = None;
    for number in wanted.clone() {
        let page = format!("[page {number}]\n{}\n\n", all[number - 1].trim());
        if text.len() + page.len() > MAX_TEXT_BYTES {
            // A single page that is too long is still better cut than left out.
            if last.is_none() {
                text.push_str(fitting(&page));
                last = Some(number);
            }
            break;
        }
        text.push_str(&page);
        last = Some(number);
    }
    let last = last.unwrap_or(*wanted.start());

    Ok(Document {
        text,
        page_count: Some(all.len()),
        pages: Some(format!("{}-{last}", wanted.start())),
        truncated: last < *wanted.end() || all[last - 1].trim().len() + 16 > MAX_TEXT_BYTES,
    })
}

fn parse_pages(pages: &str, count: usize) -> Result<RangeInclusive<usize>> {
    let invalid = || Error::InvalidInput(format!("`{pages}` is not a page range like 3 or 2-5"));
    let (first, last) = pages.trim().split_once('-').unwrap_or((pages, pages));
    let first: usize = first.trim().parse().map_err(|_| invalid())?;
    let last: usize = last.trim().parse().map_err(|_| invalid())?;
    if first == 0 || last < first {
        return Err(invalid());
    }
    if first > count {
        return Err(Error::InvalidInput(format!(
            "page {first} does not exist; the file has {count} pages"
        )));
    }
    Ok(first..=last.min(count))
}

/// The start of `text` that fits into one answer, cut between characters.
fn fitting(text: &str) -> &str {
    let mut end = text.len().min(MAX_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PDF with one line of text per page.
    fn pdf(pages: &[&str]) -> Vec<u8> {
        let count = pages.len();
        let kids: Vec<String> = (0..count).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {count} >>",
                kids.join(" ")
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        for (i, line) in pages.iter().enumerate() {
            let stream = format!("BT /F1 12 Tf 72 720 Td ({line}) Tj ET");
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                5 + 2 * i
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            ));
        }

        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    #[test]
    fn plain_text_is_returned_as_it_is() {
        let document = read("Übung 1\n".as_bytes(), None).expect("text");
        assert_eq!(document.text, "Übung 1\n");
        assert_eq!(document.page_count, None);
        assert!(!document.truncated);
    }

    #[test]
    fn long_text_is_cut_between_characters() {
        let text = "ä".repeat(MAX_TEXT_BYTES);
        let document = read(text.as_bytes(), None).expect("text");
        assert!(document.truncated);
        assert!(document.text.len() <= MAX_TEXT_BYTES);
        assert!(document.text.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn other_files_are_refused() {
        assert!(read(&[0xff, 0xd8, 0xff, 0xe0], None).is_err());
    }

    #[test]
    fn a_pdf_is_read_page_by_page() {
        let document = read(&pdf(&["Lecture one", "Lecture two"]), None).expect("pdf");
        assert_eq!(document.page_count, Some(2));
        assert_eq!(document.pages.as_deref(), Some("1-2"));
        assert!(!document.truncated);
        let first = document.text.find("[page 1]").expect("page 1");
        let second = document.text.find("[page 2]").expect("page 2");
        assert!(first < second);
        assert!(document.text[first..second].contains("Lecture one"));
        assert!(document.text[second..].contains("Lecture two"));
    }

    #[test]
    fn only_the_asked_pages_are_returned() {
        let bytes = pdf(&["Alpha", "Beta", "Gamma"]);
        let document = read(&bytes, Some("2")).expect("pdf");
        assert_eq!(document.pages.as_deref(), Some("2-2"));
        assert!(document.text.contains("Beta"));
        assert!(!document.text.contains("Alpha") && !document.text.contains("Gamma"));

        let rest = read(&bytes, Some("2-9")).expect("pdf");
        assert_eq!(rest.pages.as_deref(), Some("2-3"));
    }

    #[test]
    fn page_ranges_are_checked() {
        let bytes = pdf(&["Alpha", "Beta"]);
        for pages in ["0", "3", "2-1", "two", "1-"] {
            assert!(read(&bytes, Some(pages)).is_err(), "{pages}");
        }
    }

    #[test]
    fn a_pdf_without_text_says_so() {
        let error = read(&pdf(&[""]), None).expect_err("no text");
        assert!(error.to_string().contains("scan"));
    }

    #[test]
    fn a_damaged_pdf_is_an_error_not_a_crash() {
        assert!(read(b"%PDF-1.4\nthis is not a pdf", None).is_err());
    }
}
