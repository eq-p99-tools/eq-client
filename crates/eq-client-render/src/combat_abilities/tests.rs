use super::*;
use crate::online::testing;
use eq_client_core::{WorldEvent, disciplines::Reuse};
use std::time::Duration;

fn warrior(known: &[u32]) -> OnlineState {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.level = 60;
    player.disciplines = Some(known.to_vec());
    testing::admit(&mut online, 1, player);
    online
}

fn names() -> SpellNames {
    let mut fields: Vec<String> = vec!["0".into(); 217];
    fields[0] = "4503".into();
    fields[1] = "Evasive Discipline".into();
    fields[167] = "1".into();
    fields[168] = "1".into();
    SpellNames::parse(&fields.join("^"))
}

#[test]
fn the_window_lists_the_known_disciplines_in_list_order() {
    let online = warrior(&[4503, 0, 4499, 0]);
    assert_eq!(listed(online.world()), [4503, 4499]);
    assert_eq!(listed(OnlineState::new(true).world()), Vec::<u32>::new());
}

#[test]
fn a_row_shows_the_time_left_while_its_timer_runs() {
    let now = Instant::now();
    let mut online = warrior(&[4503]);
    assert_eq!(
        caption(online.world(), &names(), 4503, now),
        "Evasive Discipline"
    );
    testing::news_at(
        &mut online,
        [WorldEvent::DisciplineReuse(Reuse {
            timer: 1,
            remaining: Duration::from_millis(41_500),
        })],
        now,
    );
    assert_eq!(
        caption(online.world(), &names(), 4503, now),
        "Evasive Discipline (42s)"
    );
    assert!(crate::disciplines::waits(
        online.world(),
        &names(),
        4503,
        now
    ));
    // A spell the file lacks shows its number.
    assert_eq!(caption(online.world(), &names(), 4499, now), "Spell 4499");
}

#[test]
fn a_clicked_row_uses_its_discipline_through_the_one_path() {
    let mut app = App::new();
    app.insert_resource(warrior(&[4499, 4503]))
        .init_resource::<ChatState>()
        .add_systems(Update, (fill, input).chain());
    crate::clicks::testing::install(&mut app);
    app.world_mut().spawn(Rows::default());
    app.update();
    let mut entries = app
        .world_mut()
        .query::<(Entity, &Entry, &hotbar::Pickable)>();
    let rows: Vec<_> = entries
        .iter(app.world())
        .map(|(entity, entry, pickable)| (entity, entry.0, pickable.0))
        .collect();
    assert_eq!(rows.len(), 2);
    let (row, spell, source) = rows
        .iter()
        .copied()
        .find(|(_, spell, _)| *spell == 4503)
        .unwrap();
    // A hold or Ctrl and a number puts the same discipline on the bar.
    assert!(matches!(
        source,
        hotbar::Source::Fixed(Binding::Discipline(4503))
    ));
    crate::clicks::testing::click(&mut app, row);
    assert_eq!(
        app.world().resource::<ChatState>().disciplines,
        [crate::disciplines::Asked::Use(spell)]
    );
}
