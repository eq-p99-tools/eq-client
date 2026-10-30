//! The player's invisibility, dropped before a cast, a song or an item's
//! click where the installed client does so
//! ([`InstalledClient::invisibility_drop`]): the Mac-era program fades the
//! player's invisibility buffs and tells the server the state left, each
//! fade in the spell's own words (from the program, TAKP Batch 1 R1;
//! recording checklist item 6.163).
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{ClientCommand, PostureState, world::ClientWorld};

/// The colour the program says a spell's fade in, as a chat line's type.
const FADE_TYPE: u32 = 264;

/// Sends the drop before a cast where the installed client makes one, and
/// gives back the lines that say each fade. The program drops after its
/// refusal of a player who is not standing, so a player known to be
/// sitting or otherwise not standing drops nothing (the cast's refusal is
/// the server's). Nothing goes when no buff of the player's carries the
/// effects, or the spells' effects are not in the installed data.
pub(crate) fn drop_before_cast(
    world: &ClientWorld,
    outbox: &crate::outbox::Outbox,
    spells: &crate::spellbook::SpellNames,
    client: InstalledClient,
) -> Vec<eq_client_core::chat::ChatLine> {
    let (Some(order), Some(player), Some(slots)) = (
        client.invisibility_drop(),
        world.player(),
        world.buffs().slots(),
    ) else {
        return Vec::new();
    };
    if world
        .posture(player.spawn_id)
        .is_some_and(|posture| posture != PostureState::Standing)
    {
        return Vec::new();
    }
    let steps = eq_client_core::buffs::invisibility_drop(slots, order, |spell| {
        spells
            .mechanics(spell)
            .map(|mechanics| mechanics.effects.iter().map(|effect| effect.id).collect())
    });
    if steps.is_empty() {
        return Vec::new();
    }
    let faded: Vec<u16> = steps
        .iter()
        .filter_map(|step| match step {
            eq_client_core::InvisibilityStep::Fade { spell_id, .. } => Some(*spell_id),
            _ => None,
        })
        .collect();
    let sent = outbox.post(world, |stamp| ClientCommand::DropInvisibility {
        session_id: stamp.session_id,
        steps,
    });
    if sent.is_err() {
        return Vec::new();
    }
    faded
        .into_iter()
        .filter_map(|spell| spells.fades(u32::from(spell)))
        .map(|text| {
            let mut line = crate::chat::system_line(text);
            line.message_type = Some(FADE_TYPE);
            line
        })
        .collect()
}
