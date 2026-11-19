//! The source widget: one egui `TextEdit` coloured by the shared highlighter,
//! with a line-number gutter and the parser's marks drawn over it. egui still
//! owns editing, selection, undo, clipboard and Tab; marks are presentation.

use std::ops::Range;

use bevy_egui::egui::{
    self, Align, Color32, FontId, Galley, Margin, Pos2, Rect, Sense, Stroke, TextBuffer,
    text::{CCursor, LayoutJob, TextFormat},
    text_edit::{TextEditOutput, TextEditState},
    text_selection::CCursorRange,
};

use super::super::syntax::{self, Kind, Symbols};
use crate::script::ParseError;

pub(super) const FONT_SIZE: f32 = 14.0;
const ROWS: usize = 14;
/// Pixels between the gutter's rule and the start of the text.
const TEXT_GAP: i8 = 3;
const PROBLEM_ROWS: usize = 4;
pub(super) const BACKGROUND: Color32 = Color32::from_rgb(15, 20, 25);
const GUTTER_INK: Color32 = Color32::from_rgb(92, 103, 115);
const GUTTER_RULE: Color32 = Color32::from_rgb(52, 60, 68);
pub(super) const ERROR: Color32 = Color32::from_rgb(255, 105, 100);
const BREAKPOINT: Color32 = Color32::from_rgb(229, 83, 75);
/// A dot on a line that holds no statement now: kept, but not used.
const BREAKPOINT_UNUSED: Color32 = Color32::from_rgb(110, 116, 124);

pub(super) fn id() -> egui::Id {
    egui::Id::new("eqscript-source")
}

fn font() -> FontId {
    FontId::monospace(FONT_SIZE)
}

fn color(kind: Kind) -> Color32 {
    let [red, green, blue] = kind.rgb();
    Color32::from_rgb(red, green, blue)
}

/// A parser error placed in the buffer. egui cursors count characters, the
/// parser counts bytes; `marks` is the one place that converts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Mark {
    pub line: Option<usize>,
    pub chars: Option<Range<usize>>,
    pub message: String,
}

/// Place each error on the characters its line and byte span name, clamped
/// to that line's text and to character boundaries. Lines split as the
/// parser splits them (`str::lines`): at `\n`, dropping a `\r` before it.
pub(super) fn marks(text: &str, errors: &[ParseError]) -> Vec<Mark> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
    // (byte, characters before it), advanced through sorted lines.
    let mut counted = (0, 0);
    let mut chars_before = |byte: usize| {
        if byte < counted.0 {
            counted = (0, 0);
        }
        counted = (byte, counted.1 + text[counted.0..byte].chars().count());
        counted.1
    };
    errors
        .iter()
        .map(|error| {
            let chars = error.line.and_then(|line| {
                let start = *starts.get(line.checked_sub(1)?)?;
                let end = starts.get(line).map_or(text.len(), |next| next - 1);
                let content = &text[start..end];
                let content = if end < text.len() {
                    content.strip_suffix('\r').unwrap_or(content)
                } else {
                    content
                };
                let span = error.span.clone().unwrap_or_else(|| {
                    content.len() - content.trim_start().len()..content.trim_end().len()
                });
                let from = content.floor_char_boundary(span.start.min(content.len()));
                let to = content
                    .ceil_char_boundary(span.end.min(content.len()))
                    .max(from);
                let base = chars_before(start);
                Some(base + content[..from].chars().count()..base + content[..to].chars().count())
            });
            Mark {
                line: error.line,
                chars,
                message: error.message.clone(),
            }
        })
        .collect()
}

/// The character index where a one-based line starts.
fn line_start(text: &str, line: usize) -> usize {
    let byte = text
        .match_indices('\n')
        .nth(line.saturating_sub(2))
        .filter(|_| line > 1)
        .map_or(0, |(index, _)| index + 1);
    text[..byte].chars().count()
}

/// Colour one buffer; built only when its text changes.
pub(super) fn layout(text: &str) -> LayoutJob {
    let symbols = Symbols::scan(text);
    let mut job = LayoutJob::default();
    for line in text.split_inclusive('\n') {
        let (code, newline) = line.split_at(line.trim_end_matches(['\r', '\n']).len());
        for (token, kind) in syntax::tokens(code, &symbols) {
            let mut format = TextFormat::simple(font(), color(kind));
            format.italics = kind == Kind::Comment;
            job.append(token, 0.0, format);
        }
        if !newline.is_empty() {
            job.append(
                newline,
                0.0,
                TextFormat::simple(font(), color(Kind::Argument)),
            );
        }
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

/// What the widget keeps between frames.
#[derive(Default)]
pub(super) struct Memory {
    /// The coloured text and the job built from it.
    job: Option<(String, LayoutJob)>,
    /// A line chosen in the problems list, applied on the next frame.
    pub jump: Option<usize>,
}

pub(super) struct Shown {
    pub output: TextEditOutput,
    /// A problem was chosen this frame; the editor keeps the keyboard.
    pub chose: bool,
    /// The line whose number was clicked this frame, to toggle its dot.
    pub clicked: Option<usize>,
}

/// The breakpoint dots and the lines they can pause on, when the script is
/// valid.
pub(super) struct Dots<'a> {
    pub lines: &'a std::collections::BTreeSet<usize>,
    pub statements: Option<&'a std::collections::BTreeSet<usize>>,
}

/// Draw the editor and its problems list into `ui`.
pub(super) fn show(
    ui: &mut egui::Ui,
    text: &mut dyn TextBuffer,
    marks: &[Mark],
    memory: &mut Memory,
    dots: &Dots<'_>,
) -> Shown {
    let row = ui.fonts_mut(|fonts| fonts.row_height(&font()));
    let strip = if marks.is_empty() {
        0.0
    } else {
        f32::from(u8::try_from(marks.len().min(PROBLEM_ROWS)).unwrap_or(u8::MAX))
            .mul_add(row + 2.0, 8.0)
    };
    let digits = text.as_str().lines().count().max(ROWS).to_string().len();
    let glyph = ui.fonts_mut(|fonts| fonts.glyph_width(&font(), '0'));
    #[allow(
        clippy::cast_possible_truncation,
        reason = "bounded by the source size"
    )]
    let gutter = f32::from(u8::try_from(digits).unwrap_or(u8::MAX))
        .mul_add(glyph, 18.0)
        .ceil()
        .min(120.0) as i8;
    let margin = Margin {
        // A few pixels of air between the gutter's rule and the text.
        left: gutter + TEXT_GAP,
        right: 8,
        top: 2,
        bottom: 2,
    };
    let jump = memory.jump.take();
    let mut output = None;
    let mut clicked = None;
    egui::ScrollArea::both()
        .id_salt("eqscript-scroll")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - strip).max(row))
        .show_viewport(ui, |ui, viewport| {
            if let Some(line) = jump {
                let at = CCursor::new(line_start(text.as_str(), line));
                let mut state = TextEditState::load(ui.ctx(), id()).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::one(at)));
                state.store(ui.ctx(), id());
                ui.memory_mut(|memory| memory.request_focus(id()));
            }
            let job = &mut memory.job;
            let mut layouter = |ui: &egui::Ui, buffer: &dyn TextBuffer, _wrap: f32| {
                let source = buffer.as_str();
                if job.as_ref().is_none_or(|(text, _)| text != source) {
                    *job = Some((source.to_owned(), layout(source)));
                }
                let built = job.as_ref().map(|(_, job)| job.clone()).unwrap_or_default();
                ui.fonts_mut(|fonts| fonts.layout_job(built))
            };
            let shown = egui::TextEdit::multiline(text)
                .id(id())
                .font(font())
                .lock_focus(true)
                .desired_rows(ROWS)
                .desired_width(f32::INFINITY)
                // Fill the view, so the gutter rule reaches its bottom and a
                // click below the last line still lands in the editor.
                .min_size(egui::vec2(0.0, viewport.height()))
                // A custom frame replaces the margin, so the frame carries
                // the gutter; the margin still sizes the wrap width.
                .frame(egui::Frame::NONE.inner_margin(margin))
                .margin(margin)
                .layouter(&mut layouter)
                .show(ui);
            if let Some(line) = jump {
                let at = CCursor::new(line_start(text.as_str(), line));
                let rect = shown
                    .galley
                    .pos_from_cursor(at)
                    .translate(shown.galley_pos.to_vec2());
                ui.scroll_to_rect(rect, Some(Align::Center));
            }
            clicked = gutter_and_marks(ui, &shown, f32::from(gutter), glyph, marks, dots);
            output = Some(shown);
        });
    let chose = problems(ui, marks, memory);
    Shown {
        output: output.expect("the scroll area always shows its contents"),
        chose,
        clicked,
    }
}

/// Line numbers, breakpoint dots, error markers and wavy underlines, for
/// visible rows only. A click on a line number gives that line.
fn gutter_and_marks(
    ui: &egui::Ui,
    output: &TextEditOutput,
    gutter: f32,
    glyph: f32,
    marks: &[Mark],
    dots: &Dots<'_>,
) -> Option<usize> {
    let mut clicked = None;
    let clip = ui.clip_rect();
    let painter = ui.painter().with_clip_rect(clip);
    let left = output.response.rect.left();
    let galley: &Galley = &output.galley;
    let origin = output.galley_pos.to_vec2();
    // A thin rule between the line numbers and the text.
    let rule = left + gutter - 4.0;
    let span = output.response.rect.y_range();
    painter.vline(
        rule,
        span.min.max(clip.top())..=span.max.min(clip.bottom()),
        Stroke::new(1.0, GUTTER_RULE),
    );
    let mut line = 1;
    for placed in &galley.rows {
        let rect = placed.rect().translate(origin);
        if rect.bottom() >= clip.top() && rect.top() <= clip.bottom() {
            let errors = marks
                .iter()
                .filter(|mark| mark.line == Some(line))
                .collect::<Vec<_>>();
            let ink = if errors.is_empty() { GUTTER_INK } else { ERROR };
            painter.text(
                Pos2::new(left + gutter - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                line.to_string(),
                font(),
                ink,
            );
            let dot = Pos2::new(left + 6.0, rect.center().y);
            if dots.lines.contains(&line) {
                let used = dots
                    .statements
                    .is_none_or(|statements| statements.contains(&line));
                let ink = if used { BREAKPOINT } else { BREAKPOINT_UNUSED };
                painter.circle_filled(dot, 4.0, ink);
            } else if !errors.is_empty() {
                painter.circle_filled(dot, 2.5, ERROR);
            }
            let marker = Rect::from_x_y_ranges(left..=left + gutter - 4.0, rect.y_range());
            let response = ui.interact(marker, ui.id().with(("gutter", line)), Sense::click());
            if response.clicked() {
                clicked = Some(line);
            }
            if !errors.is_empty() {
                let text = errors
                    .iter()
                    .map(|mark| mark.message.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                response.on_hover_text(text);
            }
        }
        if placed.ends_with_newline {
            line += 1;
        }
    }
    for (index, mark) in marks.iter().enumerate() {
        let Some(chars) = &mark.chars else {
            continue;
        };
        let start = galley
            .pos_from_cursor(CCursor::new(chars.start))
            .translate(origin);
        if start.bottom() < clip.top() || start.top() > clip.bottom() {
            continue;
        }
        let end = galley
            .pos_from_cursor(CCursor::new(chars.end))
            .translate(origin);
        let right = if chars.is_empty() || (end.top() - start.top()).abs() > 0.5 {
            start.left() + glyph
        } else {
            end.left()
        };
        squiggle(
            &painter,
            start.left(),
            right.max(start.left() + glyph * 0.5),
            start.bottom(),
        );
        let area = Rect::from_x_y_ranges(start.left()..=right, start.y_range());
        ui.interact(area, ui.id().with(("mark", index)), Sense::hover())
            .on_hover_text(&mark.message);
    }
    clicked
}

fn squiggle(painter: &egui::Painter, left: f32, right: f32, bottom: f32) {
    const STEP: f32 = 2.5;
    let mut points = Vec::new();
    let mut x = left;
    let mut up = false;
    while x < right {
        points.push(Pos2::new(x, if up { bottom - 2.5 } else { bottom - 0.5 }));
        x += STEP;
        up = !up;
    }
    points.push(Pos2::new(
        right,
        if up { bottom - 2.5 } else { bottom - 0.5 },
    ));
    painter.add(egui::Shape::line(points, Stroke::new(1.2, ERROR)));
}

/// Every error by line, document-level ones too. Choosing a line jumps to it.
fn problems(ui: &mut egui::Ui, marks: &[Mark], memory: &mut Memory) -> bool {
    if marks.is_empty() {
        return false;
    }
    let mut chose = false;
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("eqscript-problems")
        .auto_shrink([false, true])
        .max_height(ui.available_height())
        .show(ui, |ui| {
            for mark in marks {
                let label = mark.line.map_or_else(
                    || format!("Script: {}", mark.message),
                    |line| format!("Line {line}: {}", mark.message),
                );
                let text = egui::RichText::new(label).font(font()).color(ERROR);
                let response = ui.add(egui::Label::new(text).sense(if mark.line.is_some() {
                    Sense::click()
                } else {
                    Sense::hover()
                }));
                if let Some(line) = mark.line
                    && response.clicked()
                {
                    memory.jump = Some(line);
                    chose = true;
                }
            }
        });
    chose
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(line: usize, span: Option<Range<usize>>) -> ParseError {
        ParseError {
            line: Some(line),
            span,
            message: "bad".into(),
        }
    }

    fn marked(text: &str, error: ParseError) -> String {
        let mark = marks(text, &[error]).remove(0);
        text.chars()
            .skip(mark.chars.as_ref().unwrap().start)
            .take(mark.chars.unwrap().len())
            .collect()
    }

    #[test]
    fn byte_spans_become_character_ranges_on_their_own_line() {
        let text = "report café\r\n\tpres é\nwait 1";
        // Bytes 7..13 of line 1 are "café" (é is two bytes).
        assert_eq!(marked(text, error(1, Some(7..12))), "café");
        assert_eq!(marked(text, error(2, Some(1..5))), "pres");
        assert_eq!(marked(text, error(2, Some(6..8))), "é");
        // Inside a character, past the line, or past the text: clamped.
        assert_eq!(marked(text, error(2, Some(7..400))), "é");
        assert_eq!(marked(text, error(1, Some(13..13))), "");
        assert_eq!(marks(text, &[error(9, Some(0..1))])[0].chars, None);
        // No span: the line's text without its indent.
        assert_eq!(marked(text, error(2, None)), "pres é");
        assert_eq!(marked("abc\r", error(1, Some(0..9))), "abc\r");
    }

    #[test]
    fn a_leading_bom_and_windows_newlines_line_up_with_the_parser() {
        let text = "\u{feff}pres(\"I\")\r\nx: int = nope\r\n";
        let errors = crate::script::Program::diagnostics(text, std::path::Path::new("."));
        let found = marks(text, &errors)
            .into_iter()
            .map(|mark| {
                text.chars()
                    .skip(mark.chars.as_ref().unwrap().start)
                    .take(mark.chars.unwrap().len())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(found, ["pres", "nope"]);
    }

    #[test]
    fn line_starts_count_characters() {
        let text = "é\nab\n\ncd";
        assert_eq!(line_start(text, 1), 0);
        assert_eq!(line_start(text, 2), 2);
        assert_eq!(line_start(text, 4), 6);
        assert_eq!(line_start(text, 9), 0);
    }

    #[test]
    fn the_text_starts_right_of_the_gutter() {
        let ctx = egui::Context::default();
        let mut text = String::from("wait(1)\npres(\"I\")\n");
        let mut memory = Memory::default();
        let mut placed = None;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let none = std::collections::BTreeSet::new();
                let shown = show(
                    ui,
                    &mut text,
                    &[],
                    &mut memory,
                    &Dots {
                        lines: &none,
                        statements: None,
                    },
                );
                placed = Some((shown.output.response.rect.left(), shown.output.galley_pos.x));
            });
            output.textures_delta.clear();
        }
        let (left, text_left) = placed.unwrap();
        // The line numbers, their rule and a gap sit left of the text.
        assert!(
            text_left - left > 18.0 + f32::from(TEXT_GAP),
            "{left} {text_left}"
        );
    }

    #[test]
    fn a_short_script_still_fills_the_editor_to_its_bottom() {
        let ctx = egui::Context::default();
        let mut text = String::from("wait(1)\n");
        let mut memory = Memory::default();
        let mut sizes = None;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 900.0))),
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let bottom = ui.max_rect().bottom();
                let none = std::collections::BTreeSet::new();
                let shown = show(
                    ui,
                    &mut text,
                    &[],
                    &mut memory,
                    &Dots {
                        lines: &none,
                        statements: None,
                    },
                );
                sizes = Some((shown.output.response.rect.bottom(), bottom));
            });
            output.textures_delta.clear();
        }
        let (text_bottom, bottom) = sizes.unwrap();
        // The text box, and so its gutter rule, runs to the foot of the view.
        assert!((bottom - text_bottom).abs() < 1.0, "{text_bottom} {bottom}");
    }

    #[test]
    fn layout_keeps_every_byte() {
        let text = "n: int = 1 # é\r\n\tpress(\"CTRL+I\")\n\n";
        assert_eq!(layout(text).text, text);
    }
}
