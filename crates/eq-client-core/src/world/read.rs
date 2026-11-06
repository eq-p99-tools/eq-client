//! What the world knows, for any front end to read; only the session's news
//! and the player's own choices change it.
use super::{
    Camp, CastDone, Casting, CharacterList, ClientWorld, Exchange, Landing, Loot, Merchant, Motion,
    MotionGrant, ServerList, Spawn, Target, Vitals,
};
use crate::{
    BookActionStatus, Coins, Death, PlayerState, PostureState, SpellBook, ZoneOffer,
    buffs::BuffTracker, combat::ConColor, doors::DoorTable, ground::Objects, inventory::Inventory,
};
use std::collections::BTreeMap;

impl ClientWorld {
    /// Latest session request for host-owned movement inputs; reset on admission.
    #[must_use]
    pub const fn speed_input_revision(&self) -> Option<u64> {
        self.speed_input_revision
    }

    /// The local session's receipt for ready inputs in this admission/revision.
    /// It does not grant movement. Frontends must match the exact posted report
    /// identity and inputs; requirements, withdrawal and resets invalidate it.
    #[must_use]
    pub const fn accepted_speed_inputs(&self) -> Option<crate::SpeedInputAcceptance> {
        self.accepted_speed_inputs
    }

    /// Movement load in tenths from one worker replica. Unknown inventory/coins
    /// never become zero. A proven cursor-arrival history can conservatively
    /// overstate carried mass without confirming predicted placement.
    ///
    /// # Errors
    /// Returns the input category requiring authoritative state.
    pub fn movement_weight(&self) -> Result<u64, eq_network_game::movement::SpeedInputUnavailable> {
        use eq_network_game::{
            inventory::WeightUnavailable, movement::SpeedInputUnavailable as Missing,
        };
        if !self.inventory.received() || self.inventory.stale() {
            return Err(Missing::Inventory);
        }
        let purse = self.wallet.purse.ok_or(Missing::Coins)?;
        // CoinsElsewhere establishes the cursor. Its bank balance is irrelevant
        // to carried weight and can become unknown independently.
        let cursor = self.known_cursor_coins.ok_or(Missing::Coins)?;
        let coins: u64 = [purse, cursor]
            .iter()
            .map(|c| {
                [c.platinum, c.gold, c.silver, c.copper]
                    .into_iter()
                    .map(u64::from)
                    .sum::<u64>()
            })
            .sum();
        self.inventory
            .carried_weight_tenths(self.generation(), coins)
            .map_err(|reason| match reason {
                WeightUnavailable::Prediction => Missing::Prediction,
                WeightUnavailable::Inventory | WeightUnavailable::Container => Missing::Inventory,
            })
    }

    /// The weight a landing's cost takes, in the program's units: the
    /// programs' sum ([`Self::movement_weight`]; their fall cost reads the
    /// same weight as their speeds, Titanium's from the field the stat
    /// recompute 0x4212b3 fills with 0x41bb06 and `EQMac`'s fall 0x4bfe16
    /// calling 0x4bbb78) where it can be established, else this client's
    /// own larger reading ([`Self::carried_weight`]), which overstates the
    /// weight and so the damage: the safe side where the client reports
    /// its falls.
    #[must_use]
    pub fn fall_weight(&self) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        self.movement_weight()
            .map_or_else(|_| self.carried_weight(), |tenths| tenths as f32 / 10.0)
    }

    /// Changes when the player's skills, languages, level or admission change.
    /// Unrelated movement, chat and resource updates leave it unchanged.
    #[must_use]
    pub const fn skills_revision(&self) -> u64 {
        self.skills_revision
    }

    /// Changes when training offers open, change or end, or the world resets.
    /// Skill values are tracked separately by [`Self::skills_revision`].
    #[must_use]
    pub const fn training_revision(&self) -> u64 {
        self.training_revision
    }

    /// Changes when merchant stock opens, changes or closes, or the world resets.
    #[must_use]
    pub const fn merchant_revision(&self) -> u64 {
        self.merchant_revision
    }

    /// Whether the session has admitted the player and is connected.
    #[must_use]
    pub const fn connected(&self) -> bool {
        self.connected
    }

    /// Whether the session is over and will not reconnect.
    #[must_use]
    pub const fn ended(&self) -> bool {
        self.ended
    }

    /// The time in Norrath now, once the server has given it: the time it
    /// gave, run on by the real time since.
    #[must_use]
    pub fn game_time(&self, now: std::time::Instant) -> Option<crate::clock::GameTime> {
        self.time
            .map(|(time, given)| time.after(now.saturating_duration_since(given).as_secs()))
    }

    /// How the zone's sky and fog look, once the zone has said.
    #[must_use]
    pub const fn sky(&self) -> Option<crate::clock::ZoneSky> {
        self.zone.sky
    }

    /// The world server's short name.
    #[must_use]
    pub fn world_name(&self) -> Option<&str> {
        self.world_name.as_deref()
    }

    /// The worlds the login server listed, until the chosen world lists
    /// characters.
    #[must_use]
    pub const fn servers(&self) -> Option<&ServerList> {
        self.servers.as_ref()
    }

    /// The characters the world server offered, until one enters the world.
    #[must_use]
    pub const fn characters(&self) -> Option<&CharacterList> {
        self.characters.as_ref()
    }

    /// Expansions the world lets this account use; unknown until reported.
    pub const fn expansions(&self) -> Option<crate::rules::Expansions> {
        self.expansions
    }

    /// Last name suggestion, scoped to a frontend request.
    pub const fn name_suggestion(&self) -> Option<&super::NameSuggestion> {
        self.name_suggestion.as_ref()
    }

    /// Whether the world enables its generation UI (unknown means allowed).
    pub const fn name_generator(&self) -> bool {
        !matches!(self.name_generator, Some(false))
    }

    /// The world's last answer to a character the player asked for.
    #[must_use]
    pub const fn creation(&self) -> Option<&super::Creation> {
        self.creation.as_ref()
    }

    /// What the admission lets the player do: a front end greys out or hides
    /// the rest.
    #[must_use]
    pub fn capabilities(&self) -> &[crate::Capability] {
        &self.capabilities
    }

    /// Whether the admission lets the player do this: what it offers, and
    /// what it leaves to the player once they turn it on.
    #[must_use]
    pub fn can(&self, capability: crate::Capability) -> bool {
        self.capabilities.contains(&capability)
            || (self.leaves(capability) && self.chosen.contains(&capability))
    }

    /// What the admission leaves to the player, whether or not they turned
    /// it on.
    #[must_use]
    pub fn choices(&self) -> &[crate::Capability] {
        &self.choices
    }

    /// Whether the admission leaves this to the player: the server's own
    /// client keeps it off, so the player has it only once they turn it on.
    #[must_use]
    pub fn leaves(&self, capability: crate::Capability) -> bool {
        self.choices.contains(&capability)
    }

    /// The current admission, which commands must name.
    #[must_use]
    pub const fn session_id(&self) -> Option<u64> {
        self.session_id
    }

    /// The admission's interpreted own-player stun state. None preserves
    /// unknown/unconfigured semantics; charm, roots and death are separate.
    #[must_use]
    pub const fn stun_state(&self) -> Option<crate::control::StunState> {
        match self.own_stun {
            Some((_, state)) => Some(state),
            None => None,
        }
    }

    /// The latest stun transition revision, including expiry. Consumers can
    /// withdraw queued intent even when a complete stun crossed one frame.
    #[must_use]
    pub const fn stun_revision(&self) -> Option<u64> {
        match self.own_stun {
            Some((revision, _)) => Some(revision),
            None => None,
        }
    }

    /// Whether a verified own stun is active. False is not proof of known
    /// clear state: inspect [`Self::stun_state`] when that distinction matters.
    #[must_use]
    pub fn is_stunned(&self) -> bool {
        self.stun_state() == Some(crate::control::StunState::Stunned)
    }

    /// The zone's short name, as the server gave it.
    #[must_use]
    pub fn zone(&self) -> &str {
        &self.zone_name
    }

    /// The zone's long name, from the zone list, or its short name for a
    /// zone the list lacks. Inferred: the official client names the zone as
    /// the server's zone header does, which the session doesn't read, and
    /// the list is taken to agree with it.
    #[must_use]
    pub fn zone_long_name(&self) -> &str {
        crate::zones::long_name(&self.zone_name).unwrap_or(&self.zone_name)
    }

    /// How strongly the zone pulls the player down, as its header gives it
    /// (`OP_NewZone` +420): the fall speed added each 50 ms step. None until
    /// this admission's header comes.
    #[must_use]
    pub const fn zone_gravity(&self) -> Option<f32> {
        self.zone_gravity
    }

    /// The zone's top and lowest heights, as its header gives them (Titanium
    /// `OP_NewZone` +504 and +508, `EQMac` +500 and +504). None until this
    /// admission's header comes.
    #[must_use]
    pub const fn zone_heights(&self) -> Option<crate::ZoneHeights> {
        self.zone_heights
    }

    /// The Titanium zone header's classic model scale flag (`OP_NewZone`
    /// +592), as it came: while it is not zero, the Titanium program draws
    /// the classic character models at 1/1.3 of their usual scale. None
    /// until this admission's header comes, and on a generation without it.
    #[must_use]
    pub const fn classic_model_scale_flag(&self) -> Option<u32> {
        self.classic_model_scale_flag
    }

    /// The zone's far clip distance, when the server gave one.
    #[must_use]
    pub const fn far_clip(&self) -> Option<f32> {
        self.far_clip
    }

    /// The world's player-versus-player ruleset, by the server's number: 0
    /// for none, and until the world's rules say.
    #[must_use]
    pub const fn pvp_rule(&self) -> u32 {
        self.pvp_rule
    }

    /// The world's received language-zero name selector. None means no
    /// selector has been established in this session; received zero stays
    /// Some, separately from `PvP`. A new session's restart clears this fact.
    #[must_use]
    pub const fn language_zero_selector(&self) -> Option<crate::rules::LanguageZeroSelector> {
        self.language_zero_selector
    }

    /// The zone header's minimum clip, the nearest its far plane comes;
    /// None until this admission's header comes.
    #[must_use]
    pub const fn min_clip(&self) -> Option<f32> {
        self.min_clip
    }

    /// Details supplied by an existing death event, until the next admission.
    /// A death-only bind transfer supplies lifecycle knowledge without details.
    #[must_use]
    pub const fn death(&self) -> Option<&Death> {
        match self.death.as_ref() {
            Some(super::OwnDeath::Reported { details, .. }) => Some(details),
            _ => None,
        }
    }

    /// Whether the session established death from a death event or a death-only
    /// bind transfer. This never implies any killer or corpse details.
    #[must_use]
    pub const fn is_dead(&self) -> bool {
        self.death.is_some()
    }

    /// Latest admission-scoped feedback from the local movement worker. Success
    /// establishes its installed policy, not server acceptance or a speed grant.
    #[must_use]
    pub const fn movement_options_result(
        &self,
    ) -> Option<&(u64, Result<crate::movement::MovementOptions, String>)> {
        self.movement_options_result.as_ref()
    }

    /// When this admission first established own death, for the avatar's
    /// death animation. This does not establish a corpse in the entity map.
    #[must_use]
    pub const fn death_since(&self) -> Option<std::time::Instant> {
        match self.death {
            Some(
                super::OwnDeath::Reported { since, .. } | super::OwnDeath::BindTransfer { since },
            ) => Some(since),
            None => None,
        }
    }

    /// The player's HP, with what their items add, when the server's last
    /// report said they bled out and no front end has reported it yet.
    #[must_use]
    pub const fn bled_out(&self) -> Option<i64> {
        self.bled_out
    }

    /// The transfer the server offered, until it is answered.
    #[must_use]
    pub const fn pending_transfer(&self) -> Option<&ZoneOffer> {
        self.pending_transfer.as_ref()
    }

    /// The admitted player.
    #[must_use]
    pub const fn player(&self) -> Option<&PlayerState> {
        self.player.as_ref()
    }

    /// The spells memorized in the player's eight gems; none before admission.
    #[must_use]
    pub fn gems(&self) -> [Option<u32>; 8] {
        self.player
            .as_ref()
            .map_or([None; 8], |player| player.memorized_spells)
    }

    /// The spell memorized in this gem, counted from 0, if any.
    #[must_use]
    pub fn gem(&self, gem: usize) -> Option<u32> {
        self.gems().get(gem).copied().flatten()
    }

    /// The player's pet: the spawn the player owns.
    #[must_use]
    pub fn pet(&self) -> Option<&Spawn> {
        self.pet_of(self.player.as_ref()?.spawn_id)
    }

    /// The pet a spawn owns, while it is in view.
    #[must_use]
    pub fn pet_of(&self, owner: u16) -> Option<&Spawn> {
        self.zone
            .spawns
            .values()
            .find(|spawn| spawn.state.pet_owner == Some(owner))
    }

    /// The translocate offered and not yet answered, and when it came.
    #[must_use]
    pub fn translocate(
        &self,
    ) -> Option<(&crate::confirmations::TranslocateOffer, std::time::Instant)> {
        self.translocate.as_ref().map(|(offer, at)| (offer, *at))
    }

    /// The resurrection offered and not yet answered.
    #[must_use]
    pub fn resurrection(&self) -> Option<&crate::resurrection::ResurrectionOffer> {
        self.resurrection.as_ref()
    }

    /// The book or note open to read.
    #[must_use]
    pub fn reading(&self) -> Option<&crate::books::BookText> {
        self.reading.as_ref()
    }

    /// The world container open for the player, such as a forge.
    #[must_use]
    pub fn container(&self) -> Option<&crate::ground::ContainerView> {
        self.zone.container.as_ref()
    }

    /// The tradeskill container whose combine waits for the server.
    #[must_use]
    pub const fn combining(&self) -> Option<crate::inventory::InventorySlot> {
        self.combining
    }

    /// The guildmaster the player is training with, and how far they train
    /// each skill.
    #[must_use]
    pub fn training(&self) -> Option<&crate::training::TrainingOffer> {
        self.zone.training.as_ref()
    }

    /// The pet's buffs, in their slots, once the server has said.
    #[must_use]
    pub fn pet_buffs(&self) -> Option<&crate::pets::PetBuffs> {
        let pet = self.pet()?.state.spawn_id;
        self.zone
            .pet_buffs
            .as_ref()
            .filter(|buffs| buffs.pet == pet)
    }

    /// The zone's spawns, by spawn ID.
    #[must_use]
    pub const fn spawns(&self) -> &BTreeMap<u16, Spawn> {
        &self.zone.spawns
    }

    /// One spawn.
    #[must_use]
    pub fn spawn(&self, id: u16) -> Option<&Spawn> {
        self.zone.spawns.get(&id)
    }

    /// The player's target.
    #[must_use]
    pub const fn target(&self) -> &Target {
        &self.target
    }

    /// Correlation ID for the next explicit local target choice.
    ///
    /// # Panics
    /// Panics if all 64-bit selection IDs have been used in this process.
    #[must_use]
    pub fn next_target_request(&self) -> u64 {
        self.target_sequence
            .checked_add(1)
            .expect("target sequence exhausted")
    }

    /// Current target identity for a queued ability. A pending local choice,
    /// unresolved server ID or lost incarnation has no usable identity.
    #[must_use]
    pub fn target_intent(&self) -> Option<eq_network_game::targets::TargetState> {
        self.target.authority.filter(|state| {
            self.in_world()
                && self.target.sent
                && state.available
                && state.spawn_id == self.target.selected
                && !self.target_stale()
        })
    }

    /// Whether the target is gone: its spawn despawned, was replaced or
    /// turned invisible. The player choosing themselves never goes stale.
    #[must_use]
    pub fn target_stale(&self) -> bool {
        if self.target.server_selected
            && self.target.authority.is_some_and(|state| !state.available)
        {
            return self.target.selected.is_some();
        }
        self.target.selected.is_some_and(|id| {
            !self.is_player(id)
                && self.zone.spawns.get(&id).is_none_or(|spawn| {
                    spawn.state.invisible || Some(spawn.revision) != self.target.revision
                })
        })
    }

    /// The level color the server gave when the player last considered a
    /// spawn.
    #[must_use]
    pub fn considered(&self, id: u16) -> Option<ConColor> {
        self.zone.considered.get(&id).copied()
    }

    /// Whether a spawn ID is the player's.
    #[must_use]
    pub fn is_player(&self, id: u16) -> bool {
        self.player
            .as_ref()
            .is_some_and(|player| player.spawn_id == id)
    }

    /// A spawn's latest one-shot motion, such as a swing, the player's own
    /// included, while it is in view.
    #[must_use]
    pub fn motion_of(&self, id: u16) -> Option<Motion> {
        self.zone.motions.get(&id).copied()
    }

    /// When a corpse in view died, for one the player saw die rather than
    /// one that arrived dead.
    #[must_use]
    pub fn died_at(&self, id: u16) -> Option<std::time::Instant> {
        self.zone.deaths.get(&id).copied()
    }

    /// The latest cast a spawn completed, the player's own included, while
    /// it is in view: its spell landed while the cast was under way.
    #[must_use]
    pub fn completed_cast_of(&self, id: u16) -> Option<CastDone> {
        self.zone.completions.get(&id).copied()
    }

    /// The spell a spawn is casting, the player's own included, while the
    /// cast is under way and the spawn in view.
    #[must_use]
    pub fn casting_of(&self, id: u16) -> Option<u16> {
        self.zone.casts.get(&id).map(|cast| cast.spell_id)
    }

    /// The latest spell that landed on a spawn, the player included, while
    /// it is in view.
    #[must_use]
    pub fn landing_on(&self, id: u16) -> Option<Landing> {
        self.zone.landings.get(&id).copied()
    }

    /// The posture last reported for the player or a spawn.
    #[must_use]
    pub fn posture(&self, id: u16) -> Option<PostureState> {
        if self.is_player(id) {
            return self.zone.player_posture;
        }
        self.zone.spawns.get(&id).and_then(|spawn| spawn.posture)
    }

    /// The health last reported for the player or a spawn, in percent.
    #[must_use]
    pub fn health(&self, id: u16) -> Option<u8> {
        match self.player.as_ref() {
            Some(player) if player.spawn_id == id => player.hp_percent,
            _ => self.zone.spawns.get(&id).and_then(|spawn| spawn.health),
        }
    }

    /// The zone's doors.
    #[must_use]
    pub const fn doors(&self) -> &DoorTable {
        &self.zone.doors
    }

    /// The zone's items on the ground and world containers.
    #[must_use]
    pub const fn objects(&self) -> &Objects {
        &self.zone.objects
    }

    /// The player's mana, endurance and experience.
    #[must_use]
    pub const fn vitals(&self) -> &Vitals {
        &self.vitals
    }

    /// The player's HP to show, current and maximum, once reported: what the
    /// server reported, with what equipped items add when it leaves that out.
    #[must_use]
    pub fn hit_points(&self) -> Option<(u32, u32)> {
        self.vitals.hit_points(self.death.is_some())
    }

    /// The player's inventory, bank and cursor as the server reported them,
    /// with the ordered local projection of successfully queued moves.
    #[must_use]
    pub fn inventory(&self) -> &Inventory {
        self.inventory_moves
            .projected
            .as_ref()
            .unwrap_or(&self.inventory)
    }

    /// The ordered worker replica, including only locally submitted predictions
    /// and received server updates. Movement weight, equipment and food must use
    /// this same view. It does not imply server acceptance of submitted moves.
    #[must_use]
    pub const fn worker_inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// The corpse the player is looting.
    #[must_use]
    pub const fn loot(&self) -> Option<&Loot> {
        self.zone.trade.loot.as_ref()
    }

    /// The generation's reply boundary for automatic loot continuation.
    #[must_use]
    pub const fn loot_continuation(&self) -> super::LootContinuation {
        super::LootContinuation::of(self.generation)
    }

    /// The coins in a place, where the client knows them: a trade window's
    /// only while it is open.
    #[must_use]
    pub fn coins_in(&self, place: crate::money::CoinPlace) -> Option<Coins> {
        use crate::money::CoinPlace;
        match place {
            CoinPlace::Trade => self
                .zone
                .trade
                .exchange
                .as_ref()
                .filter(|exchange| exchange.open)
                .map(|_| self.wallet.given),
            place => self.wallet.get(place),
        }
    }

    /// The give or trade window the player asked for or has open.
    #[must_use]
    pub const fn exchange(&self) -> Option<&Exchange> {
        self.zone.trade.exchange.as_ref()
    }

    /// The coins the other player put in the open trade window.
    #[must_use]
    pub fn offered_coins(&self) -> Option<Coins> {
        self.zone
            .trade
            .exchange
            .as_ref()
            .filter(|exchange| exchange.open)
            .map(|_| self.wallet.offered)
    }

    /// How fed and watered the player is, as the server last said.
    #[must_use]
    pub const fn nourishment(&self) -> Option<crate::food::Nourishment> {
        self.nourishment
    }

    /// The merchant the player is trading with.
    #[must_use]
    pub const fn merchant(&self) -> Option<&Merchant> {
        self.zone.trade.merchant.as_ref()
    }

    /// An item's definition, when the server sent it this admission.
    #[must_use]
    pub fn item(&self, id: u32) -> Option<&crate::ItemDetails> {
        self.items.get(id)
    }

    /// How the session lets the player move, once it says.
    #[must_use]
    pub const fn motion(&self) -> Option<MotionGrant> {
        self.motion
    }

    /// Camping under way.
    #[must_use]
    pub const fn camp(&self) -> Option<Camp> {
        self.camp
    }

    /// The coins the player carries, as last reported.
    #[must_use]
    pub const fn coins(&self) -> Option<&Coins> {
        self.wallet.purse.as_ref()
    }

    /// What the player carries, in the weight units the programs count a
    /// fall's cost in (TAKP Batch 1 R3, recording checklist item 6.161):
    /// a tenth of each item's weight and a fortieth of each coin. The
    /// program lessens a bag's contents by the bag's reduction, which the
    /// item data the sessions decode lacks, so they count in full; a stack
    /// counts each of its units; and worn items, the carried packs and
    /// bags, the cursor and the coins in the purse and on the cursor count
    /// (this client's reading).
    #[must_use]
    pub fn carried_weight(&self) -> f32 {
        let items: u64 = self
            .inventory
            .items()
            .iter()
            .filter(|(slot, _)| {
                slot.is_equipment()
                    || slot.is_carried()
                    || **slot == crate::inventory::InventorySlot::CURSOR
                    || slot.is_in_cursor_bag()
            })
            .map(|(_, item)| {
                u64::from(item.details.weight_tenths)
                    * u64::from(item.stack_count.unwrap_or(1).max(1))
            })
            .sum();
        let coins: u64 = [self.wallet.purse.unwrap_or_default(), self.wallet.cursor]
            .iter()
            .map(|coins| {
                [coins.platinum, coins.gold, coins.silver, coins.copper]
                    .into_iter()
                    .map(u64::from)
                    .sum::<u64>()
            })
            .sum();
        // Weights past 2^24 tenths lose only fractions of a unit.
        #[allow(clippy::cast_precision_loss)]
        let weight = items as f32 / 10.0 + coins as f32 * 0.025;
        weight
    }

    /// The player's own casting and gem timers.
    #[must_use]
    pub const fn casting(&self) -> &Casting {
        &self.casting
    }

    /// The player's buffs and the lasting effects not yet slotted.
    #[must_use]
    pub const fn buffs(&self) -> &BuffTracker {
        &self.buffs
    }

    /// The player's spellbook.
    #[must_use]
    pub const fn spell_book(&self) -> Option<&SpellBook> {
        self.spell_book.as_ref()
    }

    /// The spellbook change in flight, until confirmed.
    #[must_use]
    pub const fn book_action(&self) -> Option<&BookActionStatus> {
        self.book_action.as_ref()
    }

    /// Counts the session's spellbook replies, so that two alike still differ.
    #[must_use]
    pub const fn book_action_revision(&self) -> u64 {
        self.book_action_revision
    }

    /// Whether the player can act now: admitted, connected, alive and not
    /// zoning.
    #[must_use]
    pub const fn in_world(&self) -> bool {
        self.connected
            && self.session_id.is_some()
            && self.death.is_none()
            && self.pending_transfer.is_none()
    }

    /// Whether a reply that names this admission still applies: it is the
    /// current one and connected.
    #[must_use]
    pub fn accepts_reply(&self, session_id: u64) -> bool {
        self.connected && self.session_id == Some(session_id)
    }
}
