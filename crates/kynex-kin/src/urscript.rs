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

/// One move of the program: the point it reaches, by its name, and what
/// the program does before it that the script cannot say yet, as comments.
pub struct Move<'a> {
    pub name: &'a str,
    pub waypoint: &'a Waypoint,
    pub notes: Vec<String>,
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
    /// What the program does after its last move, as comments.
    pub trailing: Vec<String>,
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
    let notes = |out: &mut String, indent: &str, notes: &[String]| {
        for n in notes {
            let _ = writeln!(out, "{indent}# {}", ascii(n));
        }
    };
    let step = |out: &mut String, indent: &str, m: &Move, kind: MoveKind| {
        let w = m.waypoint;
        let _ = writeln!(out, "{indent}# {}", ascii(m.name));
        match kind {
            MoveKind::Joint => {
                let v = w.speed.clamp(0.01, 1.0) * slowest;
                let _ = writeln!(
                    out,
                    "{indent}movej({}, a={:.4}, v={:.4})",
                    joints(&w.positions),
                    path::JOINT_ACCELERATION * slowest,
                    v
                );
            }
            MoveKind::Linear => {
                let at = base.inverse() * kin.pose(&w.positions, tool);
                let _ = writeln!(
                    out,
                    "{indent}movel({}, a={:.4}, v={:.4})",
                    pose(&at),
                    path::LINEAR_ACCELERATION,
                    w.speed
                );
            }
        }
    };
    // Wherever the robot stands, it goes to the first point by its joints:
    // a line from an unknown pose could go anywhere.
    let first = &export.moves[0];
    notes(&mut out, "  ", &first.notes);
    let _ = writeln!(out, "  # To the start");
    step(&mut out, "  ", first, MoveKind::Joint);
    let rest = &export.moves[1..];
    if export.close {
        let _ = writeln!(out, "  while (True):");
        for m in rest {
            notes(&mut out, "    ", &m.notes);
            step(&mut out, "    ", m, m.waypoint.kind);
        }
        notes(&mut out, "    ", &export.trailing);
        // Round again: what came before the first move comes again too.
        notes(&mut out, "    ", &first.notes);
        step(&mut out, "    ", first, first.waypoint.kind);
        let _ = writeln!(out, "  end");
    } else {
        for m in rest {
            notes(&mut out, "  ", &m.notes);
            step(&mut out, "  ", m, m.waypoint.kind);
        }
        notes(&mut out, "  ", &export.trailing);
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
