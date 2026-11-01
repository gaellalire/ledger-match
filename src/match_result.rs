//! Activity-agnostic game result model and its canonical TLV encoding (v1).
//!
//! Two layouts share one format:
//! - **Versus**: two sides (side 1/side 2), each side being an optional name
//!   plus an optional player list; a shared score (per period) is optional.
//!   The outcome (win/draw) is deduced from the score; forfeits are carried
//!   by an explicit field.
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
const TAG_SIDE1: u8 = 0x06;
const TAG_SIDE2: u8 = 0x07;
const TAG_SCORE: u8 = 0x08;
const TAG_REFEREES: u8 = 0x09;
const TAG_SIDE1_PLAYERS: u8 = 0x0A;
const TAG_SIDE2_PLAYERS: u8 = 0x0B;
const TAG_PARTICIPANTS: u8 = 0x0C;
const TAG_FORFEIT: u8 = 0x0D;

const FORMAT_VERSION: u8 = 0x01;

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

/// Which side(s) forfeited (explicit field, tag 0x0D).
#[derive(Clone, Copy, PartialEq)]
pub enum Forfeit {
    Side1 = 0x01,
    Side2 = 0x02,
    Both = 0x03,
}

/// Outcome of a versus game, deduced from the forfeit field and the score.
#[derive(PartialEq)]
pub enum Outcome {
    Side1Wins,
    Side2Wins,
    Draw,
    Side1WinsByForfeit,
    Side2WinsByForfeit,
    BothForfeited,
    /// No forfeit and no score: nothing to deduce.
    Unknown,
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
    /// Side names; both optional (tennis doubles).
    pub side1: Option<String>,
    pub side2: Option<String>,
    /// Score as (side 1 points, side 2 points) per period; empty = no score.
    /// Points are signed (card games, golf can go negative).
    pub periods: Vec<(i32, i32)>,
    pub referees: Vec<String>,
    pub side1_players: Vec<String>,
    pub side2_players: Vec<String>,
    pub participants: Vec<Participant>,
    /// Which side(s) forfeited, if any.
    pub forfeit: Option<Forfeit>,
}

impl MatchResult {
    pub fn layout(&self) -> Layout {
        if self.participants.is_empty() {
            Layout::Versus
        } else {
            Layout::FreeForAll
        }
    }

    /// Deduce the outcome of a versus game.
    ///
    /// A forfeit field takes precedence. Otherwise the score decides: the
    /// side winning more periods wins; on a tie, the side with the higher
    /// point total wins; if both are tied the game is a draw. Sports decided
    /// by total points must therefore be encoded as a single period.
    pub fn outcome(&self) -> Outcome {
        match self.forfeit {
            Some(Forfeit::Side1) => return Outcome::Side2WinsByForfeit,
            Some(Forfeit::Side2) => return Outcome::Side1WinsByForfeit,
            Some(Forfeit::Both) => return Outcome::BothForfeited,
            None => {}
        }
        if self.periods.is_empty() {
            return Outcome::Unknown;
        }
        let periods1 = self.periods.iter().filter(|(a, b)| a > b).count();
        let periods2 = self.periods.iter().filter(|(a, b)| b > a).count();
        if periods1 != periods2 {
            return if periods1 > periods2 {
                Outcome::Side1Wins
            } else {
                Outcome::Side2Wins
            };
        }
        let total1: i64 = self.periods.iter().map(|(a, _)| *a as i64).sum();
        let total2: i64 = self.periods.iter().map(|(_, b)| *b as i64).sum();
        if total1 > total2 {
            Outcome::Side1Wins
        } else if total2 > total1 {
            Outcome::Side2Wins
        } else {
            Outcome::Draw
        }
    }

    /// "25-10 25-14" (one "side1-side2" group per period).
    pub fn score_string(&self) -> String {
        let mut s = String::new();
        for (i, (a, b)) in self.periods.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&alloc::format!("{}-{}", a, b));
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
            let a = i32::from_be_bytes(value[0..4].try_into().unwrap());
            let b = i32::from_be_bytes(value[4..8].try_into().unwrap());
            periods.push((a, b));
        }
    }
    Ok(periods)
}

/// Forfeit field: 1 byte, which side(s) forfeited.
fn parse_forfeit(data: &[u8]) -> Result<Forfeit, AppSW> {
    match data {
        [0x01] => Ok(Forfeit::Side1),
        [0x02] => Ok(Forfeit::Side2),
        [0x03] => Ok(Forfeit::Both),
        _ => Err(AppSW::ParsingFail),
    }
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
            TAG_SIDE1 => result.side1 = Some(parse_name(value)?),
            TAG_SIDE2 => result.side2 = Some(parse_name(value)?),
            TAG_SCORE => result.periods = parse_score(value)?,
            TAG_REFEREES => result.referees = parse_name_list(value)?,
            TAG_SIDE1_PLAYERS => result.side1_players = parse_name_list(value)?,
            TAG_SIDE2_PLAYERS => result.side2_players = parse_name_list(value)?,
            TAG_PARTICIPANTS => result.participants = parse_participants(value)?,
            TAG_FORFEIT => result.forfeit = Some(parse_forfeit(value)?),
            _ => {} // forward compatibility
        }
    }

    if !have_timestamp {
        return Err(AppSW::ParsingFail);
    }

    match result.layout() {
        Layout::FreeForAll => {
            // Versus-only fields must not be mixed in.
            if result.side1.is_some()
                || result.side2.is_some()
                || !result.periods.is_empty()
                || !result.side1_players.is_empty()
                || !result.side2_players.is_empty()
                || result.forfeit.is_some()
            {
                return Err(AppSW::ParsingFail);
            }
        }
        Layout::Versus => {
            // At least one side designation (name or players) must exist.
            if result.side1.is_none()
                && result.side2.is_none()
                && result.side1_players.is_empty()
                && result.side2_players.is_empty()
            {
                return Err(AppSW::ParsingFail);
            }
        }
    }

    Ok(result)
}
