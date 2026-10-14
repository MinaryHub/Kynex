//! A program as URScript, for a Universal Robots controller.
//!
//! Joint moves go as `movej` with the taught joint positions, exactly;
//! linear moves as `movel` to the taught tool pose, which the controller
//! solves for itself. Poses are in the controller's base frame — the
//! robot's `base` link in `ur_description`, which is turned half a turn
//! from `base_link` — and the tool point is set with `set_tcp` from the
//! flange (`tool0`).
//!
//! Speeds: a joint move's share of top speed becomes `v` on the slowest
//! joint's top speed, and its acceleration follows
//! [`path::JOINT_ACCELERATION`]; a linear move's speed is its own, at
//! [`path::LINEAR_ACCELERATION`]. The controller times its moves in its own
//! way, so a cycle on the robot is close to the planned one, not equal.

use std::fmt::Write;

use kreate_math::{DAffine3, DQuat};
use kreate_scene::robot::JointKind;

use crate::path::{self, MoveKind, Waypoint};
use crate::{Kinematics, Tool};

/// A line the program has between its moves: script, or a comment for
/// what the script cannot say yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Note(String),
    /// URScript, its own lines indented within it as they go.
    Code(String),
}

impl From<&str> for Line {
    fn from(note: &str) -> Self {
        Line::Note(note.to_string())
    }
}

/// One move of the program: the point it reaches, by its name, and what
/// the program does before it.
pub struct Move<'a> {
    pub name: &'a str,
    pub waypoint: &'a Waypoint,
    pub notes: Vec<Line>,
    /// A pose the move's target is moved by, as a URScript expression —
    /// a pallet's place, counted in the script — or none.
    pub offset: Option<String>,
}

pub struct Export<'a> {
    /// The script's function: letters, digits and `_` are kept.
    pub name: &'a str,
    pub moves: Vec<Move<'a>>,
    /// Round again forever, back to the first point after the last.
    pub close: bool,
    /// The controller's base frame, by identity: the `base` link.
    pub base: u64,
    /// The flange the tool point is set from (`tool0`).
    pub flange: u64,
    /// What the program does after its last move.
    pub trailing: Vec<Line>,
    /// Lines at the top of the function: what the moves and lines use.
    pub globals: Vec<String>,
    /// Number the moves on output integer register 0 as they start — 1,
    /// 2, … and one past the last at the end — for timing them on a
    /// controller or URSim (see `tools/ursim`).
    pub markers: bool,
}

/// The program as a URScript file, or why it cannot be one.
pub fn urscript(kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
    let turns = kin
        .vars
        .iter()
        .all(|v| matches!(v.kind, JointKind::Revolute | JointKind::Continuous));
    if kin.vars.len() != 6 || !turns {
        return Err("URScript는 회전 관절 6개인 UR 로봇만 만들 수 있습니다".into());
    }
    if export.moves.is_empty() {
        return Err("프로그램에 지점이 없습니다".into());
    }
    let any = &export.moves[0].waypoint.positions;
    let base = kin.world_of(any, export.base).ok_or("no base frame")?;
    let flange = kin.world_of(any, export.flange).ok_or("no flange")?;
    // The tool point from the flange: the same whatever the pose, both
    // being on the last link.
    let tcp = flange.inverse() * kin.pose(any, tool);
    let slowest = kin
        .vars
        .iter()
        .map(|v| v.velocity)
        .fold(f64::INFINITY, f64::min);

    let name = function_name(export.name);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Made by KYNEX from program \"{}\".",
        ascii(export.name)
    );
    let _ = writeln!(
        out,
        "# Poses in the base frame, metres and rotation vectors."
    );
    let _ = writeln!(out, "def {name}():");
    let _ = writeln!(out, "  set_tcp({})", pose(&tcp));
    for g in &export.globals {
        // One line each: nothing smuggled in on another.
        if g.contains(['\n', '\r']) {
            return Err("URScript: a global spans lines".into());
        }
        let _ = writeln!(out, "  {g}");
    }
    let notes = |out: &mut String, indent: &str, notes: &[Line]| {
        for n in notes {
            match n {
                Line::Note(n) => {
                    let _ = writeln!(out, "{indent}# {}", ascii(n));
                }
                Line::Code(code) => {
                    for line in code.lines() {
                        let _ = writeln!(out, "{indent}{line}");
                    }
                }
            }
        }
    };
    let mut mark = 0;
    let mut marker = |out: &mut String, indent: &str| {
        if export.markers {
            mark += 1;
            let _ = writeln!(out, "{indent}write_output_integer_register(0, {mark})");
        }
    };
    let step = |out: &mut String, indent: &str, m: &Move, kind: MoveKind| {
        let w = m.waypoint;
        let _ = writeln!(out, "{indent}# {}", ascii(m.name));
        // Passed without stopping, within the blend radius.
        let r = if w.blend > 0.0 {
            format!(", r={:.4}", w.blend)
        } else {
            String::new()
        };
        // A pose, moved by the move's offset if it has one.
        let target = |q: &[f64]| {
            let p = pose(&(base.inverse() * kin.pose(q, tool)));
            match &m.offset {
                Some(o) => format!("pose_add({p}, {o})"),
                None => p,
            }
        };
        match kind {
            MoveKind::Joint => {
                let v = w.speed.clamp(0.01, 1.0) * slowest;
                // On a pallet, to a pose the controller solves for: the
                // joints taught are for one place only.
                let to = if m.offset.is_some() {
                    target(&w.positions)
                } else {
                    joints(&w.positions)
                };
                let _ = writeln!(
                    out,
                    "{indent}movej({to}, a={:.4}, v={:.4}{r})",
                    path::JOINT_ACCELERATION * slowest,
                    v
                );
            }
            MoveKind::Linear => {
                let _ = writeln!(
                    out,
                    "{indent}movel({}, a={:.4}, v={:.4}{r})",
                    target(&w.positions),
                    path::LINEAR_ACCELERATION,
                    w.speed
                );
            }
            MoveKind::Circular => {
                // Without its via point, as a line.
                match &w.via {
                    Some(via) => {
                        let _ = writeln!(
                            out,
                            "{indent}movec({}, {}, a={:.4}, v={:.4}{r}, mode=0)",
                            target(via),
                            target(&w.positions),
                            path::LINEAR_ACCELERATION,
                            w.speed
                        );
                    }
                    None => {
                        let _ = writeln!(
                            out,
                            "{indent}movel({}, a={:.4}, v={:.4}{r})",
                            target(&w.positions),
                            path::LINEAR_ACCELERATION,
                            w.speed
                        );
                    }
                }
            }
        }
    };
    // Wherever the robot stands, it goes to the first point by its joints:
    // a line from an unknown pose could go anywhere.
    let first = &export.moves[0];
    notes(&mut out, "  ", &first.notes);
    let _ = writeln!(out, "  # To the start");
    marker(&mut out, "  ");
    step(&mut out, "  ", first, MoveKind::Joint);
    let rest = &export.moves[1..];
    if export.close {
        let _ = writeln!(out, "  while (True):");
        for m in rest {
            notes(&mut out, "    ", &m.notes);
            marker(&mut out, "    ");
            step(&mut out, "    ", m, m.waypoint.kind);
        }
        notes(&mut out, "    ", &export.trailing);
        // Round again: what came before the first move comes again too.
        notes(&mut out, "    ", &first.notes);
        marker(&mut out, "    ");
        step(&mut out, "    ", first, first.waypoint.kind);
        let _ = writeln!(out, "  end");
    } else {
        for m in rest {
            notes(&mut out, "  ", &m.notes);
            marker(&mut out, "  ");
            step(&mut out, "  ", m, m.waypoint.kind);
        }
        notes(&mut out, "  ", &export.trailing);
        marker(&mut out, "  ");
    }
    let _ = writeln!(out, "end");
    let _ = writeln!(out, "{name}()");
    Ok(out)
}

/// `p[x, y, z, rx, ry, rz]`: metres, and the rotation as a vector.
fn pose(m: &DAffine3) -> String {
    let (_, r, t) = m.to_scale_rotation_translation();
    let v = rotation_vector(r);
    format!(
        "p[{:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6}]",
        t.x, t.y, t.z, v.x, v.y, v.z
    )
}

fn rotation_vector(r: DQuat) -> kreate_math::DVec3 {
    let mut r = r.normalize();
    // The short way round: an angle of at most π.
    if r.w < 0.0 {
        r = -r;
    }
    let (axis, angle) = r.to_axis_angle();
    axis * angle
}

fn joints(q: &[f64]) -> String {
    let list: Vec<String> = q.iter().map(|x| format!("{x:.6}")).collect();
    format!("[{}]", list.join(", "))
}

/// A URScript identifier: letters, digits and `_`, starting with a letter.
fn function_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        out.insert_str(0, "kynex_");
    }
    out
}

/// Text for a comment: the controller reads ASCII.
/// `text` safe in a comment: ASCII, and no control character — a name with
/// a line break in it would otherwise end the comment and put its own
/// script into the program.
pub fn comment(text: &str) -> String {
    ascii(text)
}

fn ascii(text: &str) -> String {
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

#[cfg(test)]
mod tests;
