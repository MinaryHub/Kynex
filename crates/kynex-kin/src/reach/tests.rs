use kreate_math::DAffine3;

use super::*;
use crate::tests::{find, load, UR5E};

/// A UR5e's flange reaches past the 850 mm its datasheet gives (to its
/// wrist), short of all its links laid end to end; all the way round, and
/// from below its base to above its head.
#[test]
fn a_ur5e_reaches_as_far_as_its_links_let_it() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(flange, DAffine3::IDENTITY).unwrap();
    let started = std::time::Instant::now();
    let r = reach(&kin, &tool, base).unwrap();
    let took = started.elapsed();
    let links = 0.425 + 0.3922 + 0.1333 + 0.0997 + 0.0996;
    assert!((0.85..links).contains(&r.farthest), "{}", r.farthest);
    assert!(
        r.turn.1 - r.turn.0 >= std::f64::consts::TAU - 1e-9,
        "{:?}",
        r.turn
    );
    let top = r.section.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    let bottom = r.section.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
    assert!(top > 0.9 && bottom < -0.6, "{top} {bottom}");
    // Round its edge: up the far side, back down the near.
    let half = r.section.len() / 2;
    assert!(r.section[..half].windows(2).all(|w| w[0][1] < w[1][1]));
    assert!(r.section.iter().all(|p| p[0] >= 0.0));
    assert!(took.as_secs_f64() < 2.0, "{took:?}");
    println!(
        "{} points, farthest {:.3}, {took:?}",
        r.section.len(),
        r.farthest
    );
}
