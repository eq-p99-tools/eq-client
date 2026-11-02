//! HTTP(S) links in the plain-text portions of chat messages.
use crate::theme::{self, Size};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task, TaskPool, block_on, poll_once},
};
use linkify::{LinkFinder, LinkKind};
use std::ops::Range;
use url::Url;

/// A parsed web address; item-link labels never become web links.
#[derive(Component)]
pub(crate) struct WebLink(Url);

fn urls(text: &str) -> impl Iterator<Item = (Range<usize>, Url)> + '_ {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    finder.links(text).filter_map(|link| {
        let text = link.as_str();
        if !text
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
            && !text
                .get(..8)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
        {
            return None;
        }
        let url = Url::parse(text).ok()?;
        (matches!(url.scheme(), "http" | "https") && url.has_host())
            .then_some((link.start()..link.end(), url))
    })
}

pub(crate) fn contains_url(text: &str) -> bool {
    urls(text).next().is_some()
}

/// Keeps punctuation and whitespace intact while making web addresses clickable.
pub(crate) fn spawn_text(parent: &mut ChildSpawnerCommands, text: &str, color: Color, size: Size) {
    let mut end = 0;
    for (range, url) in urls(text) {
        if range.start > end {
            parent.spawn((
                TextSpan::new(&text[end..range.start]),
                theme::font(size),
                TextColor(color),
            ));
        }
        parent.spawn((
            TextSpan::new(&text[range.clone()]),
            theme::font(size),
            TextColor(theme::LINK),
            bevy::text::Underline,
            WebLink(url),
            Interaction::None,
        ));
        end = range.end;
    }
    if end < text.len() {
        parent.spawn((
            TextSpan::new(&text[end..]),
            theme::font(size),
            TextColor(color),
        ));
    }
}

/// Browser startup runs off the frame thread; a failed launch is reported in chat.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    links: crate::clicks::Clicks<WebLink>,
    mut pending: Local<Vec<Task<std::io::Result<()>>>>,
    mut chat: ResMut<super::ChatState>,
) {
    pending.retain_mut(|task| match block_on(poll_once(task)) {
        None => true,
        Some(result) => {
            if result.is_err() {
                chat.refuse("Could not open the link in your browser.");
            }
            false
        }
    });
    for link in links.iter() {
        let url = link.0.clone();
        pending.push(
            IoTaskPool::get_or_init(TaskPool::new)
                .spawn(async move { webbrowser::open(url.as_str()) }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_web_links_with_unicode_queries_fragments_and_sentence_punctuation() {
        let text = "é 🗡 (https://example.org/wiki/Item_(Rare)), HTTP://example.com/?a=1&b=2#part!\nhttps://例え.jp/道.";
        let found: Vec<_> = urls(text).collect();
        assert_eq!(
            found
                .iter()
                .map(|(range, _)| &text[range.clone()])
                .collect::<Vec<_>>(),
            [
                "https://example.org/wiki/Item_(Rare)",
                "HTTP://example.com/?a=1&b=2#part",
                "https://例え.jp/道",
            ]
        );
        assert_eq!(found[1].1.as_str(), "http://example.com/?a=1&b=2#part");
        assert_eq!(found[2].1.host_str(), Some("xn--r8jz45g.jp"));
    }

    #[test]
    fn chat_quotes_are_not_part_of_the_url() {
        for text in [
            "You say, 'https://example.org/path'",
            "Someone tells you, 'See https://example.org/path'",
            "\"https://example.org/path\"",
        ] {
            let found: Vec<_> = urls(text).map(|(_, url)| url.to_string()).collect();
            assert_eq!(found, ["https://example.org/path"]);
        }
    }

    #[test]
    fn leaves_other_schemes_bare_domains_and_invalid_urls_as_text() {
        for text in [
            "example.com www.example.com user@example.com",
            "ftp://example.com file:///C:/test.html mailto:user@example.com",
            "javascript:alert(1) data:text/html,test steam://run/1",
            "http:// https:// http:/// https://?query http://#fragment",
            "https://example.com:99999/path https://[broken]/",
        ] {
            assert!(!contains_url(text), "unexpected link in {text}");
        }
    }

    fn render_line(line: eq_client_core::chat::ChatLine, times: bool) -> App {
        let mut app = App::new();
        crate::clicks::testing::install(&mut app);
        app.add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(|parent| {
                super::super::spawn_line(
                    parent,
                    &eq_client_core::chat::Arrival {
                        id: 1,
                        at: chrono::NaiveDateTime::default(),
                        line: &line,
                    },
                    (None, times),
                    ("", None),
                );
            });
        });
        app.update();
        app
    }

    #[test]
    fn web_and_item_links_preserve_the_entire_line_and_item_labels() {
        let prefix = "é 🗡 https://example.org/one then ";
        let label = "https://item.example/";
        let text = format!("{prefix}{label} and https://example.org/two.");
        let mut line = super::super::system_line(text.clone());
        line.message
            .item_links
            .push(eq_client_core::chat::ItemLink {
                body: "synthetic".into(),
                text: label.into(),
                item_id: 42,
                start: 0,
                end: 0,
                text_start: prefix.len(),
                text_end: prefix.len() + label.len(),
            });
        // Timestamps shift item offsets; both display settings must preserve links.
        for times in [false, true] {
            let mut app = render_line(line.clone(), times);
            let world = app.world_mut();
            let (root, children) = world.query::<(&Text, &Children)>().single(world).unwrap();
            let shown = children.iter().fold(root.0.clone(), |mut text, child| {
                text.push_str(&world.get::<TextSpan>(child).unwrap().0);
                text
            });
            assert_eq!(
                shown,
                format!("{}{text}", if times { "[00:00:00] " } else { "" })
            );
            let web: Vec<_> = world
                .query::<(&WebLink, &TextSpan, &TextColor, &bevy::text::Underline)>()
                .iter(world)
                .map(|(link, text, color, _)| {
                    assert_eq!(color.0, theme::LINK);
                    assert_eq!(link.0.as_str(), text.0);
                    text.0.clone()
                })
                .collect();
            assert_eq!(web, ["https://example.org/one", "https://example.org/two"]);
            let (item, span) = world
                .query::<(&crate::items::ItemButton, &TextSpan)>()
                .single(world)
                .unwrap();
            assert_eq!(item.0.item_id, 42);
            assert_eq!(span.0, label);
        }
    }

    #[test]
    fn a_web_only_line_uses_the_shared_click_rule_and_can_cancel_a_click() {
        #[derive(Resource, Default)]
        struct Opened(Vec<String>);
        let mut app = render_line(
            super::super::system_line("See https://example.org/path."),
            false,
        );
        app.init_resource::<Opened>().add_systems(
            Update,
            |links: crate::clicks::Clicks<WebLink>, mut opened: ResMut<Opened>| {
                opened.0.extend(links.iter().map(|link| link.0.to_string()));
            },
        );
        let world = app.world_mut();
        let link = world
            .query_filtered::<Entity, With<WebLink>>()
            .single(world)
            .unwrap();
        crate::clicks::testing::press(&mut app, Some(link));
        assert_eq!(app.world().resource::<Opened>().0, Vec::<String>::new());
        crate::clicks::testing::release(&mut app, None);
        assert_eq!(app.world().resource::<Opened>().0, Vec::<String>::new());
        crate::clicks::testing::click(&mut app, link);
        app.update();
        assert_eq!(
            app.world().resource::<Opened>().0,
            ["https://example.org/path"]
        );
    }
}
