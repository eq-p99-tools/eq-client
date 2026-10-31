//! Presentation resources and explicit frame-order contracts.
//! Keep shared-input and deferred-entity ordering visible at this composition boundary.
use crate::{
    abilities, buffs, character, character_select, chat, coins, combat, confirm, daylight, doors,
    entities, escape, exit_after_screenshot, give, ground, group, hud, interact, inventory,
    item_models, items, keys, login, logs, map, motion, move_player, names, navigation,
    navmesh_prebuild, notices, online, options, orbit_camera, outbox, outfit, paperdoll, pet,
    profile_files, raid, reading, resources, schedule_screenshot, select_preview, sheets, skills,
    skin, skinned, sounds, speed, spell_effects, spell_icons, spellbook, target, tooltip, trade,
    tradeskills, training, translocate, update_hud, whereabouts, who, windows,
};
use bevy::prelude::*;
/// Starts the presentation state (HUD, windows, chat, targeting, inventory,
/// trade, combat and motion) empty: every resource the windows keep, in one
/// list that the tests' app starts from too.
pub(super) fn init_presentation(app: &mut App) {
    app.init_resource::<spell_effects::SpellParticles>()
        .init_resource::<sounds::Sounds>()
        .init_resource::<doors::Hinges>()
        .init_resource::<options::OptionsState>()
        .init_resource::<logs::ChatLog>()
        .init_resource::<hud::HudState>()
        .init_resource::<hud::action_bar::ActionRequests>()
        .init_resource::<combat::CombatState>()
        .init_resource::<trade::TradeState>()
        .init_resource::<escape::Escape>()
        .init_resource::<windows::pointer::Wheel>()
        .init_resource::<skin::UiSkin>()
        .init_resource::<hud::hotbar::Bindings>()
        .init_resource::<hud::hotbar::carry::Presses>()
        .init_resource::<hud::hotbar::carry::Carry>()
        .init_resource::<spellbook::BookView>()
        .init_resource::<spellbook::BookSelection>()
        .init_resource::<spellbook::BookHand>()
        .init_resource::<sheets::Sheets>()
        .init_resource::<skinned::Screens>()
        .init_resource::<skinned::CursorLook>()
        .init_resource::<skinned::Skinned>()
        .init_resource::<skinned::Tabs>()
        .init_resource::<skinned::looks::Looks>()
        .init_resource::<skinned::KeyFilter>()
        .init_resource::<chat::ChatState>()
        .init_resource::<chat::ChatLook>()
        .init_resource::<profile_files::Profile>()
        .init_resource::<notices::Lines>()
        .init_resource::<items::ItemState>()
        .init_resource::<inventory::InventoryState>()
        .init_resource::<motion::Controls>()
        .init_resource::<entities::NearbyEntities>()
        .init_resource::<names::NameTags>()
        .init_resource::<item_models::ItemLibrary>()
        .init_resource::<outfit::Wardrobe>()
        .init_resource::<select_preview::Preview>()
        .init_resource::<windows::DragState>()
        .init_resource::<windows::Layouts>()
        .init_resource::<windows::Shown>()
        .init_resource::<windows::Stack>()
        .init_resource::<keys::KeyMap>()
        .init_resource::<keys::Typing>()
        .init_resource::<training::Chosen>()
        .init_resource::<raid::RaidChoice>()
        .init_resource::<confirm::Asked>()
        .init_resource::<reading::Page>()
        .init_resource::<map::MapView>()
        .init_resource::<login::FrontEnd>()
        .init_resource::<login::LoginLook>();
}

/// Orders world reduction before scene reconciliation, then text entry before
/// routing and gameplay input, and finally presentation. The chained groups
/// also serialize shared focus/consumption and deferred entity changes. Keep
/// those edges when extracting feature registration; parallelism needs a
/// measured benefit and an explicit audit of input arbitration first.
#[allow(
    clippy::too_many_lines,
    reason = "one declarative listing of every system in its stage"
)]
pub(super) fn schedule(app: &mut App) {
    app.configure_sets(
        Update,
        (
            Stage::Receive,
            Stage::Scene,
            Stage::Typing,
            Stage::Route,
            Stage::Input,
            Stage::Present,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (
            (
                online::receive,
                online::report_bleed_out,
                login::watch,
                online::tick,
                daylight::update,
                spell_effects::load,
                sounds::load,
            )
                .chain()
                .in_set(Stage::Receive),
            (
                item_models::poll,
                outfit::poll_materials,
                entities::reconcile,
                entities::interpolate,
                character::lie,
                doors::reconcile,
                ground::reconcile,
                select_preview::show,
                select_preview::turn,
            )
                .chain()
                .in_set(Stage::Scene),
            (
                login::claim,
                windows::pointer::wheel,
                // A scrollbar's press scrolls its box as the wheel does.
                skinned::scrollbar::scroll,
                navigation::update,
                chat::input,
                skinned::type_amount,
            )
                .chain()
                .in_set(Stage::Typing),
            (escape::route, hud::hotbar::carry::route).in_set(Stage::Route),
        ),
    )
    .add_systems(
        Update,
        (
            items::link_input,
            items::input,
            inventory::input,
            coins::input,
            inventory::colors::input,
            interact::input,
            target::input,
            (
                who::zone_list,
                whereabouts::answers,
                pet::window,
                group::window,
                raid::window,
            ),
            combat::input,
            (trade::input, speed::report).chain(),
            (
                give::buttons,
                give::inspect_theirs,
                training::buttons,
                confirm::buttons,
                reading::buttons,
                tradeskills::buttons,
                map::buttons,
                raid::buttons,
            ),
            (abilities::input, skinned::slash),
            hud::actions,
            (hud::hotbar::update, hud::hotbar::persist).chain(),
            hud::hotbar::item_actions,
            (
                // A book the player stands up from closes before it draws.
                spellbook::seat,
                (
                    spellbook::update,
                    spellbook::book_clicks,
                    spellbook::say_book_lines,
                ),
            )
                .chain(),
            (
                // Which login screens show decides which take the presses.
                (
                    login::read_look,
                    (login::form, login::worlds, login::light),
                    navmesh_prebuild::run_batch(),
                )
                    .chain(),
                (
                    character_select::update,
                    character_select::names,
                    login::quit,
                ),
            ),
            windows::input,
            move_player,
            motion::input,
        )
            .chain()
            .in_set(Stage::Input),
    )
    .add_systems(
        Update,
        (
            (
                inventory::update,
                inventory::feedback,
                inventory::scroll,
                (skinned::cursor_look, inventory::cursor::update).chain(),
                (chat::follow_options, chat::look, chat::refresh).chain(),
                items::update,
                items::scroll,
                target::update,
                combat::target_color,
                trade::present,
                trade::scroll,
                (login::login_screen, login::worlds_screen, login::scroll).chain(),
                skinned::scroll_lists,
                motion::interpolate,
                orbit_camera,
                update_hud,
                // Opens the give, quantity and training windows before their
                // frames are drawn.
                (
                    give::window,
                    skinned::quantity,
                    training::window,
                    (translocate::decline_unasked, confirm::window).chain(),
                    reading::window,
                    tradeskills::world_window,
                    map::load,
                ),
            )
                .chain(),
            (
                resources::update,
                hud::update,
                hud::spell_details,
                hud::key_help,
                target::key_help,
                hud::hotbar::item_artwork,
                (hud::hotbar::presentation, hud::hotbar::fit_words).chain(),
                spellbook::scribe_presentation,
                buffs::update,
                (buffs::hover, buffs::skinned_details, buffs::short_window),
                (
                    (spell_icons::update, skinned::gem::pieces),
                    logs::write,
                    (
                        options::toggle,
                        options::persist,
                        options::choose,
                        options::tell_session,
                    )
                        .chain(),
                ),
                (outbox::show, inventory::say_refusals),
                (hud::action_bar::cast_window, hud::action_bar::update),
                skinned::frames,
                (
                    skinned::apply,
                    skinned::place_own,
                    skinned::looks::load,
                    skinned::looks::dress,
                )
                    .chain(),
                skinned::show,
                skinned::buttons,
                (
                    skinned::contents,
                    skinned::theirs,
                    skinned::loot,
                    (
                        trade::fill_wares,
                        trade::picture,
                        items::icon,
                        spellbook::book_present,
                    ),
                    skinned::tabs,
                    abilities::present,
                    (
                        skinned::slide,
                        skinned::drop_downs,
                        skinned::light_choices,
                        skinned::show_sliders,
                        skinned::show_amount,
                        skinned::show_choices,
                        skinned::fill_lists,
                        training::fill,
                        skills::fill,
                        raid::fill,
                        confirm::show,
                        reading::show,
                        tradeskills::show,
                        map::draw,
                        skinned::scrollbar::place,
                        skinned::rewind,
                    ),
                ),
                skinned::close,
            )
                .chain(),
            (
                hud::hotbar::needs,
                abilities::needs,
                outbox::veil,
                outbox::grey_out,
                tooltip::end_scripted_presses,
                tooltip::show,
                select_preview::veil,
                outfit::dress,
                character::animate,
                spell_effects::update,
                sounds::update,
                target::marker::update,
                (names::request, names::update).chain(),
                schedule_screenshot,
                exit_after_screenshot,
            )
                .chain(),
        )
            .chain()
            .in_set(Stage::Present),
    );
}

/// The order of every frame's work. A system joins the stage for what it
/// does instead of naming the systems it must follow; within a stage, the
/// listing above orders the few that depend on one another.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Stage {
    /// The session's news reaches the world, and the world's clocks run.
    Receive,
    /// The world reaches the scene: spawns, doors and items on the ground.
    Scene,
    /// The chat takes the keyboard first, and the wheel goes to one surface.
    Typing,
    /// Escape and the window stack decide what a press belongs to, and so
    /// does a hotkey on the cursor or a hold that picks one up.
    Route,
    /// Keys and clicks become requests and moves.
    Input,
    /// Windows, the HUD, the camera and the models show the world; refusals
    /// show once every input has had its say.
    Present,
}

/// Registers the startup work of the overlays and the windows' shared systems.
pub(super) fn install_overlays(app: &mut App) {
    app.add_systems(Startup, tooltip::spawn);
    windows::register_layout(app);
    paperdoll::register(app);
    app.add_systems(
        Update,
        profile_files::follow
            .after(Stage::Receive)
            .before(Stage::Scene),
    );
    skin::register(app);
    app.add_systems(PostUpdate, chat::scroll.after(bevy::ui::UiSystems::Layout));
}
