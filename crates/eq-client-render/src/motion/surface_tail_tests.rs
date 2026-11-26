//! Independent API provenance and negative controls for the P3 join rule.
use eq_client_core::movement::water::{
    LIQUID_5, WET_FRAME_SECONDS, WetFrameMedium, WetInput, WetVelocity,
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decline {
    UnknownPhase,
    UnknownMedium,
    InvalidPiece,
    NotNumericTail,
    NoPrecedingPiece,
    DifferentVelocity,
    DifferentStoredMedium,
    Transition,
    AlreadyJoined,
    NotAdjacent,
    StateMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownMedium {
    Air,
    Water([u32; 4]),
}
fn known_medium(value: WetFrameMedium) -> Result<KnownMedium, Decline> {
    match value {
        WetFrameMedium::Air => Ok(KnownMedium::Air),
        WetFrameMedium::Water(set) => Ok(KnownMedium::Water([
            set.horizontal_input.to_bits(),
            set.vertical_input.to_bits(),
            set.horizontal_keep.to_bits(),
            set.vertical_keep.to_bits(),
        ])),
        // Non-exhaustive library enum. Never substitute ordinary water/air.
        _ => Err(Decline::UnknownMedium),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StoredMedium {
    known: KnownMedium,
    // The real client must obtain these from its genuine collision/session
    // context. Tests supply explicit fixture identities, not inferred regions.
    owner_epoch: u64,
    water_kind: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PieceClass {
    Physical,
    ProvenNumericTail,
}
#[derive(Clone, Copy, Debug)]
struct Origin {
    request_id: u64,
    ordinal: usize,
    owner_epoch: u64,
    water_kind: u8,
}
fn origin(ordinal: usize) -> Origin {
    Origin {
        request_id: 1,
        ordinal,
        owner_epoch: 1,
        water_kind: 5,
    }
}
#[derive(Clone, Debug)]
struct Piece {
    request_id: u64,
    ordinal: usize,
    before: WetVelocity,
    after: WetVelocity,
    class: PieceClass,
    delta: [f32; 3],
    seconds_parts: Vec<f32>,
    velocity_bits: [u32; 3],
    medium: StoredMedium,
    completes_frame: bool,
    transition: bool,
    joined: bool,
}
struct Predicted {
    after: WetVelocity,
    piece: Piece,
}

// All provenance fields are filled immediately from the actual API call.
// Do not turn this into a helper accepting arbitrary piece flags or guessed
// boundary state. The caller's original velocity is never mutated here.
fn predict(
    original: WetVelocity,
    medium: WetFrameMedium,
    origin: Origin,
    input: WetInput,
    seconds: f32,
) -> Result<Predicted, Decline> {
    let phase_before = original.frame_phase().ok_or(Decline::UnknownPhase)?;
    let stored = StoredMedium {
        known: known_medium(medium)?,
        owner_epoch: origin.owner_epoch,
        water_kind: origin.water_kind,
    };
    let mut trial = original;
    let raw = trial
        .advance_piece(medium, input, seconds)
        .ok_or(Decline::InvalidPiece)?;
    let ([vx, vz], vy) = trial.units_per_second();
    let class = if phase_before.at_boundary && !raw.starts_frame && !raw.completes_frame {
        // Positive provenance, no copied window constant:
        // A successful finite positive public API call sets
        // starts_frame = phase.at_boundary && seconds > its own window.
        // Therefore this actual pair at a known boundary entails the
        // clock's bound. Flags alone do not: an interior partial has the
        // same pair. Never infer the class from displacement or duration.
        PieceClass::ProvenNumericTail
    } else {
        PieceClass::Physical
    };
    Ok(Predicted {
        after: trial,
        piece: Piece {
            request_id: origin.request_id,
            ordinal: origin.ordinal,
            before: original,
            after: trial,
            class,
            delta: [raw.horizontal[0], raw.vertical, raw.horizontal[1]],
            seconds_parts: vec![raw.seconds],
            velocity_bits: [vx.to_bits(), vy.to_bits(), vz.to_bits()],
            medium: stored,
            completes_frame: raw.completes_frame,
            transition: false,
            joined: false,
        },
    })
}

// Geometry may be joined; time accounting remains the two exact API pieces.
// No additional stepping, clamp, phase reset, medium refresh or request.
fn join_numeric_tail(previous: Option<&Piece>, tail: &Piece) -> Result<Piece, Decline> {
    if tail.class != PieceClass::ProvenNumericTail {
        return Err(Decline::NotNumericTail);
    }
    let previous = previous.ok_or(Decline::NoPrecedingPiece)?;
    if previous.class != PieceClass::Physical || previous.joined || tail.joined {
        return Err(Decline::AlreadyJoined);
    }
    if previous.request_id != tail.request_id
        || previous.ordinal.checked_add(1) != Some(tail.ordinal)
    {
        return Err(Decline::NotAdjacent);
    }
    if previous.after != tail.before {
        return Err(Decline::StateMismatch);
    }
    if previous.medium != tail.medium {
        return Err(Decline::DifferentStoredMedium);
    }
    if previous.velocity_bits != tail.velocity_bits {
        return Err(Decline::DifferentVelocity);
    }
    if previous.transition || tail.transition {
        return Err(Decline::Transition);
    }
    let mut joined = previous.clone();
    joined.delta = std::array::from_fn(|axis| previous.delta[axis] + tail.delta[axis]);
    joined.seconds_parts.extend_from_slice(&tail.seconds_parts);
    joined.ordinal = tail.ordinal;
    joined.after = tail.after;
    joined.joined = true;
    Ok(joined)
}

fn input() -> WetInput {
    WetInput {
        horizontal: [1.7, 0.0],
        vertical: 1.7 * 15_f32.to_radians().sin(),
        jump: false,
    }
}
fn water() -> WetFrameMedium {
    WetFrameMedium::Water(LIQUID_5)
}
fn request(mut velocity: WetVelocity, ms: u64) -> (WetVelocity, Vec<Piece>) {
    let mut left = Duration::from_millis(ms).as_secs_f32();
    let mut pieces = Vec::new();
    while left > 0.0 {
        let p = predict(velocity, water(), origin(pieces.len()), input(), left).unwrap();
        left -= p.piece.seconds_parts[0];
        velocity = p.after;
        pieces.push(p.piece);
    }
    (velocity, pieces)
}
fn numeric_fixture() -> (Piece, Piece) {
    let (_, pieces) = request(WetVelocity::default(), 100);
    let index = pieces
        .iter()
        .position(|p| p.class == PieceClass::ProvenNumericTail)
        .expect("real API fixture must contain numeric tail");
    assert!(index > 0);
    (pieces[index - 1].clone(), pieces[index].clone())
}

#[test]
fn actual_boundary_tail_joins_exact_displacement_without_restepping() {
    let (previous, tail) = numeric_fixture();
    let joined = join_numeric_tail(Some(&previous), &tail).unwrap();
    let expected = std::array::from_fn::<_, 3, _>(|axis| previous.delta[axis] + tail.delta[axis]);
    assert_eq!(joined.delta.map(f32::to_bits), expected.map(f32::to_bits));
    assert_eq!(
        joined.seconds_parts,
        [previous.seconds_parts, tail.seconds_parts].concat()
    );
    assert_eq!(joined.completes_frame, previous.completes_frame);
    assert_eq!(joined.velocity_bits, previous.velocity_bits);
}

#[test]
fn genuine_interior_partial_with_same_flag_pair_is_not_numeric() {
    let first = predict(
        WetVelocity::default(),
        water(),
        origin(0),
        input(),
        WET_FRAME_SECONDS * 0.5,
    )
    .unwrap();
    assert!(!first.after.frame_phase().unwrap().at_boundary);
    let second = predict(
        first.after,
        water(),
        origin(1),
        input(),
        WET_FRAME_SECONDS * 0.25,
    )
    .unwrap();
    assert_eq!(second.piece.class, PieceClass::Physical);
    assert!(!second.piece.completes_frame);
    assert_eq!(
        join_numeric_tail(Some(&first.piece), &second.piece).err(),
        Some(Decline::NotNumericTail)
    );
}

#[test]
fn an_actual_large_boundary_request_is_not_joined() {
    // Public frame duration, not a duplicate of the private window.
    let large = predict(
        WetVelocity::default(),
        water(),
        origin(0),
        input(),
        WET_FRAME_SECONDS,
    )
    .unwrap();
    assert_eq!(large.piece.class, PieceClass::Physical);
    let (previous, _) = numeric_fixture();
    assert_eq!(
        join_numeric_tail(Some(&previous), &large.piece).err(),
        Some(Decline::NotNumericTail)
    );
}

#[test]
fn isolated_tail_velocity_medium_transition_and_epoch_changes_decline() {
    let (previous, tail) = numeric_fixture();
    assert_eq!(
        join_numeric_tail(None, &tail).err(),
        Some(Decline::NoPrecedingPiece)
    );
    let mut changed = previous.clone();
    changed.velocity_bits[0] ^= 1;
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::DifferentVelocity)
    );
    let mut changed = previous.clone();
    changed.medium.owner_epoch += 1;
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::DifferentStoredMedium)
    );
    let mut changed = previous.clone();
    changed.medium.water_kind += 1;
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::DifferentStoredMedium)
    );
    let mut changed = previous.clone();
    if let KnownMedium::Water(factors) = &mut changed.medium.known {
        factors[0] ^= 1;
    }
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::DifferentStoredMedium)
    );
    let mut changed = previous.clone();
    changed.transition = true;
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::Transition)
    );
    let joined = join_numeric_tail(Some(&previous), &tail).unwrap();
    assert_eq!(
        join_numeric_tail(Some(&joined), &tail).err(),
        Some(Decline::AlreadyJoined)
    );
}

#[test]
fn known_api_cases_map_and_wildcard_stays_a_decline() {
    assert_eq!(known_medium(WetFrameMedium::Air).unwrap(), KnownMedium::Air);
    assert!(matches!(known_medium(water()), Ok(KnownMedium::Water(_))));
    // No unsafe fabrication of a future enum variant. The actual wildcard
    // arm is source/compile coverage only until such a variant exists.
}

#[test]
fn real_api_tail_rates_match_retained_ten_second_manifest() {
    for (cadence, expected) in [
        (&[100_u64][..], 100),
        (&[150][..], 67),
        (&[250][..], 40),
        (&[100, 117, 167, 200, 250][..], 2),
    ] {
        let mut velocity = WetVelocity::default();
        let (mut elapsed, mut index, mut tails) = (0_u64, 0, 0);
        while elapsed < 10_000 {
            let ms = cadence[index % cadence.len()].min(10_000 - elapsed);
            let (after, pieces) = request(velocity, ms);
            tails += pieces
                .iter()
                .filter(|p| p.class == PieceClass::ProvenNumericTail)
                .count();
            velocity = after;
            elapsed += ms;
            index += 1;
        }
        assert_eq!(tails, expected);
    }
}

#[test]
fn request_identity_order_and_snapshot_continuity_are_required() {
    let (previous, tail) = numeric_fixture();
    let mut changed = previous.clone();
    changed.request_id += 1;
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::NotAdjacent)
    );
    let mut changed = previous.clone();
    changed.ordinal += 2;
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::NotAdjacent)
    );
    let mut changed = previous.clone();
    changed.after = WetVelocity::default();
    assert_eq!(
        join_numeric_tail(Some(&changed), &tail).err(),
        Some(Decline::StateMismatch)
    );
}

#[test]
fn a_genuine_completing_partial_is_not_a_tail() {
    let first = predict(
        WetVelocity::default(),
        water(),
        origin(0),
        input(),
        WET_FRAME_SECONDS * 0.5,
    )
    .unwrap();
    let remaining = first.after.frame_phase().unwrap().remaining_seconds;
    let second = predict(first.after, water(), origin(1), input(), remaining).unwrap();
    assert_eq!(second.piece.class, PieceClass::Physical);
    assert!(second.piece.completes_frame);
    assert_eq!(
        join_numeric_tail(Some(&first.piece), &second.piece).err(),
        Some(Decline::NotNumericTail)
    );
}

#[test]
fn unknown_phase_and_invalid_time_leave_the_original_untouched() {
    let mut invalid = WetVelocity::default();
    for _ in 0..1_000_000 {
        if invalid.frame_phase().is_none() {
            break;
        }
        // Fault fixture: the public legacy API is deliberately given1ns
        // calls until its own phase API says unknown. No private bound is
        // copied or used to classify a tail.
        invalid.advance(LIQUID_5, input(), Duration::from_nanos(1).as_secs_f32());
    }
    assert!(invalid.frame_phase().is_none());
    let original = invalid;
    assert_eq!(
        predict(invalid, water(), origin(0), input(), 0.1)
            .err()
            .unwrap(),
        Decline::UnknownPhase
    );
    assert_eq!(invalid, original);
    let original = WetVelocity::default();
    assert_eq!(
        predict(original, water(), origin(0), input(), 0.0)
            .err()
            .unwrap(),
        Decline::InvalidPiece
    );
    assert_eq!(original, WetVelocity::default());
}
