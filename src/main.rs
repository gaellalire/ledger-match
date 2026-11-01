//! ledger-match: a Ledger app that signs game results ("Game Signer").
//!
//! The device receives an activity-agnostic, TLV-encoded game result
//! (date, sides, score, optional forfeit or winner flag), displays it for clear signing,
//! and returns a secp256k1 signature of its Keccak-256 hash.
//!
//! See docs/APDU.md for the full protocol specification.

#![no_std]
#![no_main]

extern crate alloc;

mod handlers;
mod match_result;
mod ui;
mod utils;

use handlers::{
    handler_get_public_key, handler_get_version, handler_sign_match, SignContext,
};
use ledger_device_sdk::io::{self, init_comm, ApduHeader, Command, Comm, Reply, StatusWords};
use ledger_device_sdk::nbgl::{NbglReviewStatus, StatusType};

ledger_device_sdk::set_panic!(ledger_device_sdk::exiting_panic);

ledger_device_sdk::define_comm!(COMM);

/// APDU class byte accepted by this app.
const CLA: u8 = 0xE0;

// P2 for last APDU to receive.
const P2_SIGN_LAST: u8 = 0x00;
// P2 for more APDU to receive.
const P2_SIGN_MORE: u8 = 0x80;
// P1 for first APDU number (carries the BIP32 path).
const P1_SIGN_START: u8 = 0x00;
// P1 for maximum APDU number (payload chunks are capped by MAX_MATCH_LEN anyway).
const P1_SIGN_MAX: u8 = 0x0F;

/// Application status words.
#[repr(u16)]
#[derive(Clone, Copy, PartialEq)]
pub enum AppSW {
    Deny = 0x6985,
    WrongP1P2 = 0x6A86,
    InsNotSupported = 0x6D00,
    ClaNotSupported = 0x6E00,
    CommError = 0x6F00,
    DisplayFail = 0xB001,
    WrongLength = 0xB004,
    ParsingFail = 0xB005,
    HashFail = 0xB006,
    SignFail = 0xB008,
    KeyDeriveFail = 0xB009,
    VersionParsingFail = 0xB00A,
    WrongApduLength = StatusWords::BadLen as u16,
    Ok = 0x9000,
}

impl From<AppSW> for Reply {
    fn from(sw: AppSW) -> Reply {
        Reply(sw as u16)
    }
}

impl From<io::CommError> for AppSW {
    fn from(_e: io::CommError) -> Self {
        AppSW::CommError
    }
}

/// Possible input commands received through APDUs.
#[derive(Debug)]
pub enum Instruction {
    GetVersion,
    GetAppName,
    GetPubkey { display: bool },
    SignMatch { chunk: u8, more: bool },
}

impl TryFrom<ApduHeader> for Instruction {
    type Error = AppSW;

    /// APDU parsing logic.
    ///
    /// Parses INS, P1 and P2 bytes to build an [`Instruction`]. P1 and P2 are
    /// translated to strongly typed variables depending on the APDU instruction
    /// code. Invalid INS, P1 or P2 values result in errors with a status word,
    /// which are automatically sent to the host by the SDK.
    ///
    /// Note that CLA is not checked here. Instead the method
    /// [`Comm::set_expected_cla`] is used in [`sample_main`] to have this
    /// verification automatically performed by the SDK.
    fn try_from(value: ApduHeader) -> Result<Self, Self::Error> {
        match (value.ins, value.p1, value.p2) {
            (3, 0, 0) => Ok(Instruction::GetVersion),
            (4, 0, 0) => Ok(Instruction::GetAppName),
            (5, 0 | 1, 0) => Ok(Instruction::GetPubkey {
                display: value.p1 != 0,
            }),
            (6, P1_SIGN_START, P2_SIGN_MORE)
            | (6, 1..=P1_SIGN_MAX, P2_SIGN_LAST | P2_SIGN_MORE) => Ok(Instruction::SignMatch {
                chunk: value.p1,
                more: value.p2 == P2_SIGN_MORE,
            }),
            (3..=6, _, _) => Err(AppSW::WrongP1P2),
            (_, _, _) => Err(AppSW::InsNotSupported),
        }
    }
}

/// Application entry point.
#[no_mangle]
extern "C" fn sample_main() {
    // Create the communication manager, and configure it to accept only
    // APDUs from the 0xE0 class. If any APDU with a wrong class value is
    // received, comm responds automatically with a BadCla status word.
    let comm = init_comm(&COMM);
    comm.set_expected_cla(CLA);

    let mut ctx = SignContext::new();
    ctx.home = ui::ui_menu_main(comm);
    ctx.home.show_and_return();

    loop {
        let command = comm.next_command();
        let decoded = command.decode::<Instruction>();
        let Ok(ins) = decoded else {
            let _ = comm.send(&[], decoded.unwrap_err());
            continue;
        };

        let status = match handle_apdu(command, &ins, &mut ctx) {
            Ok(reply) => {
                let _ = reply.send(AppSW::Ok);
                AppSW::Ok
            }
            Err(sw) => {
                let _ = comm.send(&[], sw);
                sw
            }
        };

        show_status_and_home_if_needed(comm, &ins, &mut ctx, &status);
    }
}

fn handle_apdu<'a>(
    command: Command<'a>,
    ins: &Instruction,
    ctx: &mut SignContext,
) -> Result<io::CommandResponse<'a>, AppSW> {
    match ins {
        Instruction::GetAppName => {
            let mut response = command.into_response();
            response.append(env!("CARGO_PKG_NAME").as_bytes())?;
            Ok(response)
        }
        Instruction::GetVersion => handler_get_version(command),
        Instruction::GetPubkey { display } => handler_get_public_key(command, *display),
        Instruction::SignMatch { chunk, more } => handler_sign_match(command, *chunk, *more, ctx),
    }
}

/// After a finished review (address or match signing), show the
/// success/failure status screen and return to the home screen.
fn show_status_and_home_if_needed(
    comm: &mut Comm,
    ins: &Instruction,
    ctx: &mut SignContext,
    status: &AppSW,
) {
    let (show_status, status_type) = match (ins, status) {
        (Instruction::GetPubkey { display: true }, AppSW::Deny | AppSW::Ok) => {
            (true, StatusType::Address)
        }
        (Instruction::SignMatch { .. }, AppSW::Deny | AppSW::Ok) if ctx.finished() => {
            (true, StatusType::Transaction)
        }
        (_, _) => (false, StatusType::Transaction),
    };

    if show_status {
        NbglReviewStatus::new()
            .status_type(status_type)
            .show(comm, *status == AppSW::Ok);
        ctx.home.show_and_return();
    }
}
