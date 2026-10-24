//! The text of a PDF page by page, with raised and lowered characters
//! written as `x^2`, `a_i` and `1/n` instead of on lines of their own.

use pdf_extract::{Document, MediaBox, OutputDev, OutputError, Transform};

pub fn page_texts(bytes: &[u8]) -> Result<Vec<String>, OutputError> {
    let mut doc = Document::load_mem(bytes)?;
    if doc.is_encrypted() {
        doc.decrypt("")?;
    }
    let mut pages = Vec::new();
    for page_num in 1.. {
        let mut output = Lines::default();
        if pdf_extract::output_doc_page(&doc, &mut output, page_num).is_err() {
            break;
        }
        pages.push(output.text);
    }
    Ok(pages)
}

/// Characters set smaller and off the baseline, such as an exponent, an
/// index or one half of a small fraction.
struct Script {
    rise: f64,
    chars: String,
    start: f64,
    end: f64,
}

impl Script {
    fn raised(&self) -> bool {
        self.rise > 0.0
    }

    fn overlaps(&self, other: &Script) -> bool {
        other.start < self.end && self.start < other.end
    }
}

fn grouped(chars: &str) -> String {
    if chars.chars().count() == 1 {
        chars.to_owned()
    } else {
        format!("{{{chars}}}")
    }
}

fn parenthesised(chars: &str) -> String {
    let chars = chars.trim();
    if chars.chars().count() == 1 || chars.chars().all(|c| c.is_ascii_digit()) {
        chars.to_owned()
    } else {
        format!("({chars})")
    }
}

#[derive(Default)]
struct Lines {
    text: String,
    page_height: f64,
    /// Baseline and size of the normal text on the current line.
    line: Option<(f64, f64)>,
    last_end: f64,
    word_start: bool,
    scripts: Vec<Script>,
}

impl Lines {
    fn rise_of(&self, y: f64, size: f64) -> Option<f64> {
        let (base_y, base_size) = self.line?;
        let rise = base_y - y;
        let off_baseline = rise > base_size * 0.15 || rise < -base_size * 0.1;
        (size < base_size * 0.9 && rise.abs() <= base_size * 0.8 && off_baseline).then_some(rise)
    }

    fn add_script(&mut self, x: f64, end: f64, rise: f64, char: &str) -> bool {
        let Some((_, base_size)) = self.line else {
            return false;
        };
        let near = base_size * 0.5;
        if let Some(last) = self.scripts.last_mut()
            && (last.rise - rise).abs() < base_size * 0.1
            && x >= last.end - near
        {
            last.chars.push_str(char);
            last.end = end;
            return true;
        }
        let stacked = self
            .scripts
            .last()
            .is_some_and(|last| x >= last.start - near && x < last.end);
        if !stacked && x < self.last_end - near {
            return false;
        }
        if self.scripts.is_empty()
            && x > self.last_end + base_size * 0.2
            && !self.text.ends_with(char::is_whitespace)
        {
            self.text.push(' ');
        }
        self.scripts.push(Script {
            rise,
            chars: char.to_owned(),
            start: x,
            end,
        });
        true
    }

    /// A raised part over a deeply lowered one is a fraction, while an index
    /// under an exponent sits only a little below the line.
    fn close_scripts(&mut self) {
        let base_size = self.line.map_or(0.0, |(_, size)| size);
        let mut scripts = std::mem::take(&mut self.scripts).into_iter().peekable();
        while let Some(script) = scripts.next() {
            let other = scripts.next_if(|next| {
                next.raised() != script.raised()
                    && script.overlaps(next)
                    && script.rise.min(next.rise) < -base_size * 0.3
            });
            let written = match other {
                Some(other) => {
                    let (over, under) = if script.raised() {
                        (script, other)
                    } else {
                        (other, script)
                    };
                    format!(
                        "{}/{}",
                        parenthesised(&over.chars),
                        parenthesised(&under.chars)
                    )
                }
                None => {
                    let mark = if script.raised() { '^' } else { '_' };
                    format!("{mark}{}", grouped(&script.chars))
                }
            };
            self.text.push_str(&written);
        }
    }
}

impl OutputDev for Lines {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.page_height = media_box.ury - media_box.lly;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.close_scripts();
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let size = ((font_size * (trm.m11 + trm.m21)) * (font_size * (trm.m12 + trm.m22)))
            .abs()
            .sqrt();
        let (x, y) = (trm.m31, self.page_height - trm.m32);
        let end = x + width * size;
        let word_start = std::mem::take(&mut self.word_start);

        if let Some(rise) = self.rise_of(y, size)
            && self.add_script(x, end, rise, char)
        {
            self.last_end = self.last_end.max(end);
            return Ok(());
        }
        self.close_scripts();

        match self.line {
            Some((base_y, base_size)) if word_start => {
                let drop = (y - base_y).abs();
                let gap = drop > size * 1.5;
                let back = x < self.last_end && drop > size * 0.5;
                if gap || back {
                    self.text.push_str(if gap && back { "\n\n" } else { "\n" });
                    self.line = Some((y, size));
                } else {
                    if x > self.last_end + size * 0.1 {
                        self.text.push(' ');
                    }
                    if size > base_size {
                        self.line = Some((y, size));
                    }
                }
            }
            Some(_) => {}
            None => self.line = Some((y, size)),
        }
        self.text.push_str(char);
        self.last_end = end;
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.word_start = true;
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}
