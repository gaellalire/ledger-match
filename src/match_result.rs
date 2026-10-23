//! Activity-agnostic competition result model and its canonical TLV encoding (v2).
//!
//! Two layouts share one format:
//! - **Versus**: two sides (winner/loser), each side being an optional name
//!   plus an optional player list; a shared score (per period) is optional.
//! - **Free-for-all**: a flat list of participants, each with an optional
//!   individual score (tarot, bridge, races...).
//!
//! The binary format is TLV: 1-byte tag, 2-byte big-endian length, value.
//! Nested blocks (score, referees, player lists, participants) are themselves
//! TLV sequences carried as the value of their parent field. Unknown tags are
//! ignored so newer clients remain compatible with older firmware.
//!
//! See docs/APDU.md for the authoritative specification.

use alloc::string::String;
use alloc::vec::Vec;
use core::str;

use crate::utils::format_timestamp;
use crate::AppSW;

/// Maximum accepted size in bytes of a complete serialized result.
pub const MAX_MATCH_LEN: usize = 3072;
/// Maximum byte length of a name or free-text field (UTF-8).
pub const MAX_NAME_LEN: usize = 32;
/// Maximum items in any list (players per side, participants, referees).
pub const MAX_LIST_ITEMS: usize = 24;
/// Maximum number of score periods (sets, quarters, innings, ...).
pub const MAX_PERIODS: usize = 16;

// Top-level tags.
const TAG_VERSION: u8 = 0x01;
const TAG_ACTIVITY: u8 = 0x02;
const TAG_TIMESTAMP: u8 = 0x03;
const TAG_LOCATION: u8 = 0x04;
const TAG_COMPETITION: u8 = 0x05;
const TAG_WINNER: u8 = 0x06;
const TAG_LOSER: u8 = 0x07;
const TAG_SCORE: u8 = 0x08;
const TAG_REFEREES: u8 = 0x09;
const TAG_WINNER_PLAYERS: u8 = 0x0A;
const TAG_LOSER_PLAYERS: u8 = 0x0B;
const TAG_PARTICIPANTS: u8 = 0x0C;

const FORMAT_VERSION: u8 = 0x02;

/// A free-for-all participant with an optional individual score.
pub struct Participant {
    pub name: String,
    pub score: Option<i32>,
}

/// Which layout a result uses.
#[derive(PartialEq)]
pub enum Layout {
    Versus,
    FreeForAll,
}

/// A parsed result.
#[derive(Default)]
pub struct MatchResult {
    /// Optional activity name, e.g. "volleyball", "tarot".
    pub activity: Option<String>,
    /// Date/time as a Unix timestamp (displayed in UTC).
    pub timestamp: u64,
    pub location: Option<String>,
    /// Optional event context, e.g. "Roland Garros 2026 — QF".
    pub competition: Option<String>,
    /// Side names; both optional (tennis doubles) or both absent (draw).
    pub winner: Option<String>,
    pub loser: Option<String>,
    /// Score as (winner points, loser points) per period; empty = no score.
    /// Points are signed (card games, golf can go negative).
    pub periods: Vec<(i32, i32)>,
    pub referees: Vec<String>,
    pub winner_players: Vec<String>,
    pub loser_players: Vec<String>,
    pub participants: Vec<Participant>,
}

impl MatchResult {
    pub fn layout(&self) -> Layout {
        if self.participants.is_empty() {
            Layout::Versus
        } else {
            Layout::FreeForAll
        }
    }

    /// "25-10 25-14" (one "w-l" group per period).
    pub fn score_string(&self) -> String {
        let mut s = String::new();
        for (i, (w, l)) in self.periods.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&alloc::format!("{}-{}", w, l));
        }
        s
    }

    /// Result date as "YYYY-MM-DD HH:MM UTC".
    pub fn date_string(&self) -> String {
        format_timestamp(self.timestamp)
    }
}

/// Cursor over a TLV byte sequence.
struct TlvReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TlvReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        TlvReader { data, pos: 0 }
    }

    /// Read the next (tag, value) field, or None at end of input.
    fn next(&mut self) -> Result<Option<(u8, &'a [u8])>, AppSW> {
        if self.pos == self.data.len() {
            return Ok(None);
        }
        if self.data.len() - self.pos < 3 {
            return Err(AppSW::ParsingFail);
        }
        let tag = self.data[self.pos];
        let len = u16::from_be_bytes([self.data[self.pos + 1], self.data[self.pos + 2]]) as usize;
        let start = self.pos + 3;
        let end = start.checked_add(len).ok_or(AppSW::ParsingFail)?;
        if end > self.data.len() {
            return Err(AppSW::ParsingFail);
        }
        self.pos = end;
        Ok(Some((tag, &self.data[start..end])))
    }
}

fn parse_name(data: &[u8]) -> Result<String, AppSW> {
    if data.is_empty() || data.len() > MAX_NAME_LEN {
        return Err(AppSW::ParsingFail);
    }
    let s = str::from_utf8(data).map_err(|_| AppSW::ParsingFail)?;
    Ok(String::from(s))
}

/// Name list block: repeated fields, tag 0x01 = one name.
fn parse_name_list(data: &[u8]) -> Result<Vec<String>, AppSW> {
    let mut names = Vec::new();
    let mut reader = TlvReader::new(data);
    while let Some((tag, value)) = reader.next()? {
        if tag == 0x01 {
            if names.len() >= MAX_LIST_ITEMS {
                return Err(AppSW::ParsingFail);
            }
            names.push(parse_name(value)?);
        }
    }
    Ok(names)
}

/// Score block: repeated fields, tag 0x01 = one period (8 bytes: two BE i32).
fn parse_score(data: &[u8]) -> Result<Vec<(i32, i32)>, AppSW> {
    let mut periods: Vec<(i32, i32)> = Vec::new();
    let mut reader = TlvReader::new(data);
    while let Some((tag, value)) = reader.next()? {
        if tag == 0x01 {
            if value.len() != 8 || periods.len() >= MAX_PERIODS {
                return Err(AppSW::ParsingFail);
            }
            let w = i32::from_be_bytes(value[0..4].try_into().unwrap());
            let l = i32::from_be_bytes(value[4..8].try_into().unwrap());
            periods.push((w, l));
        }
    }
    Ok(periods)
}

/// Participants block: repeated fields, tag 0x01 = one participant, itself a
/// TLV sequence (0x01 name required, 0x02 score optional as 4-byte BE i32).
fn parse_participants(data: &[u8]) -> Result<Vec<Participant>, AppSW> {
    let mut participants: Vec<Participant> = Vec::new();
    let mut reader = TlvReader::new(data);
    while let Some((tag, value)) = reader.next()? {
        if tag == 0x01 {
            if participants.len() >= MAX_LIST_ITEMS {
                return Err(AppSW::ParsingFail);
            }
            let mut name: Option<String> = None;
            let mut score: Option<i32> = None;
            let mut inner = TlvReader::new(value);
            while let Some((itag, ivalue)) = inner.next()? {
                match itag {
                    0x01 => name = Some(parse_name(ivalue)?),
                    0x02 => {
                        if ivalue.len() != 4 {
                            return Err(AppSW::ParsingFail);
                        }
                        score = Some(i32::from_be_bytes(ivalue.try_into().unwrap()));
                    }
                    _ => {} // forward compatibility
                }
            }
            participants.push(Participant {
                name: name.ok_or(AppSW::ParsingFail)?,
                score,
            });
        }
    }
    Ok(participants)
}

/// Parse a complete serialized result.
pub fn parse(data: &[u8]) -> Result<MatchResult, AppSW> {
    let mut result = MatchResult::default();
    let mut have_timestamp = false;

    let mut reader = TlvReader::new(data);
    while let Some((tag, value)) = reader.next()? {
        match tag {
            TAG_VERSION => {
                if value != [FORMAT_VERSION] {
                    return Err(AppSW::ParsingFail);
                }
            }
            TAG_TIMESTAMP => {
                if value.len() != 8 {
                    return Err(AppSW::ParsingFail);
                }
                result.timestamp = u64::from_be_bytes(value.try_into().unwrap());
                have_timestamp = true;
            }
            TAG_ACTIVITY => result.activity = Some(parse_name(value)?),
            TAG_LOCATION => result.location = Some(parse_name(value)?),
            TAG_COMPETITION => result.competition = Some(parse_name(value)?),
            TAG_WINNER => result.winner = Some(parse_name(value)?),
            TAG_LOSER => result.loser = Some(parse_name(value)?),
            TAG_SCORE => result.periods = parse_score(value)?,
            TAG_REFEREES => result.referees = parse_name_list(value)?,
            TAG_WINNER_PLAYERS => result.winner_players = parse_name_list(value)?,
            TAG_LOSER_PLAYERS => result.loser_players = parse_name_list(value)?,
            TAG_PARTICIPANTS => result.participants = parse_participants(value)?,
            _ => {} // forward compatibility
        }
    }

    if !have_timestamp {
        return Err(AppSW::ParsingFail);
    }

    match result.layout() {
        Layout::FreeForAll => {
            // Versus-only fields must not be mixed in.
            if result.winner.is_some()
                || result.loser.is_some()
                || !result.periods.is_empty()
                || !result.winner_players.is_empty()
                || !result.loser_players.is_empty()
            {
                return Err(AppSW::ParsingFail);
            }
        }
        Layout::Versus => {
            // At least one side designation (name or players) must exist.
            if result.winner.is_none()
                && result.loser.is_none()
                && result.winner_players.is_empty()
                && result.loser_players.is_empty()
            {
                return Err(AppSW::ParsingFail);
            }
        }
    }

    Ok(result)
}
