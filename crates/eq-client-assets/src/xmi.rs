//! Extended MIDI (XMI), the form a zone's music takes (`<zone>.xmi`), turned
//! into Standard MIDI files a synthesizer plays. Written from the format: an
//! IFF `FORM XDIR` naming how many sequences follow in a `CAT XMID`, each a
//! `FORM XMID` whose `EVNT` chunk holds the events. Delays are runs of bytes
//! below 0x80, added up; a note-on carries its length as a variable-length
//! number in place of a later note-off; there is no running status. XMI
//! plays at 120 ticks a second, which a Standard MIDI file at 60 ticks a
//! quarter note and 500,000 microseconds a quarter note keeps; the XMI's
//! own tempo events are dropped [I]. How the Titanium program plays it is
//! pending read R49-7 b (recording checklist 6.503).

use thiserror::Error;

/// Why an XMI file could not be read.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum XmiError {
    /// The file ended inside a chunk or an event.
    #[error("the music file ends early")]
    Truncated,
    /// The file is not an XMI.
    #[error("not an XMI music file")]
    NotXmi,
    /// An event's status byte is not one MIDI has.
    #[error("unknown music event {0:#04x}")]
    Event(u8),
    /// The file holds more than this client converts.
    #[error("the music file is larger than this client plays")]
    TooLarge,
}

/// The most sequences converted from one file (this client's own bound).
pub const MAX_SEQUENCES: usize = 256;
/// The most events in one sequence (this client's own bound).
pub const MAX_EVENTS: usize = 262_144;
/// The largest Standard MIDI file one sequence may become (this client's
/// own bound).
pub const MAX_MIDI: usize = 4 * 1024 * 1024;

/// The Standard MIDI file's ticks a quarter note.
const DIVISION: u16 = 60;
/// Its tempo, microseconds a quarter note: with [`DIVISION`], 120 ticks a
/// second.
const TEMPO: u32 = 500_000;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    const fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], XmiError> {
        let end = self.at.checked_add(count).ok_or(XmiError::Truncated)?;
        let taken = self.bytes.get(self.at..end).ok_or(XmiError::Truncated)?;
        self.at = end;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, XmiError> {
        Ok(self.take(1)?[0])
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn tag(&mut self) -> Result<[u8; 4], XmiError> {
        let mut tag = [0; 4];
        tag.copy_from_slice(self.take(4)?);
        Ok(tag)
    }

    fn big_u32(&mut self) -> Result<u32, XmiError> {
        let mut word = [0; 4];
        word.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(word))
    }

    /// A variable-length number: seven bits a byte, high bit set on all but
    /// the last.
    fn number(&mut self) -> Result<u32, XmiError> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let byte = self.byte()?;
            value = (value << 7) | u32::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(XmiError::Truncated)
    }

    /// A chunk: its tag and body, the reader past its even padding.
    fn chunk(&mut self) -> Result<([u8; 4], &'a [u8]), XmiError> {
        let tag = self.tag()?;
        let length = usize::try_from(self.big_u32()?).map_err(|_| XmiError::Truncated)?;
        let body = self.take(length)?;
        if length % 2 == 1 && !self.done() {
            self.at += 1;
        }
        Ok((tag, body))
    }
}

/// Every `EVNT` chunk in the file, one a sequence, in order.
fn event_chunks(bytes: &[u8]) -> Result<Vec<&[u8]>, XmiError> {
    let mut events = Vec::new();
    collect(bytes, &mut events, 0)?;
    if events.is_empty() {
        return Err(XmiError::NotXmi);
    }
    Ok(events)
}

fn collect<'a>(bytes: &'a [u8], events: &mut Vec<&'a [u8]>, depth: u8) -> Result<(), XmiError> {
    if depth > 4 {
        return Err(XmiError::NotXmi);
    }
    let mut reader = Reader::new(bytes);
    while !reader.done() {
        let (tag, body) = reader.chunk()?;
        match &tag {
            b"FORM" | b"CAT " => {
                // A group: its kind, then chunks.
                let inner = body.get(4..).ok_or(XmiError::Truncated)?;
                collect(inner, events, depth + 1)?;
            }
            b"EVNT" => {
                if events.len() >= MAX_SEQUENCES {
                    return Err(XmiError::TooLarge);
                }
                events.push(body);
            }
            _ => {}
        }
    }
    Ok(())
}

fn push_number(out: &mut Vec<u8>, mut value: u32) {
    let mut stack = [0_u8; 5];
    let mut count = 0;
    loop {
        stack[count] = u8::try_from(value & 0x7f).unwrap_or(0);
        count += 1;
        value >>= 7;
        if value == 0 {
            break;
        }
    }
    for index in (0..count).rev() {
        out.push(stack[index] | if index == 0 { 0 } else { 0x80 });
    }
}

/// One timed event of the converted track: when, whether it is a note-off
/// (which goes first at its time), its order of reading, and its bytes.
type Timed = (u64, bool, usize, Vec<u8>);

/// Converts one `EVNT` body into the events of a track.
fn events(body: &[u8]) -> Result<Vec<Timed>, XmiError> {
    let mut reader = Reader::new(body);
    let mut now: u64 = 0;
    let mut timed: Vec<Timed> = Vec::new();
    while let Some(next) = reader.peek() {
        if next < 0x80 {
            now += u64::from(reader.byte()?);
            continue;
        }
        let status = reader.byte()?;
        let order = timed.len();
        if order >= MAX_EVENTS {
            return Err(XmiError::TooLarge);
        }
        match status & 0xf0 {
            0x90 => {
                let note = reader.byte()?;
                let velocity = reader.byte()?;
                let length = reader.number()?;
                timed.push((now, false, order, vec![status, note, velocity]));
                // A zero velocity is already a note-off.
                if velocity > 0 {
                    let off = 0x80 | (status & 0x0f);
                    timed.push((
                        now + u64::from(length),
                        true,
                        order + 1,
                        vec![off, note, 0x40],
                    ));
                }
            }
            0x80 | 0xa0 | 0xb0 | 0xe0 => {
                let data = reader.take(2)?;
                timed.push((
                    now,
                    status & 0xf0 == 0x80,
                    order,
                    vec![status, data[0], data[1]],
                ));
            }
            0xc0 | 0xd0 => {
                let data = reader.byte()?;
                timed.push((now, false, order, vec![status, data]));
            }
            _ => match status {
                0xf0 | 0xf7 => {
                    let length = reader.number()?;
                    let data =
                        reader.take(usize::try_from(length).map_err(|_| XmiError::Truncated)?)?;
                    let mut bytes = vec![status];
                    push_number(&mut bytes, length);
                    bytes.extend_from_slice(data);
                    timed.push((now, false, order, bytes));
                }
                0xff => {
                    let kind = reader.byte()?;
                    let length = reader.number()?;
                    let data =
                        reader.take(usize::try_from(length).map_err(|_| XmiError::Truncated)?)?;
                    match kind {
                        // The end comes after the last note's release.
                        0x2f => break,
                        // XMI's own tempo is not the file's clock [I].
                        0x51 => {}
                        _ => {
                            let mut bytes = vec![0xff, kind];
                            push_number(&mut bytes, length);
                            bytes.extend_from_slice(data);
                            timed.push((now, false, order, bytes));
                        }
                    }
                }
                other => return Err(XmiError::Event(other)),
            },
        }
    }
    timed.sort_by_key(|(time, off, order, _)| (*time, !*off, *order));
    Ok(timed)
}

/// The file's sequences, each as a Standard MIDI file (format 0).
///
/// # Errors
/// Refuses a file that is not an XMI, that ends early, or that holds an
/// event MIDI doesn't have.
pub fn to_midi(bytes: &[u8]) -> Result<Vec<Vec<u8>>, XmiError> {
    event_chunks(bytes)?
        .into_iter()
        .map(|body| {
            let timed = events(body)?;
            let mut track = Vec::new();
            // The tempo that keeps 120 ticks a second.
            track.extend_from_slice(&[0x00, 0xff, 0x51, 0x03]);
            track.extend_from_slice(&TEMPO.to_be_bytes()[1..]);
            let mut last = 0_u64;
            for (time, _, _, bytes) in timed {
                push_number(
                    &mut track,
                    u32::try_from(time - last).unwrap_or(u32::MAX >> 4),
                );
                track.extend_from_slice(&bytes);
                last = time;
            }
            track.extend_from_slice(&[0x00, 0xff, 0x2f, 0x00]);
            if track.len() > MAX_MIDI {
                return Err(XmiError::TooLarge);
            }
            let mut file = Vec::with_capacity(track.len() + 22);
            file.extend_from_slice(b"MThd");
            file.extend_from_slice(&6_u32.to_be_bytes());
            file.extend_from_slice(&0_u16.to_be_bytes());
            file.extend_from_slice(&1_u16.to_be_bytes());
            file.extend_from_slice(&DIVISION.to_be_bytes());
            file.extend_from_slice(b"MTrk");
            file.extend_from_slice(
                &u32::try_from(track.len())
                    .map_err(|_| XmiError::Truncated)?
                    .to_be_bytes(),
            );
            file.extend_from_slice(&track);
            Ok(file)
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn chunk(tag: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    /// An XMI of the sequences given as `EVNT` bodies.
    pub(crate) fn xmi(sequences: &[&[u8]]) -> Vec<u8> {
        let count = u16::try_from(sequences.len()).unwrap();
        let mut dir = b"XDIR".to_vec();
        dir.extend(chunk(*b"INFO", &count.to_le_bytes()));
        let mut cat = b"XMID".to_vec();
        for sequence in sequences {
            let mut form = b"XMID".to_vec();
            form.extend(chunk(*b"TIMB", &[0, 0]));
            form.extend(chunk(*b"EVNT", sequence));
            cat.extend(chunk(*b"FORM", &form));
        }
        [chunk(*b"FORM", &dir), chunk(*b"CAT ", &cat)].concat()
    }

    /// Program 5 on channel 0; a note of 30 ticks at once; 10 ticks on, a
    /// second note of 60 (a 0x7f run and 0x0b); a tempo; the end.
    pub(crate) const TUNE: &[u8] = &[
        0xc0, 0x05, // program
        0x90, 0x3c, 0x64, 0x1e, // note 60, length 30
        0x0a, // wait 10
        0x90, 0x40, 0x50, 0x3c, // note 64, length 60
        0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, // XMI tempo, dropped
        0x7f, 0x0b, // wait 138
        0xff, 0x2f, 0x00, // end
    ];

    #[test]
    fn a_sequence_becomes_a_standard_midi_file_with_its_note_offs() {
        let files = to_midi(&xmi(&[TUNE, &[0x90, 0x30, 0x00, 0x00]])).unwrap();
        assert_eq!(files.len(), 2);
        let file = &files[0];
        assert_eq!(&file[..4], b"MThd");
        assert_eq!(&file[12..14], &60_u16.to_be_bytes());
        let track = &file[22..];
        #[rustfmt::skip]
        let expected: &[u8] = &[
            0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, // 500,000 us
            0x00, 0xc0, 0x05,
            0x00, 0x90, 0x3c, 0x64,
            0x0a, 0x90, 0x40, 0x50, // at 10
            0x14, 0x80, 0x3c, 0x40, // at 30
            0x28, 0x80, 0x40, 0x40, // at 70
            0x00, 0xff, 0x2f, 0x00,
        ];
        assert_eq!(track, expected);
        assert_eq!(
            u32::from_be_bytes(file[18..22].try_into().unwrap()) as usize,
            expected.len()
        );
        // A zero-velocity note-on makes no extra note-off.
        assert_eq!(
            &files[1][22 + 7..],
            &[0x00, 0x90, 0x30, 0x00, 0x00, 0xff, 0x2f, 0x00]
        );
    }

    #[test]
    fn a_note_off_goes_before_a_note_on_at_the_same_time() {
        let files = to_midi(&xmi(&[&[
            0x90, 0x3c, 0x64, 0x0a, 0x0a, 0x91, 0x3c, 0x64, 0x01,
        ]]))
        .unwrap();
        let track = &files[0][22 + 7..];
        assert_eq!(
            track,
            &[
                0x00, 0x90, 0x3c, 0x64, 0x0a, 0x80, 0x3c, 0x40, 0x00, 0x91, 0x3c, 0x64, 0x01, 0x81,
                0x3c, 0x40, 0x00, 0xff, 0x2f, 0x00
            ]
        );
    }

    #[test]
    fn broken_files_are_refused_and_never_panic() {
        let whole = xmi(&[TUNE]);
        assert_eq!(to_midi(b"RIFF"), Err(XmiError::Truncated));
        assert_eq!(to_midi(&chunk(*b"JUNK", &[1, 2])), Err(XmiError::NotXmi));
        for end in 0..whole.len() {
            let _ = to_midi(&whole[..end]);
        }
        assert_eq!(to_midi(&xmi(&[&[0xf4]])), Err(XmiError::Event(0xf4)));
        assert_eq!(to_midi(&xmi(&[&[0x90, 0x3c]])), Err(XmiError::Truncated));
        let many: Vec<&[u8]> = vec![&[0xff, 0x2f, 0x00]; MAX_SEQUENCES + 1];
        assert_eq!(to_midi(&xmi(&many)), Err(XmiError::TooLarge));
        let long = [0xc0, 0x05].repeat(MAX_EVENTS + 1);
        assert_eq!(to_midi(&xmi(&[&long])), Err(XmiError::TooLarge));
    }

    #[test]
    fn numbers_round_trip() {
        for value in [0, 0x7f, 0x80, 0x3fff, 0x4000, 0x0fff_ffff] {
            let mut bytes = Vec::new();
            push_number(&mut bytes, value);
            assert_eq!(Reader::new(&bytes).number().unwrap(), value);
        }
    }
}
