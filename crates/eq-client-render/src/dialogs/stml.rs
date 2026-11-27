//! Inert server-popup STML. Table cells follow the native grid structure.
//! Equal flexible columns are a usable approximation: native width/height
//! solving, backgrounds and borders have not been established.
use bevy::prelude::*;

use crate::theme::{self, Size};

type Rgb = [u8; 3];
const WHITE: Rgb = [255; 3];
// Bound server-authored spans and nesting independently of native limits.
const MAX_SPAN: u16 = 64;
const MAX_DEPTH: usize = 16;

#[derive(Debug, PartialEq)]
struct Run {
    text: String,
    color: Rgb,
}

#[derive(Debug, PartialEq)]
enum Block {
    Text(Vec<Run>),
    Table(Table),
}

#[derive(Debug, Default, PartialEq)]
struct Table {
    cells: Vec<Cell>,
    columns: usize,
}

#[derive(Debug, PartialEq)]
struct Cell {
    row: usize,
    column: usize,
    colspan: u16,
    rowspan: u16,
    blocks: Vec<Block>,
}

#[derive(Debug)]
pub(super) struct Document {
    blocks: Vec<Block>,
    gaps: Vec<String>,
}

impl Document {
    pub(super) fn parse(markup: &str) -> Self {
        let mut parser = Parser {
            rest: markup,
            colors: Vec::new(),
            gaps: Vec::new(),
        };
        let blocks = parser.blocks(&[], 0);
        Self {
            blocks,
            gaps: parser.gaps,
        }
    }

    pub(super) fn spawn(&self, parent: &mut ChildSpawnerCommands) {
        for gap in &self.gaps {
            eq_client_core::gaps::unshown(gap);
        }
        spawn_blocks(parent, &self.blocks);
    }
}

fn spawn_blocks(parent: &mut ChildSpawnerCommands, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Text(runs) => {
                parent
                    .spawn((
                        Text::default(),
                        theme::font(Size::Body),
                        TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                        Node {
                            width: percent(100),
                            min_width: px(0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ))
                    .with_children(|text| {
                        for run in runs {
                            let [r, g, b] = run.color;
                            text.spawn((
                                TextSpan::new(&run.text),
                                theme::font(Size::Body),
                                TextColor(Color::srgb_u8(r, g, b)),
                            ));
                        }
                    });
            }
            Block::Table(table) => spawn_table(parent, table),
        }
    }
}

fn spawn_table(parent: &mut ChildSpawnerCommands, table: &Table) {
    parent
        .spawn(Node {
            display: Display::Grid,
            width: percent(100),
            min_width: px(0),
            flex_shrink: 0.0,
            grid_template_columns: vec![GridTrack::flex(1.0); table.columns],
            column_gap: px(8),
            row_gap: px(4),
            ..default()
        })
        .with_children(|grid| {
            for cell in &table.cells {
                grid.spawn(Node {
                    min_width: px(0),
                    overflow: Overflow::clip_x(),
                    flex_direction: FlexDirection::Column,
                    grid_row: GridPlacement::start_span(
                        i16::try_from(cell.row + 1).unwrap_or(i16::MAX),
                        cell.rowspan,
                    ),
                    grid_column: GridPlacement::start_span(
                        i16::try_from(cell.column + 1).unwrap_or(i16::MAX),
                        cell.colspan,
                    ),
                    ..default()
                })
                .with_children(|body| spawn_blocks(body, &cell.blocks));
            }
        });
}

struct Parser<'a> {
    rest: &'a str,
    colors: Vec<Rgb>,
    gaps: Vec<String>,
}

impl Parser<'_> {
    fn blocks(&mut self, stops: &[&str], depth: usize) -> Vec<Block> {
        let mut blocks = Vec::new();
        while !self.rest.is_empty() {
            if let Some(tag) = Tag::read(self.rest) {
                if stops.contains(&tag.name.as_str()) {
                    break;
                }
                self.rest = &self.rest[tag.raw.len()..];
                if tag.name == "table" && depth < MAX_DEPTH {
                    self.attributes(&tag, &[]);
                    blocks.push(Block::Table(self.table(depth + 1)));
                } else {
                    if tag.name == "table" {
                        self.gap("table nesting outside layout limit");
                    }
                    self.inline(&mut blocks, &tag);
                }
            } else if let Some((entity, next)) = self
                .rest
                .strip_prefix('&')
                .and_then(|tail| tail.split_once(';'))
                && let Some(ch) = entity_text(entity)
            {
                self.text(&mut blocks, &ch.to_string());
                self.rest = next;
            } else {
                let ch = self.rest.chars().next().expect("nonempty STML");
                if !ch.is_control() || matches!(ch, '\n' | '\t') {
                    self.text(&mut blocks, &ch.to_string());
                }
                self.rest = &self.rest[ch.len_utf8()..];
            }
        }
        blocks
    }

    fn text(&self, blocks: &mut Vec<Block>, text: &str) {
        let color = self.colors.last().copied().unwrap_or(WHITE);
        if !matches!(blocks.last(), Some(Block::Text(_))) {
            blocks.push(Block::Text(Vec::new()));
        }
        let Some(Block::Text(runs)) = blocks.last_mut() else {
            unreachable!()
        };
        if let Some(last) = runs.last_mut()
            && last.color == color
        {
            last.text.push_str(text);
        } else {
            runs.push(Run {
                text: text.to_owned(),
                color,
            });
        }
    }

    fn inline(&mut self, blocks: &mut Vec<Block>, tag: &Tag) {
        match tag.name.as_str() {
            "c" | "font" | "f" => {
                let current = self.colors.last().copied().unwrap_or(WHITE);
                let value = if tag.name == "c" {
                    tag.attrs.first().map(|(name, _)| name.as_str())
                } else {
                    tag.value("color")
                };
                self.colors.push(value.and_then(color).unwrap_or(current));
                if tag.name == "c" {
                    if let Some((name, Some(_))) = tag.attrs.first() {
                        self.gap(&format!("c attribute {}", safe_name(name)));
                    }
                    for (name, _) in tag.attrs.iter().skip(1) {
                        self.gap(&format!("c attribute {}", safe_name(name)));
                    }
                } else {
                    self.attributes(tag, &["color"]);
                    if tag.name == "f" {
                        self.gap("tag f font selection");
                    }
                }
            }
            "/c" | "/font" | "/f" => {
                self.colors.pop();
            }
            "br" | "p" => {
                self.text(blocks, "\n");
                self.attributes(tag, &[]);
            }
            "head" | "/head" | "body" | "/body" => self.attributes(tag, &[]),
            "b" | "/b" | "u" | "/u" | "a" | "/a" => {
                self.gap(&format!("tag {} formatting", tag.name));
                self.attributes(tag, &[]);
            }
            name if name.starts_with('/') => {
                self.gap(&format!("closing tag {}", safe_name(name)));
            }
            _ => self.text(blocks, tag.raw),
        }
    }

    fn table(&mut self, depth: usize) -> Table {
        let mut table = Table::default();
        let mut occupied = Vec::new();
        let mut row = 0;
        let mut started = false;
        while !self.rest.is_empty() {
            let Some(tag) = Tag::read(self.rest) else {
                let blocks = self.blocks(&TABLE_STOPS, depth);
                if !only_whitespace(&blocks) {
                    table.add(&mut occupied, row, 1, 1, blocks);
                    started = true;
                }
                continue;
            };
            match tag.name.as_str() {
                "/table" => {
                    self.rest = &self.rest[tag.raw.len()..];
                    break;
                }
                "tr" => {
                    self.rest = &self.rest[tag.raw.len()..];
                    row += usize::from(started);
                    started = true;
                    self.attributes(&tag, &[]);
                }
                "td" | "th" => {
                    self.rest = &self.rest[tag.raw.len()..];
                    let colspan = self.span(&tag, "colspan");
                    let rowspan = self.span(&tag, "rowspan");
                    self.attributes(&tag, &["colspan", "rowspan"]);
                    let blocks = self.blocks(&CELL_STOPS, depth);
                    table.add(&mut occupied, row, colspan, rowspan, blocks);
                    started = true;
                }
                "thead" | "/thead" | "tbody" | "/tbody" | "tfoot" | "/tfoot" | "/td" | "/th" => {
                    self.rest = &self.rest[tag.raw.len()..];
                    self.attributes(&tag, &[]);
                }
                "col" | "/col" | "colgroup" | "/colgroup" | "caption" | "/caption" => {
                    self.rest = &self.rest[tag.raw.len()..];
                    self.gap(&format!("table tag {} formatting", tag.name));
                    self.attributes(&tag, &[]);
                }
                _ => {
                    let blocks = self.blocks(&TABLE_STOPS, depth);
                    if !only_whitespace(&blocks) {
                        table.add(&mut occupied, row, 1, 1, blocks);
                        started = true;
                    }
                }
            }
        }
        table
    }

    fn span(&mut self, tag: &Tag, name: &str) -> u16 {
        let Some(value) = tag.value(name) else {
            return 1;
        };
        if let Ok(span) = value.parse::<u16>()
            && (1..=MAX_SPAN).contains(&span)
        {
            return span;
        }
        self.gap(&format!(
            "{} attribute {name} outside layout limit",
            tag.name
        ));
        1
    }

    fn attributes(&mut self, tag: &Tag, supported: &[&str]) {
        for (name, _) in &tag.attrs {
            if !supported.contains(&name.as_str()) {
                self.gap(&format!("{} attribute {}", tag.name, safe_name(name)));
            }
        }
    }

    fn gap(&mut self, reason: &str) {
        let name = format!("Server popup STML {reason}");
        if !self.gaps.contains(&name) {
            self.gaps.push(name);
        }
    }
}

const CELL_STOPS: [&str; 18] = TABLE_STOPS;
const TABLE_STOPS: [&str; 18] = [
    "td",
    "th",
    "/td",
    "/th",
    "tr",
    "/table",
    "thead",
    "/thead",
    "tbody",
    "/tbody",
    "tfoot",
    "/tfoot",
    "col",
    "/col",
    "colgroup",
    "/colgroup",
    "caption",
    "/caption",
];

impl Table {
    fn add(
        &mut self,
        occupied: &mut Vec<usize>,
        row: usize,
        colspan: u16,
        rowspan: u16,
        blocks: Vec<Block>,
    ) {
        let width = usize::from(colspan);
        let mut column = 0;
        while (column..column + width)
            .any(|index| occupied.get(index).is_some_and(|end| *end > row))
        {
            column += 1;
        }
        occupied.resize(occupied.len().max(column + width), 0);
        occupied[column..column + width].fill(row + usize::from(rowspan));
        self.columns = self.columns.max(column + width);
        self.cells.push(Cell {
            row,
            column,
            colspan,
            rowspan,
            blocks,
        });
    }
}

fn only_whitespace(blocks: &[Block]) -> bool {
    blocks.iter().all(|block| match block {
        Block::Text(runs) => runs.iter().all(|run| run.text.trim().is_empty()),
        Block::Table(_) => false,
    })
}

#[derive(Debug)]
struct Tag<'a> {
    raw: &'a str,
    name: String,
    attrs: Vec<(String, Option<String>)>,
}

impl<'a> Tag<'a> {
    fn read(rest: &'a str) -> Option<Self> {
        let end = rest.strip_prefix('<')?.find('>')? + 1;
        let mut contents = rest[1..end].trim();
        let name = token(&mut contents)?.to_ascii_lowercase();
        let name = if name.starts_with('/') {
            name
        } else {
            name.trim_end_matches('/').to_owned()
        };
        let mut attrs = Vec::new();
        while let Some(name) = token(&mut contents) {
            if name == "/" {
                break;
            }
            let value = if let Some(tail) = contents.trim_start().strip_prefix('=') {
                contents = tail;
                token(&mut contents).map(str::to_owned)
            } else {
                None
            };
            attrs.push((name.to_ascii_lowercase(), value));
        }
        Some(Self {
            raw: &rest[..=end],
            name,
            attrs,
        })
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| value.as_deref())
    }
}

fn token<'a>(rest: &mut &'a str) -> Option<&'a str> {
    *rest = rest.trim_start();
    while rest.starts_with('=') {
        *rest = rest[1..].trim_start();
    }
    let first = rest.chars().next()?;
    if matches!(first, '\'' | '"') {
        let tail = &rest[1..];
        let end = tail.find(first).unwrap_or(tail.len());
        let value = &tail[..end];
        *rest = tail.get(end + 1..).unwrap_or_default();
        Some(value)
    } else {
        let end = rest
            .find(|ch: char| ch.is_whitespace() || ch == '=')
            .unwrap_or(rest.len());
        let value = &rest[..end];
        *rest = &rest[end..];
        Some(value)
    }
}

fn safe_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-'))
        .take(32)
        .collect()
}

fn color(value: &str) -> Option<Rgb> {
    let value = value.to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let [_, r, g, b] = u32::from_str_radix(hex, 16).ok()?.to_be_bytes();
        return Some([r, g, b]);
    }
    Some(match value.as_str() {
        "black" => [0, 0, 0],
        "silver" => [192, 192, 192],
        "gray" | "grey" => [128, 128, 128],
        "white" => WHITE,
        "maroon" => [128, 0, 0],
        "red" => [255, 0, 0],
        "purple" => [128, 0, 128],
        "fuchsia" => [255, 0, 255],
        "green" => [0, 128, 0],
        "lime" => [0, 255, 0],
        "olive" => [128, 128, 0],
        "yellow" => [255, 255, 0],
        "navy" => [0, 0, 128],
        "blue" => [0, 0, 255],
        "teal" => [0, 128, 128],
        "aqua" => [0, 255, 255],
        _ => return None,
    })
}

fn entity_text(entity: &str) -> Option<char> {
    match entity.to_ascii_lowercase().as_str() {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
