//! Turns a downloaded file into text and pictures the model can take in.

use std::ops::RangeInclusive;
use std::panic::{self, AssertUnwindSafe};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use crate::model::{Document, Image};
use crate::office::{self, Office, Part};
use crate::{Error, Result, archive};

pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 200 * 1024;
/// Each page picture costs the model several times a page of text.
pub const MAX_PAGE_IMAGES: usize = 5;
/// AI apps refuse pictures much larger than this.
pub const MAX_PICTURE_BYTES: usize = 4 * 1024 * 1024;
const PAGE_IMAGE_SIDE: f32 = 1400.0;
const ZIP: &[u8] = b"PK\x03\x04";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Text,
    Pages,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Request<'a> {
    /// As `3` or `2-5`, counted from one.
    pub pages: Option<&'a str>,
    pub view: View,
    pub entry: Option<&'a str>,
}

pub fn check_size(bytes: u64) -> Result<()> {
    if bytes > MAX_FILE_BYTES as u64 {
        return Err(Error::InvalidInput(format!(
            "the file is larger than {} MB",
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

pub fn read(bytes: &[u8], request: Request) -> Result<Document> {
    check_size(bytes.len() as u64)?;
    let Request { pages, view, entry } = request;
    if let Some(entry) = entry {
        if !bytes.starts_with(ZIP) {
            return Err(Error::InvalidInput(
                "`entry` is only for zip archives".into(),
            ));
        }
        let inner = Request {
            entry: None,
            ..request
        };
        return read(&archive::entry(bytes, entry)?, inner);
    }
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
    if bytes.starts_with(ZIP) {
        return match (office::read(bytes)?, view) {
            (Some(Office::Text(body)), View::Text) => Ok(plain(&body.text)),
            (Some(Office::Text(body)), View::Pages) => body_pictures(bytes, &body, pages),
            (Some(Office::Parts { unit, parts }), View::Text) => {
                let wanted = wanted(pages, parts.len())?;
                let texts: Vec<String> = parts.into_iter().map(|part| part.text).collect();
                Ok(numbered(unit, &texts, wanted))
            }
            (Some(Office::Parts { unit, parts }), View::Pages) => {
                let wanted = wanted(pages, parts.len())?;
                part_pictures(bytes, unit, &parts, wanted)
            }
            (None, _) => Ok(plain(&archive::list(bytes)?)),
        };
    }
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        return Err(Error::InvalidInput(
            "old Office files (.doc, .ppt, .xls) cannot be read, only .docx, .pptx and .xlsx"
                .into(),
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        Error::InvalidInput("only text, PDF, picture and Office files can be read".into())
    })?;
    Ok(plain(text))
}

fn plain(text: &str) -> Document {
    let kept = fitting(text);
    Document {
        text: kept.to_owned(),
        page_count: None,
        pages: None,
        truncated: kept.len() < text.len(),
        images: Vec::new(),
    }
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

/// The pictures an AI app can take in, and how many had to be left out.
fn showable(bytes: &[u8], names: &[String], page: Option<usize>) -> Result<(Vec<Image>, usize)> {
    let mut images = Vec::new();
    for name in names {
        let Some(data) = office::picture(bytes, name)? else {
            continue;
        };
        if let Some(mime_type) = image_type(&data) {
            images.push(Image {
                mime_type,
                data,
                page,
            });
        }
    }
    let left_out = names.len() - images.len();
    Ok((images, left_out))
}

/// Slides with their text and the pictures placed on them, for as many
/// slides as fit into the picture limit.
fn part_pictures(
    bytes: &[u8],
    unit: &str,
    parts: &[Part],
    wanted: RangeInclusive<usize>,
) -> Result<Document> {
    let mut text = String::new();
    let mut images = Vec::new();
    let mut last = None;
    for number in wanted.clone() {
        let part = &parts[number - 1];
        let (mut shown, mut left_out) = showable(bytes, &part.pictures, Some(number))?;
        if last.is_some() && images.len() + shown.len() > MAX_PAGE_IMAGES {
            break;
        }
        if shown.len() > MAX_PAGE_IMAGES {
            left_out += shown.len() - MAX_PAGE_IMAGES;
            shown.truncate(MAX_PAGE_IMAGES);
        }
        let mut content = part.text.clone();
        if left_out > 0 {
            content.push_str(&format!(
                "\n(pictures on this {unit} that cannot be shown: {left_out})"
            ));
        }
        text.push_str(&marked(unit, number, &content));
        images.append(&mut shown);
        last = Some(number);
    }
    let kept = fitting(&text);
    Ok(Document {
        truncated: kept.len() < text.len() || last.is_some_and(|last| last < *wanted.end()),
        text: kept.to_owned(),
        page_count: Some(parts.len()),
        pages: last.map(|last| format!("{}-{last}", wanted.start())),
        images,
    })
}

/// A Word file has no pages to pick from, so `pages` picks its pictures.
fn body_pictures(bytes: &[u8], body: &Part, pages: Option<&str>) -> Result<Document> {
    let count = body.pictures.len();
    if count == 0 {
        return Ok(plain(&body.text));
    }
    let wanted = match pages {
        Some(pages) => parse_pages(pages, count)?,
        None => 1..=count.min(MAX_PAGE_IMAGES),
    };
    if wanted.clone().count() > MAX_PAGE_IMAGES {
        return Err(Error::InvalidInput(format!(
            "the file has {count} pictures and view `pages` shows at most {MAX_PAGE_IMAGES} at a time; ask for a range such as {}-{}",
            wanted.start(),
            wanted.start() + MAX_PAGE_IMAGES - 1
        )));
    }
    let (images, left_out) = showable(
        bytes,
        &body.pictures[wanted.start() - 1..*wanted.end()],
        None,
    )?;
    let mut note = format!(
        "[pictures {}-{} of {count}; `pages` picks others",
        wanted.start(),
        wanted.end()
    );
    if left_out > 0 {
        note.push_str(&format!("; {left_out} of these cannot be shown"));
    }
    Ok(Document {
        images,
        ..plain(&format!("{note}]\n{}", body.text))
    })
}

fn unreadable(why: impl std::fmt::Display) -> Error {
    Error::InvalidInput(format!("could not read this PDF: {why}"))
}

/// The PDF libraries panic on some damaged files.
fn page_texts(bytes: &[u8]) -> Result<Vec<String>> {
    panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(bytes))
        .map_err(|_| unreadable("it is damaged or uses an unsupported feature"))?
        .map_err(unreadable)
}

fn pdf_text(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
    let all = page_texts(bytes)?;
    let wanted = wanted(pages, all.len())?;
    if all[wanted.start() - 1..*wanted.end()]
        .iter()
        .all(|page| page.trim().is_empty())
    {
        return Err(Error::InvalidInput(
            "these pages contain no text, they are probably scans; ask for them with view `pages`"
                .into(),
        ));
    }
    Ok(numbered("page", &all, wanted))
}

fn wanted(pages: Option<&str>, count: usize) -> Result<RangeInclusive<usize>> {
    match pages {
        Some(pages) => parse_pages(pages, count),
        None => Ok(1..=count),
    }
}

fn numbered(unit: &str, all: &[String], wanted: RangeInclusive<usize>) -> Document {
    let mut text = String::new();
    let mut last = None;
    for number in wanted.clone() {
        let page = marked(unit, number, &all[number - 1]);
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
    let cut_short = last.is_some_and(|last| {
        last < *wanted.end() || marked(unit, last, &all[last - 1]).len() > MAX_TEXT_BYTES
    });

    Document {
        truncated: cut_short,
        text,
        page_count: Some(all.len()),
        pages: last.map(|last| format!("{}-{last}", wanted.start())),
        images: Vec::new(),
    }
}

/// Pictures only: the text of a page with formulas or figures on it would
/// contradict what the picture shows.
fn pdf_pages(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
    panic::catch_unwind(AssertUnwindSafe(|| render(bytes, pages)))
        .map_err(|_| unreadable("it is damaged or uses an unsupported feature"))?
}

fn render(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
    let pdf = Pdf::new(bytes.to_vec()).map_err(|_| unreadable("it is not a valid PDF"))?;
    let all = pdf.pages();
    let wanted = wanted(pages, all.len())?;
    if wanted.clone().count() > MAX_PAGE_IMAGES {
        return Err(Error::InvalidInput(format!(
            "the file has {} pages and view `pages` shows at most {MAX_PAGE_IMAGES} at a time; ask for a range such as {}-{}",
            all.len(),
            wanted.start(),
            wanted.start() + MAX_PAGE_IMAGES - 1
        )));
    }

    let cache = RenderCache::new();
    let mut images = Vec::new();
    for number in wanted.clone() {
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
    Ok(Document {
        text: String::new(),
        page_count: Some(all.len()),
        pages: Some(format!("{}-{}", wanted.start(), wanted.end())),
        truncated: false,
        images,
    })
}

fn marked(unit: &str, number: usize, text: &str) -> String {
    format!("[{unit} {number}]\n{}\n\n", text.trim())
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
    use crate::office::testing::{PNG, archive, archive_of, placing, relations, slide};

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
        let request = Request {
            pages,
            ..Request::default()
        };
        read(bytes, request)
    }

    fn pictures(bytes: &[u8], pages: Option<&str>) -> Result<Document> {
        let request = Request {
            pages,
            view: View::Pages,
            entry: None,
        };
        read(bytes, request)
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
            assert!(pictures(&bytes, Some(pages)).is_err(), "{pages}");
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
        assert!(pictures(damaged, None).is_err());
    }

    #[test]
    fn pages_come_back_as_pictures_without_text() {
        let bytes = pdf(&["Alpha", "Beta", "Gamma"]);
        let document = pictures(&bytes, Some("2-3")).expect("pages");
        assert_eq!(document.page_count, Some(3));
        assert_eq!(document.pages.as_deref(), Some("2-3"));
        assert!(!document.truncated);
        assert_eq!(document.text, "");

        let numbers: Vec<_> = document.images.iter().map(|image| image.page).collect();
        assert_eq!(numbers, [Some(2), Some(3)]);
        for image in &document.images {
            assert_eq!(image.mime_type, "image/png");
            assert!(image.data.starts_with(&[0x89, b'P', b'N', b'G']));
        }
    }

    #[test]
    fn a_page_without_text_still_gives_its_picture() {
        let document = pictures(&pdf(&[""]), None).expect("pages");
        assert_eq!(document.images.len(), 1);
    }

    #[test]
    fn more_pictures_than_allowed_are_refused_with_a_range_to_ask_for() {
        let bytes = pdf(&["Slide"; MAX_PAGE_IMAGES + 2]);
        let error = pictures(&bytes, None).expect_err("too many");
        assert!(error.to_string().contains("7 pages"), "{error}");
        assert!(error.to_string().contains("1-5"), "{error}");

        let later = pictures(&bytes, Some("2-7")).expect_err("too many");
        assert!(later.to_string().contains("2-6"), "{later}");

        let allowed = pictures(&bytes, Some("3-7")).expect("five pages");
        assert_eq!(allowed.images.len(), MAX_PAGE_IMAGES);
        assert!(!allowed.truncated);
    }

    #[test]
    fn slides_are_numbered_and_can_be_picked_like_pages() {
        let presentation = archive(&[
            ("ppt/slides/slide1.xml", &slide(&["Mechanics I"])),
            ("ppt/slides/slide2.xml", &slide(&["Forces"])),
            ("ppt/slides/slide3.xml", &slide(&["Summary"])),
        ]);
        let all = text(&presentation, None).expect("pptx");
        assert_eq!(all.page_count, Some(3));
        assert_eq!(all.pages.as_deref(), Some("1-3"));
        assert!(all.text.contains("[slide 1]\nMechanics I"));
        assert!(all.text.contains("[slide 3]\nSummary"));

        let one = text(&presentation, Some("2")).expect("pptx");
        assert_eq!(one.pages.as_deref(), Some("2-2"));
        assert!(one.text.contains("Forces") && !one.text.contains("Summary"));
        assert!(text(&presentation, Some("4")).is_err());
    }

    /// A presentation whose slides each place the given number of pictures.
    fn presentation(pictures_per_slide: &[usize]) -> Vec<u8> {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        for (index, count) in pictures_per_slide.iter().enumerate() {
            let number = index + 1;
            let ids: Vec<String> = (1..=*count).map(|n| format!("rId{n}")).collect();
            let targets: Vec<String> = (1..=*count)
                .map(|n| format!("../media/s{number}p{n}.png"))
                .collect();
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            let pairs: Vec<(&str, &str)> = ids
                .iter()
                .copied()
                .zip(targets.iter().map(String::as_str))
                .collect();
            files.push((
                format!("ppt/slides/slide{number}.xml"),
                placing("p:sld", &format!("Slide {number}"), &ids).into_bytes(),
            ));
            files.push((
                format!("ppt/slides/_rels/slide{number}.xml.rels"),
                relations(&pairs).into_bytes(),
            ));
            for n in 1..=*count {
                files.push((format!("ppt/media/s{number}p{n}.png"), PNG.to_vec()));
            }
        }
        let files: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, content)| (name.as_str(), content.as_slice()))
            .collect();
        archive_of(&files)
    }

    #[test]
    fn slides_come_with_their_text_and_their_pictures() {
        let document = pictures(&presentation(&[1, 0, 2]), None).expect("pptx");
        assert_eq!(document.page_count, Some(3));
        assert_eq!(document.pages.as_deref(), Some("1-3"));
        assert!(!document.truncated);
        assert!(document.text.contains("[slide 2]\nSlide 2"));
        let slides: Vec<_> = document.images.iter().map(|image| image.page).collect();
        assert_eq!(slides, [Some(1), Some(3), Some(3)]);
        assert_eq!(document.images[0].mime_type, "image/png");
    }

    #[test]
    fn slides_stop_where_the_picture_limit_is_reached() {
        let bytes = presentation(&[3, 2, 1]);
        let first = pictures(&bytes, None).expect("pptx");
        assert_eq!(first.pages.as_deref(), Some("1-2"));
        assert!(first.truncated);
        assert_eq!(first.images.len(), MAX_PAGE_IMAGES);
        assert!(!first.text.contains("[slide 3]"));

        let rest = pictures(&bytes, Some("3")).expect("pptx");
        assert_eq!(rest.pages.as_deref(), Some("3-3"));
        assert!(!rest.truncated);
        assert_eq!(rest.images.len(), 1);
    }

    #[test]
    fn pictures_that_cannot_be_shown_are_counted() {
        let crowded = pictures(&presentation(&[MAX_PAGE_IMAGES + 2]), None).expect("pptx");
        assert_eq!(crowded.images.len(), MAX_PAGE_IMAGES);
        assert!(crowded.text.contains("cannot be shown: 2"));

        let slide = placing("p:sld", "Vector drawing", &["rId1"]);
        let relations = relations(&[("rId1", "../media/drawing.emf")]);
        let bytes = archive_of(&[
            ("ppt/slides/slide1.xml", slide.as_bytes()),
            ("ppt/slides/_rels/slide1.xml.rels", relations.as_bytes()),
            ("ppt/media/drawing.emf", &[1, 0, 0, 0]),
        ]);
        let unknown = pictures(&bytes, None).expect("pptx");
        assert!(unknown.images.is_empty());
        assert!(unknown.text.contains("cannot be shown: 1"));
    }

    #[test]
    fn a_word_document_hands_over_its_pictures_a_few_at_a_time() {
        let ids: Vec<String> = (1..=7).map(|n| format!("rId{n}")).collect();
        let targets: Vec<String> = (1..=7).map(|n| format!("media/image{n}.png")).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let pairs: Vec<(&str, &str)> = ids
            .iter()
            .copied()
            .zip(targets.iter().map(String::as_str))
            .collect();
        let body = placing("w:document", "Lab report", &ids);
        let relations = relations(&pairs);
        let mut files: Vec<(String, Vec<u8>)> = vec![
            ("word/document.xml".into(), body.into_bytes()),
            (
                "word/_rels/document.xml.rels".into(),
                relations.into_bytes(),
            ),
        ];
        for target in &targets {
            files.push((format!("word/{target}"), PNG.to_vec()));
        }
        let files: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, content)| (name.as_str(), content.as_slice()))
            .collect();
        let bytes = archive_of(&files);

        let first = pictures(&bytes, None).expect("docx");
        assert_eq!(first.images.len(), MAX_PAGE_IMAGES);
        assert!(first.text.starts_with("[pictures 1-5 of 7"));
        assert!(first.text.ends_with("Lab report"));

        let rest = pictures(&bytes, Some("6-7")).expect("docx");
        assert_eq!(rest.images.len(), 2);
        assert!(pictures(&bytes, Some("1-7")).is_err());
        assert!(text(&bytes, None).expect("docx").images.is_empty());
    }

    #[test]
    fn an_office_file_without_pictures_is_just_its_text() {
        let xml = r#"<w:document xmlns:w="urn:w"><w:body>
            <w:p><w:r><w:t>Exercise sheet 1</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let document = pictures(&archive(&[("word/document.xml", xml)]), None).expect("docx");
        assert_eq!(document.text, "Exercise sheet 1");
        assert!(document.images.is_empty());
    }

    #[test]
    fn a_word_document_is_text_without_pages() {
        let xml = r#"<w:document xmlns:w="urn:w"><w:body>
            <w:p><w:r><w:t>Exercise sheet 1</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let document = text(&archive(&[("word/document.xml", xml)]), None).expect("docx");
        assert_eq!(document.text, "Exercise sheet 1");
        assert_eq!(document.page_count, None);
    }

    #[test]
    fn a_zip_is_listed_and_one_file_can_be_read_out_of_it() {
        let zip = archive(&[
            ("sheet1/task.txt", "Solve the equation."),
            ("slides.pptx", "not opened here"),
        ]);
        let listing = text(&zip, None).expect("zip");
        assert!(listing.text.contains("sheet1/task.txt"));
        assert!(listing.text.contains("entry"));

        let entry = Request {
            entry: Some("sheet1/task.txt"),
            ..Request::default()
        };
        assert_eq!(
            read(&zip, entry).expect("entry").text,
            "Solve the equation."
        );

        let missing = Request {
            entry: Some("nope.txt"),
            ..Request::default()
        };
        assert!(read(&zip, missing).is_err());
        assert!(read(b"plain text", entry).is_err());
    }

    #[test]
    fn a_file_inside_a_zip_is_read_like_any_other() {
        let inner = pdf(&["Alpha", "Beta"]);
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("slides.pdf", zip::write::SimpleFileOptions::default())
            .expect("start");
        std::io::Write::write_all(&mut writer, &inner).expect("write");
        let zip = writer.finish().expect("finish").into_inner();

        let request = Request {
            pages: Some("2"),
            view: View::Pages,
            entry: Some("slides.pdf"),
        };
        let document = read(&zip, request).expect("pdf in zip");
        assert_eq!(document.page_count, Some(2));
        assert_eq!(document.images.len(), 1);
        assert_eq!(document.images[0].page, Some(2));
    }

    #[test]
    fn old_office_files_say_which_formats_work() {
        let error = text(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1], None).expect_err("old format");
        assert!(error.to_string().contains(".docx"));
    }
}
