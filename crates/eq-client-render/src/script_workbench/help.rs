//! Render the bundled Markdown help pages using the client's own UI.
//!
//! These authored pages use headings, paragraphs, bullets, inline code, fenced
//! examples and page links. No HTML, network fetches or executable links.

use super::view::{self, Action, Mono};
use crate::theme;
use bevy::prelude::*;

struct Page {
    name: &'static str,
    source: &'static str,
    parent: Option<usize>,
}

const PAGES: [Page; 6] = [
    Page {
        name: "README.md",
        source: include_str!("../../../../docs/scripts/README.md"),
        parent: None,
    },
    Page {
        name: "examples.md",
        source: include_str!("../../../../docs/scripts/examples.md"),
        parent: Some(0),
    },
    Page {
        name: "reference.md",
        source: include_str!("../../../../docs/scripts/reference.md"),
        parent: Some(0),
    },
    Page {
        name: "running.md",
        source: include_str!("../../../../docs/scripts/running.md"),
        parent: Some(0),
    },
    Page {
        name: "targets.md",
        source: include_str!("../../../../docs/scripts/targets.md"),
        parent: Some(2),
    },
    Page {
        name: "vim.md",
        source: include_str!("../../../../docs/scripts/vim.md"),
        parent: Some(0),
    },
];

fn path(mut page: usize) -> Vec<usize> {
    let mut result = Vec::new();
    while let Some(entry) = PAGES.get(page) {
        result.push(page);
        let Some(parent) = entry.parent else {
            break;
        };
        page = parent;
    }
    result.reverse();
    result
}

/// Fixed above the scrolling article; links in the Markdown remain available.
pub(super) fn breadcrumbs(commands: &mut Commands, parent: Entity, page: usize) {
    for (depth, index) in path(page).into_iter().enumerate() {
        if depth > 0 {
            let divider = view::label(commands, "/", 13.0, theme::INK_DIM);
            commands.entity(parent).add_child(divider);
        }
        let title = PAGES[index]
            .source
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("# "))
            .unwrap_or(PAGES[index].name);
        let button = view::button(commands, title, Action::Help(index));
        commands.entity(parent).add_child(button);
    }
}

fn link(line: &str) -> Option<(&str, &str)> {
    let line = line.strip_prefix('[')?.strip_suffix(')')?;
    line.split_once("](")
}

pub(super) fn draw(commands: &mut Commands, parent: Entity, page: usize, mono: &Mono) {
    let Some(page) = PAGES.get(page) else {
        return;
    };
    let mut code = false;
    for line in page.source.lines() {
        if line.starts_with("```") {
            code = !code;
            continue;
        }
        if code {
            view::source_line(commands, parent, line, None, false, mono);
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if let Some((title, destination)) = link(line) {
            if let Some(index) = PAGES.iter().position(|page| page.name == destination) {
                let button = view::button(commands, title, Action::Help(index));
                commands.entity(parent).add_child(button);
            }
            continue;
        }
        let (content, size, color) = if let Some(title) = line.strip_prefix("# ") {
            (title, 21.0, theme::INK_BRIGHT)
        } else if let Some(title) = line.strip_prefix("## ") {
            (title, 16.0, theme::FOCUS)
        } else {
            (line, 13.0, theme::INK)
        };
        let text = view::label(commands, "", size, color);
        for (index, piece) in content.split('`').enumerate() {
            let span = commands
                .spawn((
                    TextSpan::new(piece),
                    TextFont {
                        font_size: FontSize::Px(size),
                        ..default()
                    },
                    TextColor(if index % 2 == 1 {
                        theme::INK_WARM
                    } else {
                        color
                    }),
                ))
                .id();
            if index % 2 == 1 {
                commands.entity(span).insert(view::font(mono, 13.0));
            }
            commands.entity(text).add_child(span);
        }
        commands.entity(text).insert(Node {
            flex_shrink: 0.0,
            ..default()
        });
        commands.entity(parent).add_child(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_links_resolve_inside_the_bundled_pages() {
        for page in PAGES {
            for line in page.source.lines() {
                if let Some((_, destination)) = link(line) {
                    assert!(
                        PAGES.iter().any(|page| page.name == destination),
                        "{}: broken link {destination}",
                        page.name
                    );
                }
            }
        }
    }

    #[test]
    fn breadcrumb_paths_have_a_stable_home_and_target_parent() {
        assert_eq!(path(0), [0]);
        assert_eq!(path(4), [0, 2, 4]);
        for (index, page) in PAGES.iter().enumerate() {
            assert!(page.parent.is_none_or(|parent| parent < index));
            assert_eq!(path(index).first(), Some(&0));
            assert_eq!(path(index).last(), Some(&index));
        }
    }

    #[test]
    fn every_runnable_help_example_parses_with_the_real_language() {
        let mut examples = 0;
        for page in PAGES {
            let mut source = None;
            for line in page.source.lines() {
                if line == "```eqscript" {
                    source = Some(String::new());
                } else if line == "```" {
                    if let Some(source) = source.take() {
                        crate::script::Program::parse(&source, std::path::Path::new("."))
                            .unwrap_or_else(|error| panic!("{}: {error}", page.name));
                        examples += 1;
                    }
                } else if let Some(source) = source.as_mut() {
                    source.push_str(line);
                    source.push('\n');
                }
            }
            assert!(source.is_none(), "{}: unclosed example", page.name);
        }
        assert!(examples >= 3);
    }
}
