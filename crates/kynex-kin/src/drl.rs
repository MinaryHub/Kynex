//! A program as DRL, for a Doosan Robotics controller: the writer for
//! Doosan (see [`crate::post`]). DRL is Python with the controller's
//! commands.
//!
//! - Joint moves go as `movej` to the taught joint positions, exactly, in
//!   degrees; each joint at its share of its own top speed, and at
//!   [`path::JOINT_ACCELERATION`] times that top speed, as KYNEX plans
//!   them.
//! - Linear and circular moves go as `movel` and `movec` to the taught tool
//!   pose in the base frame (`DR_BASE`): millimetres, and ZYZ Euler angles
//!   in degrees, as the controller writes poses. Their speed is their own,
//!   at [`path::LINEAR_ACCELERATION`]; a blend is a `radius`.
//! - The tool point is the TCP `KYNEX`, chosen with `set_tcp`: DRL cannot
//!   make one, so the program says, at its top, where it is from the flange,
//!   to be registered on the teach pendant (the emulator's is registered
//!   over the API, see `tools/doosan`).
//! - I/O: KYNEX's 0–15 are the controller's digital I/O 1–16, 16 and 17 the
//!   tool's 1 and 2.
//! - Pallet places are a list each, their count beside them; a point on a
//!   pallet is moved by `kynex_shift`, and a joint move to one goes as
//!   `movejx` to the moved pose, in the posture the point was taught in.
//! - Markers, for timing on the emulator: `tp_log("kynex mark k")` as move
//!   `k` starts, and `tp_log("kynex done k …")` as it is done, with the
//!   joints and the tool pose the controller has then.

use std::collections::BTreeSet;
use std::fmt::Write;

use kreate_math::{DAffine3, DMat3};
use kreate_scene::robot::JointKind;

use crate::path::{self, MoveKind};
use crate::post::{comment, Export, Move, Op, Place, Writer};
use crate::{Kinematics, Tool};

/// Doosan Robotics: DRL.
pub struct Doosan;

impl Writer for Doosan {
    fn vendor(&self) -> &'static str {
        "Doosan Robotics"
    }

    fn extension(&self) -> &'static str {
        "drl"
    }

    fn write(&self, kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
        drl(kin, tool, export)
    }
}

/// `v` to `digits` places, with no sign on a zero.
fn fixed(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s,
    }
}

/// The ZYZ Euler angles of `r`, radians: `r = Rz(a) · Ry(b) · Rz(c)`, `b`
/// in [0, π]; at `b` = 0 or π, where only `a ± c` tells, `c` is 0.
pub fn zyz(r: DMat3) -> [f64; 3] {
    let m = |row: usize, col: usize| r.col(col)[row];
    let b = m(2, 2).clamp(-1.0, 1.0).acos();
    if b.sin() > 1e-9 {
        [m(1, 2).atan2(m(0, 2)), b, m(2, 1).atan2(-m(2, 0))]
    } else if m(2, 2) > 0.0 {
        [m(1, 0).atan2(m(0, 0)), 0.0, 0.0]
    } else {
        [(-m(1, 0)).atan2(-m(0, 0)), std::f64::consts::PI, 0.0]
    }
}

/// `posx(x, y, z, a, b, c)`: millimetres and degrees.
fn posx(pose: &DAffine3) -> String {
    let t = pose.translation * 1000.0;
    let [a, b, c] = zyz(pose.matrix3);
    let parts: Vec<String> = [t.x, t.y, t.z]
        .map(|v| fixed(v, 3))
        .into_iter()
        .chain([a, b, c].map(|v| fixed(v.to_degrees(), 4)))
        .collect();
    format!("posx({})", parts.join(", "))
}

/// `posj(q1, …, q6)`: radians as degrees.
fn posj(q: &[f64]) -> String {
    let deg: Vec<String> = q.iter().map(|x| fixed(x.to_degrees(), 4)).collect();
    format!("posj({})", deg.join(", "))
}

/// DRL setting digital output `n`.
fn set_out(n: u32, on: bool) -> String {
    let on = if on { "ON" } else { "OFF" };
    match n {
        0..=15 => format!("set_digital_output({}, {on})", n + 1),
        _ => format!("set_tool_digital_output({}, {on})", n - 15),
    }
}

/// DRL waiting for digital input `n` to read `on`.
fn wait_in(n: u32, on: bool) -> String {
    let on = if on { "ON" } else { "OFF" };
    match n {
        0..=15 => format!("wait_digital_input({}, {on})", n + 1),
        _ => format!("wait_tool_digital_input({}, {on})", n - 15),
    }
}

/// Writes the program as DRL, or says why it cannot be.
pub fn drl(kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
    let turns = kin
        .vars
        .iter()
        .all(|v| matches!(v.kind, JointKind::Revolute | JointKind::Continuous));
    if kin.vars.len() != 6 || !turns {
        return Err("DRL은 회전 관절 6개인 로봇만 만들 수 있습니다".into());
    }
    if export.moves.is_empty() {
        return Err("프로그램에 지점이 없습니다".into());
    }
    let any = &export.moves[0].waypoint.positions;
    let base = kin.world_of(any, export.base).ok_or("no base frame")?;
    let flange = kin.world_of(any, export.flange).ok_or("no flange")?;
    let tcp = flange.inverse() * kin.pose(any, tool);
    let in_base = |q: &[f64]| base.inverse() * kin.pose(q, tool);
    // Degrees a second: each joint's top speed.
    let tops: Vec<f64> = kin.vars.iter().map(|v| v.velocity.to_degrees()).collect();
    let list = |f: &dyn Fn(f64) -> f64| {
        let v: Vec<String> = tops.iter().map(|t| fixed(f(*t), 2)).collect();
        format!("[{}]", v.join(", "))
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Made by KYNEX from program \"{}\", for a Doosan Robotics controller.",
        comment(export.name)
    );
    let _ = writeln!(
        out,
        "# Poses in the base frame (DR_BASE): millimetres, and ZYZ Euler angles in degrees."
    );
    let mut outputs: BTreeSet<u32> = BTreeSet::new();
    let mut inputs: BTreeSet<u32> = BTreeSet::new();
    for op in export
        .moves
        .iter()
        .flat_map(|m| m.before.iter())
        .chain(&export.trailing)
    {
        match op {
            Op::Set { port, .. } | Op::Pulse { port, .. } => {
                outputs.insert(*port);
            }
            Op::Wait { port, .. } => {
                inputs.insert(*port);
            }
            _ => {}
        }
    }
    if !outputs.is_empty() || !inputs.is_empty() {
        let list = |set: &BTreeSet<u32>| {
            set.iter()
                .map(|n| match n {
                    0..=15 => format!("{}", n + 1),
                    _ => format!("tool {}", n - 15),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !outputs.is_empty() {
            let _ = writeln!(out, "# Digital outputs used: {}.", list(&outputs));
        }
        if !inputs.is_empty() {
            let _ = writeln!(out, "# Digital inputs used: {}.", list(&inputs));
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "# TCP \"KYNEX\", to register on the teach pendant: {}",
        posx(&tcp)
    );
    let _ = writeln!(out, "set_tcp(\"KYNEX\")");

    // The pallets' places, each one's count, and a pose moved from one place
    // to another.
    if !export.pallets.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "def kynex_shift(p, places, to, frm):");
        let _ = writeln!(
            out,
            "    return posx(p[0] + places[to][0] - places[frm][0], p[1] + places[to][1] - places[frm][1], p[2] + places[to][2] - places[frm][2], p[3], p[4], p[5])"
        );
    }
    for (k, pallet) in export.pallets.iter().enumerate() {
        let places: Vec<String> = pallet
            .at
            .iter()
            .map(|p| {
                let [x, y, z] = (*p * 1000.0).to_array().map(|v| fixed(v, 3));
                format!("[{x}, {y}, {z}]")
            })
            .collect();
        let _ = writeln!(out, "# The places of {}", comment(&pallet.name));
        let _ = writeln!(out, "kynex_places_{k} = [{}]", places.join(", "));
        let _ = writeln!(out, "kynex_place_{k} = 0");
    }

    // Every move's target, named by its place in the program (see
    // `crate::rapid`): by its joints, and as a pose where it is moved to one.
    let _ = writeln!(out);
    for (k, m) in export.moves.iter().enumerate() {
        let w = m.waypoint;
        let n = k + 1;
        let _ = writeln!(out, "# {}", comment(m.name));
        let _ = writeln!(out, "j{n} = {}", posj(&w.positions));
        if !(w.kind == MoveKind::Joint && m.place.is_none()) {
            let _ = writeln!(out, "p{n} = {}", posx(&in_base(&w.positions)));
            if let Some(via) = w.via.as_ref().filter(|_| w.kind == MoveKind::Circular) {
                let _ = writeln!(out, "v{n} = {}", posx(&in_base(via)));
            }
        }
    }
    let _ = writeln!(out);

    let mut mark = 0;
    let mut body = |out: &mut String, indent: &str, k: usize, m: &Move, kind: MoveKind| {
        for op in &m.before {
            for line in op_lines(op, export) {
                let _ = writeln!(out, "{indent}{line}");
            }
        }
        let w = m.waypoint;
        let n = k + 1;
        if export.markers {
            mark += 1;
            let _ = writeln!(out, "{indent}tp_log(\"kynex mark {mark}\")");
        }
        let _ = writeln!(out, "{indent}# {}", comment(m.name));
        let radius = if w.blend > 0.0 {
            format!(", radius={}", fixed(w.blend * 1000.0, 1))
        } else {
            String::new()
        };
        // On a pallet: moved by how far the place counted to is from the
        // place it was taught over.
        let target = |name: String| match m.place {
            Some(Place { pallet: p, from }) => {
                format!("kynex_shift({name}, kynex_places_{p}, kynex_place_{p}, {from})")
            }
            None => name,
        };
        let share = w.speed.clamp(0.01, 1.0);
        let joint = format!(
            "vel={}, acc={}",
            list(&|top| share * top),
            list(&|top| path::JOINT_ACCELERATION * top)
        );
        let line = format!(
            "vel=[{}, {}], acc=[{}, {}]",
            fixed(w.speed * 1000.0, 1),
            fixed(path::ANGULAR_SPEED.to_degrees(), 2),
            fixed(path::LINEAR_ACCELERATION * 1000.0, 1),
            fixed(path::ANGULAR_ACCELERATION.to_degrees(), 2)
        );
        let _ = match kind {
            MoveKind::Joint if m.place.is_none() => {
                writeln!(out, "{indent}movej(j{n}, {joint}{radius})")
            }
            MoveKind::Joint => writeln!(
                out,
                "{indent}movejx({}, {joint}{radius}, sol=get_solution_space(j{n}))",
                target(format!("p{n}"))
            ),
            MoveKind::Linear => writeln!(
                out,
                "{indent}movel({}, {line}{radius}, ref=DR_BASE)",
                target(format!("p{n}"))
            ),
            MoveKind::Circular if w.via.is_some() => writeln!(
                out,
                "{indent}movec({}, {}, {line}{radius}, ref=DR_BASE)",
                target(format!("v{n}")),
                target(format!("p{n}"))
            ),
            MoveKind::Circular => writeln!(
                out,
                "{indent}movel({}, {line}{radius}, ref=DR_BASE)",
                target(format!("p{n}"))
            ),
        };
        if export.markers {
            let _ = writeln!(
                out,
                "{indent}tp_log(\"kynex done {mark} %s %s\" % (get_current_posj(), get_current_posx(ref=DR_BASE)[0]))"
            );
        }
    };
    // Wherever the robot stands, it goes to the first point by its joints.
    let first = &export.moves[0];
    let _ = writeln!(out, "# To the start");
    let start = Move {
        name: first.name,
        waypoint: first.waypoint,
        before: first.before.clone(),
        place: None,
    };
    body(&mut out, "", 0, &start, MoveKind::Joint);
    let rest = &export.moves[1..];
    let indent = if export.close {
        let _ = writeln!(out, "while True:");
        "    "
    } else {
        ""
    };
    for (k, m) in rest.iter().enumerate() {
        body(&mut out, indent, k + 1, m, m.waypoint.kind);
    }
    for op in &export.trailing {
        for line in op_lines(op, export) {
            let _ = writeln!(out, "{indent}{line}");
        }
    }
    if export.close {
        body(&mut out, indent, 0, first, first.waypoint.kind);
    } else if export.markers {
        mark += 1;
        let _ = writeln!(out, "tp_log(\"kynex mark {mark}\")");
    }
    Ok(out)
}

/// `op` as DRL lines.
fn op_lines(op: &Op, export: &Export) -> Vec<String> {
    match op {
        Op::Note(n) => vec![format!("# {}", comment(n))],
        Op::Set { port, on } => vec![set_out(*port, *on)],
        Op::Pulse { port, seconds } => vec![
            set_out(*port, true),
            format!("wait({seconds})"),
            set_out(*port, false),
        ],
        Op::Wait { port, on } => vec![wait_in(*port, *on)],
        Op::Sleep(seconds) => vec![format!("wait({seconds:.2})")],
        Op::Next { pallet } => {
            let n = export.pallets.get(*pallet).map_or(1, |p| p.at.len().max(1));
            vec![format!(
                "kynex_place_{pallet} = (kynex_place_{pallet} + 1) % {n}"
            )]
        }
    }
}

#[cfg(test)]
mod tests;
