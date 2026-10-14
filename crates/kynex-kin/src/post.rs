//! A robot program as any vendor's controller is given it: its moves, and
//! between them what the robot does — outputs set, inputs waited for,
//! pauses, a pallet's count moved on — with nothing of any vendor's
//! language in it. A vendor's writer (see [`Writer`]; URScript in
//! [`crate::urscript`]) turns it into that vendor's program.
//!
//! Digital I/O is numbered as KYNEX numbers it: 0–7 a controller's
//! standard outputs and inputs, 8–15 its configurable ones, 16 and 17 its
//! tool's. A writer maps those onto its own.

use kreate_math::DVec3;

use crate::path::Waypoint;
use crate::{Kinematics, Tool};

/// What the program does between two moves.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// A comment, for what cannot be said in the program yet.
    Note(String),
    /// Digital output `port` on or off.
    Set { port: u32, on: bool },
    /// Digital output `port` on for `seconds`, then off again: a command to
    /// a machine.
    Pulse { port: u32, seconds: f64 },
    /// Wait until digital input `port` is on (or off).
    Wait { port: u32, on: bool },
    /// Wait this many seconds.
    Sleep(f64),
    /// Move pallet `pallet`'s count (by its index in [`Export::pallets`])
    /// on to its next place, from its last back to its first.
    Next { pallet: usize },
}

impl From<&str> for Op {
    fn from(note: &str) -> Self {
        Op::Note(note.to_string())
    }
}

/// Where a pallet's places are, in the controller's base frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Places {
    pub name: String,
    pub at: Vec<DVec3>,
}

/// A move's target on a pallet: taught over place `from` of pallet
/// `pallet`, it goes as far from the place the pallet's count has reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub pallet: usize,
    pub from: usize,
}

/// One move of the program: the point it reaches, by its name, and what
/// the program does before it.
pub struct Move<'a> {
    pub name: &'a str,
    pub waypoint: &'a Waypoint,
    pub before: Vec<Op>,
    /// The pallet place it goes over, if it is on a pallet.
    pub place: Option<Place>,
}

/// A whole program, to be written for a controller.
pub struct Export<'a> {
    /// The program's name; each writer makes an identifier of it.
    pub name: &'a str,
    pub moves: Vec<Move<'a>>,
    /// Round again forever, back to the first point after the last.
    pub close: bool,
    /// The controller's base frame, by identity: the robot's `base` link.
    pub base: u64,
    /// The flange the tool point is set from (`tool0`).
    pub flange: u64,
    /// What the program does after its last move.
    pub trailing: Vec<Op>,
    /// The pallets its points count through.
    pub pallets: Vec<Places>,
    /// Number the moves on output registers as they start and as each is
    /// done, for timing them on a controller or a simulator (see
    /// `tools/ursim`), where the writer can.
    pub markers: bool,
}

/// A vendor's writer: a program in its controller's language.
pub trait Writer {
    /// The vendor, as the page names it.
    fn vendor(&self) -> &'static str;
    /// The program file's extension.
    fn extension(&self) -> &'static str;
    /// The program, or why it cannot be one.
    fn write(&self, kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String>;
}

/// `text` safe in a comment: ASCII, and no control character — a name with
/// a line break in it would otherwise end the comment and put its own
/// program into the robot's.
pub fn comment(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .collect()
}

/// An identifier from `name`: letters, digits and `_`, starting with a
/// letter.
pub fn identifier(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        out.insert_str(0, "kynex_");
    }
    out
}
