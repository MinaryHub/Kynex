//! A program as RAPID, for an ABB controller: the writer for ABB (see
//! [`crate::post`]).
//!
//! - Joint moves go as `MoveAbsJ` to the taught joint positions, exactly;
//!   their share of the joints' top speed as `VelSet` around them.
//! - Linear and circular moves go as `MoveL` and `MoveC` to the taught
//!   tool pose in the base frame (`wobj0`), millimetres and quaternions,
//!   with `ConfL \Off`: the controller keeps the posture it is in, as a
//!   line does. Their speed is a `speeddata` of their own, millimetres a
//!   second; a blend radius a `zonedata`, `fine` with none.
//! - The tool point is a `tooldata` from the flange (`tool0`).
//! - I/O: KYNEX's digital output `n` is the signal `DO_KYNEX_n`, input
//!   `n` is `DI_KYNEX_n`; the signals the program uses are listed at its
//!   top, to be set up on the controller. A command to a machine is a
//!   `PulseDO`.
//! - Pallet places are a `pos` array each, their count beside them; a
//!   point on a pallet is moved by `Offs`, and a joint move to one goes as
//!   `MoveJ` to the moved pose, the posture left to the controller.
//! - Markers, for timing on a virtual controller: each move's number in
//!   `kynex_mark` as it starts and in `kynex_done` as it is done.

use std::collections::BTreeSet;
use std::fmt::Write;

use kreate_math::{DAffine3, DQuat, DVec3};
use kreate_scene::robot::JointKind;

use crate::path::MoveKind;
use crate::post::{comment, identifier, Export, Move, Op, Place, Writer};
use crate::{Kinematics, Tool};

/// ABB: RAPID.
pub struct Abb;

impl Writer for Abb {
    fn vendor(&self) -> &'static str {
        "ABB"
    }

    fn extension(&self) -> &'static str {
        "mod"
    }

    fn write(&self, kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
        rapid(kin, tool, export)
    }
}

/// A RAPID name: an identifier of at most 32 characters.
fn rapid_name(name: &str) -> String {
    identifier(name).chars().take(32).collect()
}

/// `v` to `digits` places, with no sign on a zero.
fn fixed(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s,
    }
}

/// `[x,y,z]` in millimetres.
fn mm(p: DVec3) -> String {
    let [x, y, z] = (p * 1000.0).to_array().map(|v| fixed(v, 2));
    format!("[{x},{y},{z}]")
}

/// `[q1,q2,q3,q4]`: ABB's quaternion, its scalar first.
fn quaternion(r: DQuat) -> String {
    // One of the two that turn alike: its first part not written as
    // negative, as the controller writes it — the tool down is [0,0,1,0].
    let mut r = r.normalize();
    let first = [r.w, r.x, r.y, r.z]
        .into_iter()
        .find(|v| v.abs() >= 5e-7)
        .unwrap_or(1.0);
    if first < 0.0 {
        r = -r;
    }
    let [w, x, y, z] = [r.w, r.x, r.y, r.z].map(|v| fixed(v, 6));
    format!("[{w},{x},{y},{z}]")
}

/// A `robtarget` at `pose`, its posture left to the controller.
fn robtarget(pose: &DAffine3) -> String {
    let (_, r, t) = pose.to_scale_rotation_translation();
    format!(
        "[{},{},[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]]",
        mm(t),
        quaternion(r)
    )
}

/// A `jointtarget` at `q`, radians as degrees.
fn jointtarget(q: &[f64]) -> String {
    let deg: Vec<String> = q.iter().map(|x| fixed(x.to_degrees(), 4)).collect();
    format!("[[{}],[9E9,9E9,9E9,9E9,9E9,9E9]]", deg.join(","))
}

/// Writes the program as a RAPID module, or says why it cannot be one.
pub fn rapid(kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
    let turns = kin
        .vars
        .iter()
        .all(|v| matches!(v.kind, JointKind::Revolute | JointKind::Continuous));
    if kin.vars.len() != 6 || !turns {
        return Err("RAPID는 회전 관절 6개인 로봇만 만들 수 있습니다".into());
    }
    if export.moves.is_empty() {
        return Err("프로그램에 지점이 없습니다".into());
    }
    let any = &export.moves[0].waypoint.positions;
    let base = kin.world_of(any, export.base).ok_or("no base frame")?;
    let flange = kin.world_of(any, export.flange).ok_or("no flange")?;
    let tcp = flange.inverse() * kin.pose(any, tool);
    let in_base = |q: &[f64]| base.inverse() * kin.pose(q, tool);

    // Every move's target, named by its place in the program: a routine's
    // points come round again, and names repeat among routines.
    let mut targets = String::new();
    let mut speeds: BTreeSet<u32> = BTreeSet::new();
    let mut zones: BTreeSet<u32> = BTreeSet::new();
    let mut outputs: BTreeSet<u32> = BTreeSet::new();
    let mut inputs: BTreeSet<u32> = BTreeSet::new();
    let ops = export
        .moves
        .iter()
        .flat_map(|m| m.before.iter())
        .chain(&export.trailing);
    for op in ops {
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
    for (k, m) in export.moves.iter().enumerate() {
        let w = m.waypoint;
        let _ = writeln!(targets, "  ! {}", comment(m.name));
        // By its joints: a joint move, unless it goes to a pallet place, and
        // the first, which the robot goes to by its joints from wherever it
        // stands. As a pose: every other move, and the first again if it
        // comes round again in a loop by its own kind.
        let by_pose = !(w.kind == MoveKind::Joint && m.place.is_none());
        if k == 0 || !by_pose {
            let _ = writeln!(
                targets,
                "  CONST jointtarget j{} := {};",
                k + 1,
                jointtarget(&w.positions)
            );
        }
        if by_pose {
            let _ = writeln!(
                targets,
                "  CONST robtarget p{} := {};",
                k + 1,
                robtarget(&in_base(&w.positions))
            );
            if let Some(via) = w.via.as_ref().filter(|_| w.kind == MoveKind::Circular) {
                let _ = writeln!(
                    targets,
                    "  CONST robtarget v{} := {};",
                    k + 1,
                    robtarget(&in_base(via))
                );
            }
            if w.kind != MoveKind::Joint {
                speeds.insert((w.speed * 1000.0).round().max(1.0) as u32);
            }
        }
        if w.blend > 0.0 {
            zones.insert((w.blend * 1000.0).round().max(1.0) as u32);
        }
    }

    let module = rapid_name(&format!("KYNEX_{}", export.name));
    let mut out = String::new();
    let _ = writeln!(out, "MODULE {module}");
    let _ = writeln!(
        out,
        "  ! Made by KYNEX from program \"{}\".",
        comment(export.name)
    );
    let _ = writeln!(
        out,
        "  ! Targets in the base frame (wobj0), millimetres and quaternions."
    );
    if !outputs.is_empty() || !inputs.is_empty() {
        let _ = writeln!(out, "  ! Signals to set up on the controller:");
        for n in &outputs {
            let _ = writeln!(out, "  !   DO_KYNEX_{n}: digital output");
        }
        for n in &inputs {
            let _ = writeln!(out, "  !   DI_KYNEX_{n}: digital input");
        }
    }
    let (_, tr, tt) = tcp.to_scale_rotation_translation();
    let _ = writeln!(
        out,
        "  PERS tooldata kynex_tool := [TRUE,[{},{}],[1,[0,0,1],[1,0,0,0],0,0,0]];",
        mm(tt),
        quaternion(tr)
    );
    for v in &speeds {
        let _ = writeln!(out, "  CONST speeddata kynex_v{v} := [{v},500,5000,1000];");
    }
    for z in &zones {
        let r = *z as f64;
        let _ = writeln!(
            out,
            "  CONST zonedata kynex_z{z} := [FALSE,{r},{},{},{},{},{}];",
            r * 1.5,
            r * 1.5,
            r * 0.15,
            r * 1.5,
            r * 0.15
        );
    }
    for (k, pallet) in export.pallets.iter().enumerate() {
        let places: Vec<String> = pallet.at.iter().map(|p| mm(*p)).collect();
        let _ = writeln!(out, "  ! The places of {}", comment(&pallet.name));
        let _ = writeln!(
            out,
            "  CONST pos kynex_places_{k}{{{}}} := [{}];",
            places.len(),
            places.join(",")
        );
        let _ = writeln!(out, "  VAR num kynex_place_{k} := 0;");
    }
    if export.markers {
        let _ = writeln!(out, "  PERS num kynex_mark := 0;");
        let _ = writeln!(out, "  PERS num kynex_done := 0;");
    }
    out.push_str(&targets);
    let _ = writeln!(out);
    let _ = writeln!(out, "  PROC main()");
    let _ = writeln!(out, "    ConfJ \\Off;");
    let _ = writeln!(out, "    ConfL \\Off;");

    let mut velset: Option<u32> = None;
    let mut mark = 0;
    let mut body = |out: &mut String, indent: &str, k: usize, m: &Move, kind: MoveKind| {
        for op in &m.before {
            op_lines(out, indent, op, export);
        }
        let w = m.waypoint;
        let zone = if w.blend > 0.0 {
            format!("kynex_z{}", (w.blend * 1000.0).round().max(1.0) as u32)
        } else {
            "fine".to_string()
        };
        if export.markers {
            mark += 1;
            let _ = writeln!(out, "{indent}kynex_mark := {mark};");
        }
        let _ = writeln!(out, "{indent}! {}", comment(m.name));
        // A joint move's share of top speed, and full speed for a line.
        let share = match kind {
            MoveKind::Joint => (w.speed.clamp(0.01, 1.0) * 100.0).round() as u32,
            _ => 100,
        };
        if velset != Some(share) {
            let _ = writeln!(out, "{indent}VelSet {share}, 5000;");
            velset = Some(share);
        }
        // On a pallet: moved by how far the place counted to is from the
        // place it was taught over, a coordinate at a time (RAPID arrays
        // count from 1).
        let target = |name: String| match m.place {
            Some(Place { pallet: p, from }) => {
                let d = |c: char| {
                    format!(
                        "kynex_places_{p}{{kynex_place_{p} + 1}}.{c} - kynex_places_{p}{{{}}}.{c}",
                        from + 1
                    )
                };
                format!("Offs({name}, {}, {}, {})", d('x'), d('y'), d('z'))
            }
            None => name,
        };
        let speed = format!("kynex_v{}", (w.speed * 1000.0).round().max(1.0) as u32);
        match kind {
            MoveKind::Joint if m.place.is_none() || k == 0 => {
                let _ = writeln!(
                    out,
                    "{indent}MoveAbsJ j{}, vmax, {zone}, kynex_tool;",
                    k + 1
                );
            }
            MoveKind::Joint => {
                let _ = writeln!(
                    out,
                    "{indent}MoveJ {}, vmax, {zone}, kynex_tool \\WObj:=wobj0;",
                    target(format!("p{}", k + 1))
                );
            }
            MoveKind::Linear => {
                let _ = writeln!(
                    out,
                    "{indent}MoveL {}, {speed}, {zone}, kynex_tool \\WObj:=wobj0;",
                    target(format!("p{}", k + 1))
                );
            }
            MoveKind::Circular if w.via.is_some() => {
                let _ = writeln!(
                    out,
                    "{indent}MoveC {}, {}, {speed}, {zone}, kynex_tool \\WObj:=wobj0;",
                    target(format!("v{}", k + 1)),
                    target(format!("p{}", k + 1))
                );
            }
            MoveKind::Circular => {
                let _ = writeln!(
                    out,
                    "{indent}MoveL {}, {speed}, {zone}, kynex_tool \\WObj:=wobj0;",
                    target(format!("p{}", k + 1))
                );
            }
        }
        if export.markers {
            let _ = writeln!(out, "{indent}kynex_done := {mark};");
        }
    };
    // Wherever the robot stands, it goes to the first point by its joints.
    let first = &export.moves[0];
    let _ = writeln!(out, "    ! To the start");
    body(&mut out, "    ", 0, first, MoveKind::Joint);
    let rest = &export.moves[1..];
    if export.close {
        let _ = writeln!(out, "    WHILE TRUE DO");
        for (k, m) in rest.iter().enumerate() {
            body(&mut out, "      ", k + 1, m, m.waypoint.kind);
        }
        for op in &export.trailing {
            op_lines(&mut out, "      ", op, export);
        }
        body(&mut out, "      ", 0, first, first.waypoint.kind);
        let _ = writeln!(out, "    ENDWHILE");
    } else {
        for (k, m) in rest.iter().enumerate() {
            body(&mut out, "    ", k + 1, m, m.waypoint.kind);
        }
        for op in &export.trailing {
            op_lines(&mut out, "    ", op, export);
        }
        if export.markers {
            mark += 1;
            let _ = writeln!(out, "    kynex_mark := {mark};");
        }
    }
    let _ = writeln!(out, "  ENDPROC");
    let _ = writeln!(out, "ENDMODULE");
    Ok(out)
}

/// `op` as RAPID, at `indent`.
fn op_lines(out: &mut String, indent: &str, op: &Op, export: &Export) {
    let _ = match op {
        Op::Note(n) => writeln!(out, "{indent}! {}", comment(n)),
        Op::Set { port, on } => writeln!(out, "{indent}SetDO DO_KYNEX_{port}, {};", u8::from(*on)),
        Op::Pulse { port, seconds } => writeln!(
            out,
            "{indent}PulseDO \\PLength:={seconds}, DO_KYNEX_{port};"
        ),
        Op::Wait { port, on } => {
            writeln!(out, "{indent}WaitDI DI_KYNEX_{port}, {};", u8::from(*on))
        }
        Op::Sleep(seconds) => writeln!(out, "{indent}WaitTime {seconds:.2};"),
        Op::Next { pallet } => {
            let n = export.pallets.get(*pallet).map_or(1, |p| p.at.len().max(1));
            writeln!(
                out,
                "{indent}kynex_place_{pallet} := (kynex_place_{pallet} + 1) MOD {n};"
            )
        }
    };
}

#[cfg(test)]
mod tests;
