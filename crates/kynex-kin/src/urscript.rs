//! A program as URScript, for a Universal Robots controller: the writer
//! for Universal Robots (see [`crate::post`]).
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
//!
//! I/O: KYNEX's 0–7 are the standard digital I/O, 8–15 the configurable,
//! 16 and 17 the tool's. Pallet places are listed at the top, each
//! pallet's count beside them; a point on a pallet is moved by
//! `pose_add`.

use std::fmt::Write;

use kreate_math::{DAffine3, DQuat};

use crate::path::{self, MoveKind};
pub use crate::post::{comment, Export, Move, Op, Place, Places};
use crate::post::{identifier, Writer};
use crate::{Kinematics, Tool};

/// Universal Robots: URScript.
pub struct Ur;

impl Writer for Ur {
    fn vendor(&self) -> &'static str {
        "Universal Robots"
    }

    fn extension(&self) -> &'static str {
        "script"
    }

    fn write(&self, kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
        urscript(kin, tool, export)
    }
}

/// URScript setting digital output `n`.
fn set_out(n: u32, on: bool) -> String {
    let on = if on { "True" } else { "False" };
    match n {
        0..=7 => format!("set_standard_digital_out({n}, {on})"),
        8..=15 => format!("set_configurable_digital_out({}, {on})", n - 8),
        _ => format!("set_tool_digital_out({}, {on})", n - 16),
    }
}

/// URScript reading digital input `n`.
fn get_in(n: u32) -> String {
    match n {
        0..=7 => format!("get_standard_digital_in({n})"),
        8..=15 => format!("get_configurable_digital_in({})", n - 8),
        _ => format!("get_tool_digital_in({})", n - 16),
    }
}

/// `op` as URScript lines.
fn op_lines(op: &Op, pallets: &[Places]) -> Vec<String> {
    match op {
        Op::Note(n) => vec![format!("# {}", comment(n))],
        // A UR has no axis to turn it with: the positioner's own
        // controller does.
        Op::Turn { unit, degrees, .. } => vec![format!(
            "# KYNEX: turn positioner {} to {degrees:.1} degrees (its own controller)",
            comment(unit)
        )],
        Op::Set { port, on } => vec![set_out(*port, *on)],
        Op::Pulse { port, seconds } => vec![
            set_out(*port, true),
            format!("sleep({seconds})"),
            set_out(*port, false),
        ],
        Op::Wait { port, on } => vec![
            if *on {
                format!("while (not {}):", get_in(*port))
            } else {
                format!("while ({}):", get_in(*port))
            },
            "  sync()".to_string(),
            "end".to_string(),
        ],
        Op::Sleep(seconds) => vec![format!("sleep({seconds:.2})")],
        Op::Next { pallet } => {
            let n = pallets.get(*pallet).map_or(1, |p| p.at.len().max(1));
            vec![format!(
                "kynex_place_{pallet} = (kynex_place_{pallet} + 1) % {n}"
            )]
        }
    }
}

/// The program as a URScript file, or why it cannot be one.
pub fn urscript(kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
    // An axis outside the arm — a track — is no axis this controller has.
    let (_, outside) = crate::post::axes(kin)
        .map_err(|_| "URScript는 회전 관절 6개인 UR 로봇만 만들 수 있습니다".to_string())?;
    if !outside.is_empty() {
        return Err(
            "URScript: 외부축(주행 트랙 등)이 있는 로봇은 RAPID(ABB)로만 내보낼 수 있습니다".into(),
        );
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

    let name = identifier(export.name);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Made by KYNEX from program \"{}\".",
        comment(export.name)
    );
    let _ = writeln!(
        out,
        "# Poses in the base frame, metres and rotation vectors."
    );
    let _ = writeln!(out, "def {name}():");
    let _ = writeln!(out, "  set_tcp({})", pose(&tcp));
    // The pallets' places, and each one's count.
    for (k, pallet) in export.pallets.iter().enumerate() {
        let places: Vec<String> = pallet
            .at
            .iter()
            .map(|p| format!("p[{:.6}, {:.6}, {:.6}, 0, 0, 0]", p.x, p.y, p.z))
            .collect();
        let _ = writeln!(out, "  # The places of {}", comment(&pallet.name));
        let _ = writeln!(out, "  kynex_places_{k} = [{}]", places.join(", "));
        let _ = writeln!(out, "  kynex_place_{k} = 0");
    }
    let notes = |out: &mut String, indent: &str, ops: &[Op]| {
        for op in ops {
            for line in op_lines(op, &export.pallets) {
                let _ = writeln!(out, "{indent}{line}");
            }
        }
    };
    let mark = 0;
    // Each move's number on register 0 as it starts, and again on
    // register 1 once it is done: the time between is the move's own,
    // whatever the program does after it.
    let mark = std::cell::Cell::new(mark);
    let marker = |out: &mut String, indent: &str| {
        if export.markers {
            mark.set(mark.get() + 1);
            let _ = writeln!(
                out,
                "{indent}write_output_integer_register(0, {})",
                mark.get()
            );
        }
    };
    let ended = |out: &mut String, indent: &str| {
        if export.markers {
            let _ = writeln!(
                out,
                "{indent}write_output_integer_register(1, {})",
                mark.get()
            );
        }
    };
    let step = |out: &mut String, indent: &str, m: &Move, kind: MoveKind| {
        let w = m.waypoint;
        let _ = writeln!(out, "{indent}# {}", comment(m.name));
        // Passed without stopping, within the blend radius.
        let r = if w.blend > 0.0 {
            format!(", r={:.4}", w.blend)
        } else {
            String::new()
        };
        // A pose, moved to the pallet place counted to if it is on one.
        let target = |q: &[f64]| {
            let p = pose(&(base.inverse() * kin.pose(q, tool)));
            match m.place {
                Some(Place { pallet: k, from }) => format!(
                    "pose_add({p}, pose_sub(kynex_places_{k}[kynex_place_{k}], kynex_places_{k}[{from}]))"
                ),
                None => p,
            }
        };
        match kind {
            MoveKind::Joint => {
                let v = w.speed.clamp(0.01, 1.0) * slowest;
                // On a pallet, to a pose the controller solves for — the
                // joints taught are for one place only — from the joints
                // taught, as KYNEX solves it: left to itself, the
                // controller takes the solution nearest wherever the arm
                // happens to be, which may be another posture altogether.
                let to = if m.place.is_some() {
                    format!(
                        "get_inverse_kin({}, qnear={})",
                        target(&w.positions),
                        joints(&w.positions)
                    )
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
    notes(&mut out, "  ", &first.before);
    let _ = writeln!(out, "  # To the start");
    marker(&mut out, "  ");
    step(&mut out, "  ", first, MoveKind::Joint);
    ended(&mut out, "  ");
    let rest = &export.moves[1..];
    if export.close {
        let _ = writeln!(out, "  while (True):");
        for m in rest {
            notes(&mut out, "    ", &m.before);
            marker(&mut out, "    ");
            step(&mut out, "    ", m, m.waypoint.kind);
            ended(&mut out, "    ");
        }
        notes(&mut out, "    ", &export.trailing);
        // Round again: what came before the first move comes again too.
        notes(&mut out, "    ", &first.before);
        marker(&mut out, "    ");
        step(&mut out, "    ", first, first.waypoint.kind);
        ended(&mut out, "    ");
        let _ = writeln!(out, "  end");
    } else {
        for m in rest {
            notes(&mut out, "  ", &m.before);
            marker(&mut out, "  ");
            step(&mut out, "  ", m, m.waypoint.kind);
            ended(&mut out, "  ");
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

#[cfg(test)]
mod tests;
