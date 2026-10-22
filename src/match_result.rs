//! Sport-agnostic match result model and its canonical TLV encoding.
//!
//! The binary format is a simple TLV: 1-byte tag, 2-byte big-endian length,
//! then the value. Nested structures (teams, score) are themselves TLV
//! sequences carried as the value of their parent field. Unknown tags are
//! ignored so newer clients remain compatible with older firmware.
//!
//! See docs/APDU.md for the authoritative specification.

use alloc::string::String;
use alloc::vec::Vec;
use core::str;

use crate::utils::format_timestamp;
use crate::AppSW;

/// Maximum accepted size in bytes of a complete serialized match result.
pub const MAX_MATCH_LEN: usize = 3072;
/// Maximum byte length of a team or player name (UTF-8).
pub const MAX_NAME_LEN: usize = 32;
/// Maximum number of players per team (starters + substitutes).
pub const MAX_PLAYERS: usize = 24;
/// Maximum number of score periods (sets, quarters, innings, ...).
pub const MAX_PERIODS: usize = 16;

// Top-level tags.
const TAG_VERSION: u8 = 0x01;
const TAG_TIMESTAMP: u8 = 0x02;
const TAG_SPORT: u8 = 0x03;
const TAG_WINNER: u8 = 0x04;
const TAG_LOSER: u8 = 0x05;
const TAG_SCORE: u8 = 0x06;

const FORMAT_VERSION: u8 = 0x01;

/// A team that took part in the match.
#[derive(Default)]
pub struct Team {
    pub name: String,
    pub players: Vec<String>,
}

/// A parsed match result.
pub struct MatchResult {
    /// Match date/time as a Unix timestamp (displayed in UTC).
    pub timestamp: u64,
    /// Optional sport identifier, e.g. "volleyball", "soccer", "tennis".
    pub sport: Option<String>,
    pub winner: Team,
    pub loser: Team,
    /// Score as (winner points, loser points) per period. A single period is
    /// used for sports with one final score (soccer, basketball...); multiple
    /// periods model sets/quarters/innings (volleyball, tennis, baseball...).
    pub periods: Vec<(u32, u32)>,
}

impl MatchResult {
    /// "25-10 25-14 15-25 25-22" (one "w-l" group per period).
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

    /// Match date as "YYYY-MM-DD HH:MM UTC".
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

/// Team value: repeated fields, tag 0x01 = team name (once), 0x02 = one player.
fn parse_team(data: &[u8]) -> Result<Team, AppSW> {
    let mut team = Team::default();
    let mut have_name = false;
    let mut reader = TlvReader::new(data);
    while let Some((tag, value)) = reader.next()? {
        match tag {
            0x01 => {
                team.name = parse_name(value)?;
                have_name = true;
            }
            0x02 => {
                if team.players.len() >= MAX_PLAYERS {
                    return Err(AppSW::ParsingFail);
                }
                team.players.push(parse_name(value)?);
            }
            _ => {} // forward compatibility
        }
    }
    if !have_name {
        return Err(AppSW::ParsingFail);
    }
    Ok(team)
}

/// Score value: repeated fields, tag 0x01 = one period (8 bytes: two BE u32).
fn parse_score(data: &[u8]) -> Result<Vec<(u32, u32)>, AppSW> {
    let mut periods: Vec<(u32, u32)> = Vec::new();
    let mut reader = TlvReader::new(data);
    while let Some((tag, value)) = reader.next()? {
        match tag {
            0x01 => {
                if value.len() != 8 || periods.len() >= MAX_PERIODS {
                    return Err(AppSW::ParsingFail);
                }
                let w = u32::from_be_bytes(value[0..4].try_into().unwrap());
                let l = u32::from_be_bytes(value[4..8].try_into().unwrap());
                periods.push((w, l));
            }
            _ => {}
        }
    }
    if periods.is_empty() {
        return Err(AppSW::ParsingFail);
    }
    Ok(periods)
}

/// Parse a complete serialized match result.
pub fn parse(data: &[u8]) -> Result<MatchResult, AppSW> {
    let mut timestamp: Option<u64> = None;
    let mut sport: Option<String> = None;
    let mut winner: Option<Team> = None;
    let mut loser: Option<Team> = None;
    let mut periods: Option<Vec<(u32, u32)>> = None;

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
                timestamp = Some(u64::from_be_bytes(value.try_into().unwrap()));
            }
            TAG_SPORT => sport = Some(parse_name(value)?),
            TAG_WINNER => winner = Some(parse_team(value)?),
            TAG_LOSER => loser = Some(parse_team(value)?),
            TAG_SCORE => periods = Some(parse_score(value)?),
            _ => {} // forward compatibility
        }
    }

    Ok(MatchResult {
        timestamp: timestamp.ok_or(AppSW::ParsingFail)?,
        sport,
        winner: winner.ok_or(AppSW::ParsingFail)?,
        loser: loser.ok_or(AppSW::ParsingFail)?,
        periods: periods.ok_or(AppSW::ParsingFail)?,
    })
}
