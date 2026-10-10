//! Turns a downloaded file into something the model can take in: text, and
//! pictures of PDF pages when the text alone would lose what is on them.

use std::ops::RangeInclusive;
use std::panic::{self, AssertUnwindSafe};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use crate::model::{Document, Image};
use crate::{Error, Result};

/// Largest file a connector downloads.
pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
/// Most text handed to the model in one answer.
pub const MAX_TEXT_BYTES: usize = 200 * 1024;
/// Most page pictures in one answer; each costs the model about as much as a
/// page of text.
pub const MAX_PAGE_IMAGES: usize = 5;
/// Longer side of a page picture, in pixels.
const PAGE_IMAGE_SIDE: f32 = 1400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The text of the file.
    Text,
    /// For a PDF, pictures of its pages next to their text.
    Pages,
}

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
pub fn read(bytes: &[u8], pages: Option<&str>, view: View) -> Result<Document> {
    check_size(bytes.len() as u64)?;
    if bytes.starts_with(b"%PDF") {
        return match view {
            View::Text => pdf_text(bytes, pages),
            View::Pages => pdf_pages(bytes, pages),
        };
    }
    if let Some(mime_type) = image_type(bytes) {
        return Ok(Document {
            text: String::new(),
            page_count: None,
            pages: None,
            truncated: false,
            images: vec![Image {
                mime_type,
                data: bytes.to_vec(),
                page: None,
            }],
        });
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::InvalidInput("only text, PDF and image files can be read".into()))?;
    let kept = fitting(text);
    Ok(Document {
        text: kept.to_owned(),
        page_count: None,
        pages: None,
        truncated: kept.len() < text.len(),
        images: Vec::new(),
    })
}

fn image_type(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("image/webp"),
        _ => None,
    }
}

fn unreadable(why: impl std::fmt::Display) -> Error {
    Error::InvalidInput(format!("could not read this PDF: {why}"))
}

/// The text of every page. The PDF libraries panic on some damaged files.
fn page_texts(bytes: &[u8]) -> Result<Vec<String>> {
    panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(bytes))
        .map_err(|_| unreadable("it is damaged or uses an unsupported feature"))?
        .map_err(unreadable)
}

fn pdf_text(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
    let all = page_texts(bytes)?;
    let wanted = match pages {
        Some(pages) => parse_pages(pages, all.len())?,
        None => 1..=all.len(),
    };
    if all[wanted.start() - 1..*wanted.end()]
        .iter()
        .all(|page| page.trim().is_empty())
    {
        return Err(Error::InvalidInput(
            "these pages contain no text, they are probably scans; ask for them with view `pages`"
                .into(),
        ));
    }

    let mut text = String::new();
    let mut last = None;
    for number in wanted.clone() {
        let page = page_text(number, &all[number - 1]);
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
        truncated: last < *wanted.end() || page_text(last, &all[last - 1]).len() > MAX_TEXT_BYTES,
        text,
        page_count: Some(all.len()),
        pages: Some(format!("{}-{last}", wanted.start())),
        images: Vec::new(),
    })
}

fn pdf_pages(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
    let rendered = panic::catch_unwind(AssertUnwindSafe(|| render(bytes, pages)))
        .map_err(|_| unreadable("it is damaged or uses an unsupported feature"))??;
    // A scan has no text, which is exactly when the pictures are needed.
    let texts = page_texts(bytes).unwrap_or_default();

    let first = rendered.images.first().and_then(|image| image.page);
    let last = rendered.images.last().and_then(|image| image.page);
    let (Some(first), Some(last)) = (first, last) else {
        return Err(unreadable("it has no pages"));
    };
    let text = (first..=last)
        .filter_map(|number| Some(page_text(number, texts.get(number - 1)?)))
        .collect::<String>();

    Ok(Document {
        text: fitting(&text).to_owned(),
        page_count: Some(rendered.page_count),
        pages: Some(format!("{first}-{last}")),
        truncated: last < rendered.wanted_last,
        images: rendered.images,
    })
}

struct Rendered {
    images: Vec<Image>,
    page_count: usize,
    wanted_last: usize,
}

fn render(bytes: &[u8], pages: Option<&str>) -> Result<Rendered> {
    let pdf = Pdf::new(bytes.to_vec()).map_err(|_| unreadable("it is not a valid PDF"))?;
    let all = pdf.pages();
    let wanted = match pages {
        Some(pages) => parse_pages(pages, all.len())?,
        None => 1..=all.len(),
    };

    let cache = RenderCache::new();
    let mut images = Vec::new();
    for number in wanted.clone().take(MAX_PAGE_IMAGES) {
        let page = all
            .get(number - 1)
            .ok_or_else(|| unreadable("a page is missing"))?;
        let (width, height) = page.render_dimensions();
        let scale = PAGE_IMAGE_SIDE / width.max(height).max(1.0);
        let pixmap = hayro::render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
            },
        );
        images.push(Image {
            mime_type: "image/png",
            data: pixmap.into_png().map_err(unreadable)?,
            page: Some(number),
        });
    }
    Ok(Rendered {
        images,
        page_count: all.len(),
        wanted_last: *wanted.end(),
    })
}

fn page_text(number: usize, text: &str) -> String {
    format!("[page {number}]\n{}\n\n", text.trim())
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

    fn text(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
        read(bytes, pages, View::Text)
    }

    #[test]
    fn plain_text_is_returned_as_it_is() {
        let document = text("Übung 1\n".as_bytes(), None).expect("text");
        assert_eq!(document.text, "Übung 1\n");
        assert_eq!(document.page_count, None);
        assert!(!document.truncated);
        assert!(document.images.is_empty());
    }

    #[test]
    fn long_text_is_cut_between_characters() {
        let long = "ä".repeat(MAX_TEXT_BYTES);
        let document = text(long.as_bytes(), None).expect("text");
        assert!(document.truncated);
        assert!(document.text.len() <= MAX_TEXT_BYTES);
        assert!(document.text.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn other_files_are_refused() {
        assert!(text(&[0x50, 0x4b, 0x03, 0x04, 0xff], None).is_err());
    }

    #[test]
    fn a_picture_is_handed_over_as_a_picture() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0xff];
        let document = text(&png, None).expect("image");
        assert_eq!(document.text, "");
        assert_eq!(document.images.len(), 1);
        assert_eq!(document.images[0].mime_type, "image/png");
        assert_eq!(document.images[0].data, png);
    }

    #[test]
    fn a_pdf_is_read_page_by_page() {
        let document = text(&pdf(&["Lecture one", "Lecture two"]), None).expect("pdf");
        assert_eq!(document.page_count, Some(2));
        assert_eq!(document.pages.as_deref(), Some("1-2"));
        assert!(!document.truncated);
        assert!(document.images.is_empty());
        let first = document.text.find("[page 1]").expect("page 1");
        let second = document.text.find("[page 2]").expect("page 2");
        assert!(first < second);
        assert!(document.text[first..second].contains("Lecture one"));
        assert!(document.text[second..].contains("Lecture two"));
    }

    #[test]
    fn only_the_asked_pages_are_returned() {
        let bytes = pdf(&["Alpha", "Beta", "Gamma"]);
        let document = text(&bytes, Some("2")).expect("pdf");
        assert_eq!(document.pages.as_deref(), Some("2-2"));
        assert!(document.text.contains("Beta"));
        assert!(!document.text.contains("Alpha") && !document.text.contains("Gamma"));

        let rest = text(&bytes, Some("2-9")).expect("pdf");
        assert_eq!(rest.pages.as_deref(), Some("2-3"));
    }

    #[test]
    fn page_ranges_are_checked() {
        let bytes = pdf(&["Alpha", "Beta"]);
        for pages in ["0", "3", "2-1", "two", "1-"] {
            assert!(text(&bytes, Some(pages)).is_err(), "{pages}");
            assert!(read(&bytes, Some(pages), View::Pages).is_err(), "{pages}");
        }
    }

    #[test]
    fn a_pdf_without_text_points_to_the_page_view() {
        let error = text(&pdf(&[""]), None).expect_err("no text");
        assert!(error.to_string().contains("view `pages`"));
    }

    #[test]
    fn a_damaged_pdf_is_an_error_not_a_crash() {
        let damaged = b"%PDF-1.4\nthis is not a pdf";
        assert!(text(damaged, None).is_err());
        assert!(read(damaged, None, View::Pages).is_err());
    }

    #[test]
    fn pages_come_back_as_pictures_with_their_text() {
        let bytes = pdf(&["Alpha", "Beta", "Gamma"]);
        let document = read(&bytes, Some("2-3"), View::Pages).expect("pages");
        assert_eq!(document.page_count, Some(3));
        assert_eq!(document.pages.as_deref(), Some("2-3"));
        assert!(!document.truncated);
        assert!(document.text.contains("Beta") && document.text.contains("Gamma"));

        let numbers: Vec<_> = document.images.iter().map(|image| image.page).collect();
        assert_eq!(numbers, [Some(2), Some(3)]);
        for image in &document.images {
            assert_eq!(image.mime_type, "image/png");
            assert!(image.data.starts_with(&[0x89, b'P', b'N', b'G']));
        }
    }

    #[test]
    fn a_page_without_text_still_gives_its_picture() {
        let document = read(&pdf(&[""]), None, View::Pages).expect("pages");
        assert_eq!(document.images.len(), 1);
    }

    #[test]
    fn only_a_few_pictures_are_sent_at_once() {
        let lines = vec!["Slide"; MAX_PAGE_IMAGES + 2];
        let document = read(&pdf(&lines), None, View::Pages).expect("pages");
        assert_eq!(document.images.len(), MAX_PAGE_IMAGES);
        assert_eq!(document.pages, Some(format!("1-{MAX_PAGE_IMAGES}")));
        assert!(document.truncated);
    }
}
