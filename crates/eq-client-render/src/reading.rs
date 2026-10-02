//! Reading books and notes, as the official client shows them: right-clicking
//! a readable item asks the server for its text, which opens the skin's note
//! window for a note or scroll and its book window for a book, two pages at a
//! time with arrows to turn them. Done, or closing the window, puts it away.
use crate::{
    online::OnlineState,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::world::ClientWorld;

/// How many characters a line of a book's page holds, and how many lines a
/// page holds, as the skin's default book pages (190 by 246 pixels) fit
/// them in the client's text.
const PAGE_COLUMNS: usize = 26;
const PAGE_LINES: usize = 16;

/// The first of the two pages the book window shows, from 0.
#[derive(Resource, Default)]
pub(crate) struct Page(pub(crate) usize);

/// A book window's arrow: forward (true) or back.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageButton(pub(crate) bool);

/// Where a window shows the text being read.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Text {
    /// The note window's text.
    Note,
    /// The book window's left (0) or right (1) page.
    Page(usize),
    /// The book window's number for its left (0) or right (1) page.
    Number(usize),
}

/// The window a text opens in: the book window for a book, the note window
/// for anything else.
fn window_for(world: &ClientWorld) -> Option<WindowId> {
    let text = world.reading()?;
    Some(if text.kind == 1 {
        WindowId::Book
    } else {
        WindowId::Note
    })
}

/// The pages of the book being read.
fn pages(world: &ClientWorld) -> Vec<String> {
    world.reading().map_or_else(Vec::new, |text| {
        eq_client_core::reading::pages(&text.text, PAGE_COLUMNS, PAGE_LINES)
    })
}

/// Keeps the note or book window open while there is something to read; a
/// window the player closes puts the text away.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    mut online: ResMut<OnlineState>,
    mut shown: ResMut<Shown>,
    mut page: ResMut<Page>,
    mut opened: Local<Option<WindowId>>,
) {
    let wanted = window_for(online.world());
    if let Some(id) = *opened {
        if wanted == Some(id) && !shown.is_open(id) {
            // The player closed the window: the text is put away.
            online.close_reading();
            *opened = None;
            return;
        }
        if wanted != Some(id) {
            shown.close(id);
            *opened = None;
        }
    }
    if let Some(id) = wanted
        && *opened != Some(id)
    {
        shown.open(id);
        page.0 = 0;
        *opened = Some(id);
    }
}

/// Turns the book's pages, two at a time, within the book.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    online: Res<OnlineState>,
    mut page: ResMut<Page>,
    buttons: Query<(&Interaction, &PageButton), Changed<Interaction>>,
) {
    let count = pages(online.world()).len();
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if button.0 && page.0 + 2 < count {
            page.0 += 2;
        } else if !button.0 && page.0 >= 2 {
            page.0 -= 2;
        }
    }
}

/// Shows the text being read: the note's whole text, or the book's two pages
/// and their numbers.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    online: Res<OnlineState>,
    page: Res<Page>,
    mut texts: Query<(&Text, &mut bevy::prelude::Text)>,
) {
    if texts.is_empty() {
        return;
    }
    let world = online.world();
    let pages = pages(world);
    for (shows, mut text) in &mut texts {
        let wanted = match *shows {
            Text::Note => world.reading().map_or_else(String::new, |text| {
                eq_client_core::reading::plain(&text.text)
            }),
            Text::Page(side) => pages.get(page.0 + side).cloned().unwrap_or_default(),
            Text::Number(side) => {
                let number = page.0 + side + 1;
                if number <= pages.len() {
                    number.to_string()
                } else {
                    String::new()
                }
            }
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{WorldEvent, books::BookText};

    fn read(kind: u8, text: &str) -> App {
        let mut app = crate::testing::app();
        app.init_resource::<Page>()
            .add_systems(Update, (window, buttons, show).chain());
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(
            &mut online,
            [WorldEvent::BookText(BookText {
                kind,
                text: text.into(),
            })],
        );
        app.insert_resource(online);
        app
    }

    #[test]
    fn a_note_opens_the_note_window_and_closing_it_puts_the_note_away() {
        let mut app = read(0, "To the guard,<BR>greetings.");
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Note));
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Book));
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Note);
        app.update();
        assert!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .reading()
                .is_none()
        );
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Note));
    }

    #[test]
    fn a_book_opens_the_book_window_at_its_first_pages() {
        let long = "word ".repeat(PAGE_COLUMNS * PAGE_LINES);
        let mut app = read(1, &long);
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Book));
        assert_eq!(app.world().resource::<Page>().0, 0);
        assert!(pages(app.world().resource::<OnlineState>().world()).len() > 2);
    }
}
