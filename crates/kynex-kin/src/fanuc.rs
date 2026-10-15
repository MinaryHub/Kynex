//! A program as TP, for a FANUC controller: the writer for FANUC (see
//! [`crate::post`]). It is written as an LS file, the TP program's text
//! form, which a controller with the ASCII program loader reads, and
//! FANUC's tools turn into a TP file.
//!
//! - Positions are the controller's own: in joint form (`J1`–`J6`,
//!   degrees), exactly as taught, for every move but those on a pallet. A
//!   FANUC arm's J3 is its forearm's angle from level, not from the upper
//!   arm as a robot description has it (J2/J3 interaction): J3 is written
//!   as the description's joint 3 less its joint 2.
//! - Joint moves go as `J` at their share of top speed (`%`); linear and
//!   circular moves as `L` and `C` at their own speed (`mm/sec`), the
//!   controller turning a joint-form position into the tool pose. A blend
//!   is `CNT`, its radius in millimetres taken as the CNT value (FANUC's
//!   CNT is no distance: the corner it rounds depends on the speed); no
//!   blend is `FINE`.
//! - The tool point is user tool 1, set from the faceplate (`tool0`) at
//!   the program's start by way of `PR[49]`; positions are in the world
//!   frame (user frame 0), the robot's `base`.
//! - I/O: KYNEX's 0–15 are `DO[1]`–`DO[16]` and `DI[1]`–`DI[16]`, 16 and 17
//!   the robot's own at its wrist, `RO[1]`, `RO[2]` and `RI[1]`, `RI[2]`.
//! - Pallets: each place is a position register, `PR[51]` on, set at the
//!   program's start; each pallet's count is `R[81]` on. A move on a
//!   pallet goes to its point, taught over a place, in Cartesian form, by
//!   `Offset,PR[50]`: how far the place counted to is from that one.
//!   `R[90]` is scratch. Its posture (`CONFIG`) is worked out from the
//!   taught joints: flip from J5's sign, up or down from the elbow, front
//!   or back from where the wrist is from J1's axis, and J1, J4, J6's turns.
//! - Markers, for timing: each move's number in `R[98]` as it starts and in
//!   `R[99]` as it is done.
//! - An axis outside the arm (a track) is not written; a positioner's turn
//!   is a remark.

use std::fmt::Write;

use kreate_math::glam::EulerRot;
use kreate_math::{DAffine3, DQuat};

use crate::path::MoveKind;
use crate::post::{axes, comment, identifier, Export, Move, Op, Place, Writer};
use crate::{Kinematics, Tool};

/// FANUC: TP, as LS.
pub struct Fanuc;

impl Writer for Fanuc {
    fn vendor(&self) -> &'static str {
        "FANUC"
    }

    fn extension(&self) -> &'static str {
        "ls"
    }

    fn write(&self, kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
        tp(kin, tool, export)
    }
}

/// The position register the tool is set from.
const TOOL_PR: u32 = 49;
/// The position register a pallet move's offset is worked out in.
const OFFSET_PR: u32 = 50;
/// The first position register of the pallets' places, and the last one.
const PLACES_PR: u32 = 51;
const LAST_PR: u32 = 100;
/// The first register of the pallets' counts.
const COUNT_R: u32 = 81;
/// Scratch: the register a place's position register is counted in.
const SCRATCH_R: u32 = 90;
/// The markers' registers: a move started, and done.
const MARK_R: u32 = 98;
const DONE_R: u32 = 99;

/// `v` to `digits` places, with no sign on a zero.
fn fixed(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s,
    }
}

/// A remark: what fits on a line of the teach pendant, with no `;` to end
/// the instruction early and put the rest of a name into the program.
fn remark(text: &str) -> String {
    let safe = comment(text).replace(';', "?");
    format!("!{}", safe.chars().take(32).collect::<String>())
}

/// The arm's joints at `q` as the controller has them, degrees: J3 from
/// level (see the module's notes).
fn pendant(q: &[f64], arm: &[usize]) -> [f64; 6] {
    let mut j = [0.0; 6];
    for (k, i) in arm.iter().enumerate() {
        j[k] = q[*i].to_degrees();
    }
    j[2] -= j[1];
    j
}

/// FANUC's W, P, R of `r`, degrees: `r = Rz(R) · Ry(P) · Rx(W)`.
fn wpr(r: DQuat) -> [f64; 3] {
    let (rz, ry, rx) = r.to_euler(EulerRot::ZYX);
    [rx, ry, rz].map(f64::to_degrees)
}

/// How many turns past ±180° a joint at `deg` is.
fn turn(deg: f64) -> i32 {
    ((deg - 180.0) / 360.0).ceil() as i32
}

/// The posture of the arm at `q`, as `CONFIG` says it.
fn config(kin: &Kinematics, arm: &[usize], q: &[f64]) -> Result<String, String> {
    let at = |k: usize| {
        kin.world_of(q, kin.vars[arm[k]].uid)
            .ok_or_else(|| "no joint frame".to_string())
    };
    let (j1, j2, j3, j5) = (at(0)?, at(1)?, at(2)?, at(4)?);
    // The arm's forward and J2's axis, as J1 has turned them.
    let forward = j1.matrix3.x_axis.normalize_or_zero();
    let side = j1.matrix3.y_axis.normalize_or_zero();
    let wrist = j5.translation;
    let front = (wrist - j1.translation).dot(forward) >= 0.0;
    let up = (j3.translation - j2.translation)
        .cross(wrist - j3.translation)
        .dot(side)
        > 0.0;
    let j = pendant(q, arm);
    Ok(format!(
        "'{} {} {}, {}, {}, {}'",
        if j[4] > 0.0 { 'F' } else { 'N' },
        if up { 'U' } else { 'D' },
        if front { 'T' } else { 'B' },
        turn(j[0]),
        turn(j[3]),
        turn(j[5])
    ))
}

/// A position in the program's `/POS` section.
enum Pos {
    Joints([f64; 6]),
    Cartesian { pose: DAffine3, config: String },
}

impl Pos {
    fn write(&self, out: &mut String, n: usize) {
        let _ = writeln!(out, "P[{n}]{{");
        let _ = writeln!(out, "   GP1:");
        match self {
            Pos::Joints(j) => {
                let _ = writeln!(out, "\tUF : 0, UT : 1,\t");
                let v = j.map(|x| format!("{:>10}", fixed(x, 3)));
                let _ = writeln!(
                    out,
                    "\tJ1={} deg,\tJ2={} deg,\tJ3={} deg,",
                    v[0], v[1], v[2]
                );
                let _ = writeln!(out, "\tJ4={} deg,\tJ5={} deg,\tJ6={} deg", v[3], v[4], v[5]);
            }
            Pos::Cartesian { pose, config } => {
                let (_, r, t) = pose.to_scale_rotation_translation();
                let [x, y, z] = (t * 1000.0)
                    .to_array()
                    .map(|v| format!("{:>10}", fixed(v, 3)));
                let [w, p, r] = wpr(r).map(|v| format!("{:>10}", fixed(v, 3)));
                let _ = writeln!(out, "\tUF : 0, UT : 1,\t\tCONFIG : {config},");
                let _ = writeln!(out, "\tX ={x}  mm,\tY ={y}  mm,\tZ ={z}  mm,");
                let _ = writeln!(out, "\tW ={w} deg,\tP ={p} deg,\tR ={r} deg");
            }
        }
        let _ = writeln!(out, "}};");
    }
}

/// The program's lines as it builds them: each instruction one or more
/// lines of text, numbered as one.
#[derive(Default)]
struct Lines {
    lines: Vec<Vec<String>>,
    label: u32,
}

impl Lines {
    /// An instruction that is not a move.
    fn say(&mut self, text: impl AsRef<str>) {
        self.lines.push(vec![format!("  {} ;", text.as_ref())]);
    }

    /// A move: its letter first.
    fn motion(&mut self, text: impl AsRef<str>) {
        self.lines.push(vec![format!("{}    ;", text.as_ref())]);
    }

    /// A circular move: its via point, then its end on a line of its own.
    fn circular(&mut self, via: &str, end: &str) {
        self.lines
            .push(vec![format!("C {via}    "), format!("  {end}    ;")]);
    }

    /// A label not used yet.
    fn next_label(&mut self) -> u32 {
        self.label += 1;
        self.label
    }

    fn write(&self, out: &mut String) {
        for (k, lines) in self.lines.iter().enumerate() {
            for (i, line) in lines.iter().enumerate() {
                if i == 0 {
                    let _ = writeln!(out, "{:>4}:{line}", k + 1);
                } else {
                    let _ = writeln!(out, "    :{line}");
                }
            }
        }
    }
}

/// KYNEX's digital output or input `n` as the controller names it.
fn io(n: u32, output: bool) -> String {
    match (n, output) {
        (0..=15, true) => format!("DO[{}]", n + 1),
        (0..=15, false) => format!("DI[{}]", n + 1),
        (_, true) => format!("RO[{}]", n - 15),
        (_, false) => format!("RI[{}]", n - 15),
    }
}

/// Writes the program as an LS file, or says why it cannot be one.
pub fn tp(kin: &Kinematics, tool: &Tool, export: &Export) -> Result<String, String> {
    let (arm, outside) = axes(kin).map_err(|e| format!("TP: {e}"))?;
    if !outside.is_empty() {
        return Err(
            "TP: 외부축(주행 트랙 등)이 있는 로봇은 아직 RAPID(ABB)로만 내보낼 수 있습니다".into(),
        );
    }
    if export.moves.is_empty() {
        return Err("프로그램에 지점이 없습니다".into());
    }
    let any = &export.moves[0].waypoint.positions;
    let base = kin.world_of(any, export.base).ok_or("no base frame")?;
    let flange = kin.world_of(any, export.flange).ok_or("no flange")?;
    let tcp = flange.inverse() * kin.pose(any, tool);
    let in_base = |q: &[f64]| base.inverse() * kin.pose(q, tool);

    // Each pallet's first place's register.
    let mut first_pr = Vec::new();
    let mut next_pr = PLACES_PR;
    for pallet in &export.pallets {
        first_pr.push(next_pr);
        next_pr += pallet.at.len() as u32;
    }
    if next_pr > LAST_PR + 1 {
        return Err(format!(
            "TP: 팔레트 칸이 {}개로, 위치 레지스터 PR[{PLACES_PR}]–PR[{LAST_PR}]에 다 들어가지 않습니다",
            next_pr - PLACES_PR
        ));
    }

    // Every move's position, and its circular move's via point after all
    // of them.
    let mut positions: Vec<Pos> = Vec::new();
    let mut vias: Vec<(usize, Pos)> = Vec::new();
    for (k, m) in export.moves.iter().enumerate() {
        let w = m.waypoint;
        let form = |q: &[f64]| -> Result<Pos, String> {
            Ok(if m.place.is_some() && k > 0 {
                Pos::Cartesian {
                    pose: in_base(q),
                    config: config(kin, &arm, q)?,
                }
            } else {
                Pos::Joints(pendant(q, &arm))
            })
        };
        positions.push(form(&w.positions)?);
        if let Some(via) = w.via.as_ref().filter(|_| w.kind == MoveKind::Circular) {
            vias.push((k, form(via)?));
        }
    }
    let via_of = |k: usize| {
        vias.iter()
            .position(|(at, _)| *at == k)
            .map(|i| export.moves.len() + i + 1)
    };

    let mut lines = Lines::default();
    lines.say(remark(&format!("KYNEX: {}", export.name)));
    // The tool, from the faceplate.
    let (_, tr, tt) = tcp.to_scale_rotation_translation();
    let t = (tt * 1000.0).to_array();
    let r = wpr(tr);
    lines.say(format!("PR[{TOOL_PR}]=LPOS-LPOS"));
    for (i, v) in t.iter().chain(&r).enumerate() {
        lines.say(format!("PR[{TOOL_PR},{}]={}", i + 1, fixed(*v, 3)));
    }
    lines.say(format!("UTOOL[1]=PR[{TOOL_PR}]"));
    lines.say("UTOOL_NUM=1");
    lines.say("UFRAME_NUM=0");
    // The pallets' places, and their counts from the first.
    for (p, pallet) in export.pallets.iter().enumerate() {
        lines.say(remark(&format!("places of {}", pallet.name)));
        for (i, at) in pallet.at.iter().enumerate() {
            let pr = first_pr[p] + i as u32;
            lines.say(format!("PR[{pr}]=LPOS-LPOS"));
            for (c, v) in (*at * 1000.0).to_array().iter().enumerate() {
                lines.say(format!("PR[{pr},{}]={}", c + 1, fixed(*v, 3)));
            }
        }
        lines.say(format!("R[{}]=0", COUNT_R + p as u32));
    }

    let mut mark = 0;
    let mut body = |lines: &mut Lines, k: usize, m: &Move, kind: MoveKind| {
        for op in &m.before {
            op_lines(lines, op, export);
        }
        let w = m.waypoint;
        let term = if w.blend > 0.0 {
            format!("CNT{}", (w.blend * 1000.0).round().clamp(1.0, 100.0) as u32)
        } else {
            "FINE".to_string()
        };
        if export.markers {
            mark += 1;
            lines.say(format!("R[{MARK_R}]={mark}"));
        }
        lines.say(remark(m.name));
        // On a pallet: the place counted to, less the one it was taught
        // over.
        let offset = match m.place {
            Some(Place { pallet, from }) if k > 0 => {
                let first = first_pr[pallet];
                lines.say(format!(
                    "R[{SCRATCH_R}]=R[{}]+{first}",
                    COUNT_R + pallet as u32
                ));
                lines.say(format!(
                    "PR[{OFFSET_PR}]=PR[R[{SCRATCH_R}]]-PR[{}]",
                    first + from as u32
                ));
                format!(" Offset,PR[{OFFSET_PR}]")
            }
            _ => String::new(),
        };
        let speed = format!("{}mm/sec", (w.speed * 1000.0).round().max(1.0) as u32);
        let p = k + 1;
        match kind {
            MoveKind::Joint => {
                let share = if w.kind == MoveKind::Joint {
                    (w.speed.clamp(0.01, 1.0) * 100.0).round() as u32
                } else {
                    100
                };
                lines.motion(format!("J P[{p}] {share}% {term}{offset}"));
            }
            MoveKind::Circular if via_of(k).is_some() => {
                let via = via_of(k).unwrap();
                lines.circular(
                    &format!("P[{via}]"),
                    &format!("P[{p}] {speed} {term}{offset}"),
                );
            }
            MoveKind::Linear | MoveKind::Circular => {
                lines.motion(format!("L P[{p}] {speed} {term}{offset}"));
            }
        }
        if export.markers {
            lines.say(format!("R[{DONE_R}]={mark}"));
        }
    };
    // Wherever the robot stands, it goes to the first point by its joints.
    let first = &export.moves[0];
    lines.say(remark("To the start"));
    body(&mut lines, 0, first, MoveKind::Joint);
    let rest = &export.moves[1..];
    if export.close {
        let again = lines.next_label();
        lines.say(format!("LBL[{again}]"));
        for (k, m) in rest.iter().enumerate() {
            body(&mut lines, k + 1, m, m.waypoint.kind);
        }
        for op in &export.trailing {
            op_lines(&mut lines, op, export);
        }
        body(&mut lines, 0, first, first.waypoint.kind);
        lines.say(format!("JMP LBL[{again}]"));
    } else {
        for (k, m) in rest.iter().enumerate() {
            body(&mut lines, k + 1, m, m.waypoint.kind);
        }
        for op in &export.trailing {
            op_lines(&mut lines, op, export);
        }
        if export.markers {
            mark += 1;
            lines.say(format!("R[{MARK_R}]={mark}"));
        }
    }

    let name: String = identifier(&format!("KYNEX_{}", export.name))
        .to_uppercase()
        .chars()
        .take(30)
        .collect();
    let mut out = String::new();
    let _ = writeln!(out, "/PROG  {name}");
    let _ = writeln!(out, "/ATTR");
    let _ = writeln!(out, "OWNER\t\t= MNEDITOR;");
    let _ = writeln!(out, "COMMENT\t\t= \"KYNEX\";");
    let _ = writeln!(out, "PROG_SIZE\t= 0;");
    let _ = writeln!(out, "CREATE\t\t= DATE 26-01-01  TIME 00:00:00;");
    let _ = writeln!(out, "MODIFIED\t= DATE 26-01-01  TIME 00:00:00;");
    let _ = writeln!(out, "FILE_NAME\t= ;");
    let _ = writeln!(out, "VERSION\t\t= 0;");
    let _ = writeln!(out, "LINE_COUNT\t= {};", lines.lines.len());
    let _ = writeln!(out, "MEMORY_SIZE\t= 0;");
    let _ = writeln!(out, "PROTECT\t\t= READ_WRITE;");
    let _ = writeln!(out, "TCD:  STACK_SIZE\t= 0,");
    let _ = writeln!(out, "      TASK_PRIORITY\t= 50,");
    let _ = writeln!(out, "      TIME_SLICE\t= 0,");
    let _ = writeln!(out, "      BUSY_LAMP_OFF\t= 0,");
    let _ = writeln!(out, "      ABORT_REQUEST\t= 0,");
    let _ = writeln!(out, "      PAUSE_REQUEST\t= 0;");
    let _ = writeln!(out, "DEFAULT_GROUP\t= 1,*,*,*,*;");
    let _ = writeln!(out, "CONTROL_CODE\t= 00000000 00000000;");
    let _ = writeln!(out, "/MN");
    lines.write(&mut out);
    let _ = writeln!(out, "/POS");
    for (k, p) in positions.iter().enumerate() {
        p.write(&mut out, k + 1);
    }
    for (i, (_, p)) in vias.iter().enumerate() {
        p.write(&mut out, export.moves.len() + i + 1);
    }
    let _ = writeln!(out, "/END");
    Ok(out)
}

/// `op` as TP.
fn op_lines(lines: &mut Lines, op: &Op, export: &Export) {
    match op {
        Op::Note(n) => lines.say(remark(n)),
        Op::Turn { unit, degrees, .. } => {
            lines.say(remark(&format!("turn {unit} to {degrees:.0} deg")));
        }
        Op::Set { port, on } => {
            lines.say(format!(
                "{}={}",
                io(*port, true),
                if *on { "ON" } else { "OFF" }
            ));
        }
        Op::Pulse { port, seconds } => {
            lines.say(format!(
                "PULSE {},{}sec",
                io(*port, true),
                fixed(*seconds, 1)
            ));
        }
        Op::Wait { port, on } => {
            lines.say(format!(
                "WAIT {}={}",
                io(*port, false),
                if *on { "ON" } else { "OFF" }
            ));
        }
        Op::Sleep(seconds) => lines.say(format!("WAIT {:>6}(sec)", fixed(*seconds, 2))),
        Op::Next { pallet } => {
            let n = export.pallets.get(*pallet).map_or(1, |p| p.at.len().max(1));
            let r = COUNT_R + *pallet as u32;
            let past = lines.next_label() + 100;
            lines.say(format!("R[{r}]=R[{r}]+1"));
            lines.say(format!("IF R[{r}]<{n},JMP LBL[{past}]"));
            lines.say(format!("R[{r}]=0"));
            lines.say(format!("LBL[{past}]"));
        }
    }
}

#[cfg(test)]
mod tests;
