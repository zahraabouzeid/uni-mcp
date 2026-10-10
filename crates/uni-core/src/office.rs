//! Text from Word, PowerPoint and Excel files, which are zip archives of XML.

use std::io::{Cursor, Read};

use roxmltree::{Document as Xml, Node};
use zip::ZipArchive;

use crate::{Error, Result};

/// Most one unpacked part may hold, against archives built to exhaust memory.
const MAX_PART_BYTES: u64 = 30 * 1024 * 1024;

const SLIDES: (&str, &str) = ("ppt/slides/slide", ".xml");
const SHEETS: (&str, &str) = ("xl/worksheets/sheet", ".xml");

pub enum Office {
    Text(String),
    Parts {
        unit: &'static str,
        parts: Vec<String>,
    },
}

type Archive<'a> = ZipArchive<Cursor<&'a [u8]>>;

/// `None` if the archive is a plain zip and not an Office file.
pub fn read(bytes: &[u8]) -> Result<Option<Office>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(unreadable)?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|name| Some(name.ok()?.into_owned()))
        .collect();
    let has = |name: &str| names.iter().any(|n| n == name);

    if has("word/document.xml") {
        let xml = part(&mut archive, "word/document.xml")?;
        return Ok(Some(Office::Text(paragraphs(&xml)?)));
    }
    let slides = numbered(&names, SLIDES);
    if !slides.is_empty() {
        let parts = slides
            .iter()
            .map(|name| paragraphs(&part(&mut archive, name)?))
            .collect::<Result<_>>()?;
        return Ok(Some(Office::Parts {
            unit: "slide",
            parts,
        }));
    }
    if has("xl/workbook.xml") {
        return Ok(Some(Office::Parts {
            unit: "sheet",
            parts: sheets(&mut archive, &names)?,
        }));
    }
    Ok(None)
}

fn unreadable(why: impl std::fmt::Display) -> Error {
    Error::InvalidInput(format!("could not read this Office file: {why}"))
}

fn part(archive: &mut Archive, name: &str) -> Result<String> {
    let file = archive.by_name(name).map_err(unreadable)?;
    let mut text = String::new();
    file.take(MAX_PART_BYTES)
        .read_to_string(&mut text)
        .map_err(unreadable)?;
    Ok(text)
}

fn numbered(names: &[String], (prefix, suffix): (&str, &str)) -> Vec<String> {
    let mut found: Vec<(u32, &String)> = names
        .iter()
        .filter_map(|name| {
            let number = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
            Some((number.parse().ok()?, name))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, name)| name.clone()).collect()
}

fn is(node: Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn paragraphs(xml: &str) -> Result<String> {
    let doc = Xml::parse(xml).map_err(unreadable)?;
    let lines: Vec<String> = doc
        .descendants()
        .filter(|node| is(*node, "p"))
        .map(|paragraph| {
            let mut line = String::new();
            runs(paragraph, &mut line);
            line.trim().to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect();
    Ok(lines.join("\n"))
}

fn runs(node: Node, line: &mut String) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            // A text box inside a paragraph has paragraphs of its own.
            "p" => {}
            "t" => line.push_str(child.text().unwrap_or_default()),
            "tab" => line.push('\t'),
            "br" => line.push('\n'),
            _ => runs(child, line),
        }
    }
}

fn sheets(archive: &mut Archive, names: &[String]) -> Result<Vec<String>> {
    let strings = match names.iter().any(|n| n == "xl/sharedStrings.xml") {
        true => shared_strings(&part(archive, "xl/sharedStrings.xml")?)?,
        false => Vec::new(),
    };
    let workbook = part(archive, "xl/workbook.xml")?;
    let workbook = Xml::parse(&workbook).map_err(unreadable)?;
    let titles: Vec<&str> = workbook
        .descendants()
        .filter(|node| is(*node, "sheet"))
        .filter_map(|sheet| sheet.attribute("name"))
        .collect();

    numbered(names, SHEETS)
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let rows = rows(&part(archive, name)?, &strings)?;
            Ok(match titles.get(index) {
                Some(title) => format!("{title}\n{rows}"),
                None => rows,
            })
        })
        .collect()
}

fn shared_strings(xml: &str) -> Result<Vec<String>> {
    let doc = Xml::parse(xml).map_err(unreadable)?;
    Ok(doc
        .descendants()
        .filter(|node| is(*node, "si"))
        .map(text_of)
        .collect())
}

fn text_of(node: Node) -> String {
    node.descendants()
        .filter(|n| is(*n, "t"))
        .filter_map(|n| n.text())
        .collect()
}

fn rows(xml: &str, strings: &[String]) -> Result<String> {
    let doc = Xml::parse(xml).map_err(unreadable)?;
    let rows: Vec<String> = doc
        .descendants()
        .filter(|node| is(*node, "row"))
        .map(|row| {
            let cells: Vec<String> = row
                .children()
                .filter(|node| is(*node, "c"))
                .map(|cell| cell_value(cell, strings))
                .collect();
            cells.join("\t").trim_end().to_owned()
        })
        .filter(|row| !row.is_empty())
        .collect();
    Ok(rows.join("\n"))
}

fn cell_value(cell: Node, strings: &[String]) -> String {
    let value = cell
        .children()
        .find(|node| is(*node, "v"))
        .and_then(|node| node.text())
        .unwrap_or_default();
    match cell.attribute("t") {
        // The value is an index into the strings shared by the workbook.
        Some("s") => value
            .parse::<usize>()
            .ok()
            .and_then(|index| strings.get(index))
            .cloned()
            .unwrap_or_default(),
        Some("inlineStr") => text_of(cell),
        _ => value.to_owned(),
    }
}

#[cfg(test)]
pub mod testing {
    use std::io::{Cursor, Write};

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    pub fn archive(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in parts {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("start");
            writer.write_all(content.as_bytes()).expect("write");
        }
        writer.finish().expect("finish").into_inner()
    }

    pub fn slide(lines: &[&str]) -> String {
        let paragraphs: String = lines
            .iter()
            .map(|line| format!("<a:p><a:r><a:t>{line}</a:t></a:r></a:p>"))
            .collect();
        format!(
            r#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a"><p:cSld><p:spTree><p:sp><p:txBody>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{archive, slide};
    use super::*;

    fn text(office: Option<Office>) -> String {
        match office {
            Some(Office::Text(text)) => text,
            _ => panic!("expected a text"),
        }
    }

    fn parts(office: Option<Office>) -> (&'static str, Vec<String>) {
        match office {
            Some(Office::Parts { unit, parts }) => (unit, parts),
            _ => panic!("expected parts"),
        }
    }

    #[test]
    fn a_word_document_is_read_paragraph_by_paragraph() {
        let document = r#"<w:document xmlns:w="urn:w"><w:body>
            <w:p><w:r><w:t>Exercise </w:t></w:r><w:r><w:t>sheet 1</w:t></w:r></w:p>
            <w:p/>
            <w:p><w:r><w:t>a)</w:t><w:tab/><w:t>Solve</w:t><w:br/><w:t>the equation</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
        let read = read(&archive(&[("word/document.xml", document)])).expect("docx");
        assert_eq!(
            text(read),
            "Exercise sheet 1\na)\tSolve\nthe equation\nCell"
        );
    }

    #[test]
    fn a_text_box_is_not_read_twice() {
        let document = r#"<w:document xmlns:w="urn:w"><w:body>
            <w:p><w:r><w:t>Outer</w:t></w:r><w:r><w:pict><w:txbxContent>
                <w:p><w:r><w:t>Inner</w:t></w:r></w:p>
            </w:txbxContent></w:pict></w:r></w:p>
        </w:body></w:document>"#;
        let read = read(&archive(&[("word/document.xml", document)])).expect("docx");
        assert_eq!(text(read), "Outer\nInner");
    }

    #[test]
    fn slides_come_in_the_order_of_their_numbers() {
        let read = read(&archive(&[
            ("ppt/slides/slide10.xml", &slide(&["Summary"])),
            (
                "ppt/slides/slide2.xml",
                &slide(&["Forces", "Newton's laws"]),
            ),
            ("ppt/slides/slide1.xml", &slide(&["Mechanics I"])),
            ("ppt/slides/_rels/slide1.xml.rels", "<Relationships/>"),
        ]))
        .expect("pptx");
        let (unit, slides) = parts(read);
        assert_eq!(unit, "slide");
        assert_eq!(slides, ["Mechanics I", "Forces\nNewton's laws", "Summary"]);
    }

    #[test]
    fn a_workbook_is_read_sheet_by_sheet() {
        let workbook = r#"<workbook xmlns="urn:x"><sheets>
            <sheet name="Grades" sheetId="1"/><sheet name="Notes" sheetId="2"/>
        </sheets></workbook>"#;
        let strings = r#"<sst xmlns="urn:x">
            <si><t>Course</t></si><si><r><t>Mecha</t></r><r><t>nics</t></r></si>
        </sst>"#;
        let grades = r#"<worksheet xmlns="urn:x"><sheetData>
            <row><c t="s"><v>0</v></c><c t="str"><v>Grade</v></c></row>
            <row><c t="s"><v>1</v></c><c><v>1.3</v></c></row>
            <row/>
        </sheetData></worksheet>"#;
        let notes = r#"<worksheet xmlns="urn:x"><sheetData>
            <row><c t="inlineStr"><is><t>Retake in March</t></is></c></row>
        </sheetData></worksheet>"#;
        let read = read(&archive(&[
            ("xl/workbook.xml", workbook),
            ("xl/sharedStrings.xml", strings),
            ("xl/worksheets/sheet1.xml", grades),
            ("xl/worksheets/sheet2.xml", notes),
        ]))
        .expect("xlsx");
        let (unit, sheets) = parts(read);
        assert_eq!(unit, "sheet");
        assert_eq!(
            sheets,
            [
                "Grades\nCourse\tGrade\nMechanics\t1.3",
                "Notes\nRetake in March"
            ]
        );
    }

    #[test]
    fn other_archives_and_damaged_files_are_refused() {
        assert!(matches!(
            read(&archive(&[("readme.txt", "hello")])),
            Ok(None)
        ));
        assert!(read(b"PK\x03\x04 not really a zip").is_err());
        let broken = archive(&[("word/document.xml", "<w:document")]);
        assert!(read(&broken).is_err());
    }
}
