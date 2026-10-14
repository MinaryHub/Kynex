//! A model's bodies and robots as JSON: what `kynex link --json` prints and
//! what the viewer's joint panel reads.
//!
//! Identities are strings: past the 31st session participant they exceed
//! 2^53, which a JavaScript number cannot hold exactly.

use kynex_model::{Body, Model, Robot};
use serde_json::{json, Value};

pub fn model(model: &Model) -> Value {
    json!({
        "bodies": model.bodies.values().map(body).collect::<Vec<_>>(),
        "robots": robots(model),
    })
}

pub fn robots(model: &Model) -> Value {
    Value::Array(model.robots.iter().map(robot).collect())
}

pub fn body(body: &Body) -> Value {
    let t = body.world.translation;
    let r = body.world.rotation;
    json!({
        "uid": body.uid.to_string(),
        "name": body.name,
        "parent": body.parent.map(|p| p.to_string()),
        "position": [t.x, t.y, t.z],
        "rotation_xyzw": [r.x, r.y, r.z, r.w],
        "role": body.role.map(|r| format!("{r:?}").to_lowercase()),
        "mass": body.rigid.map(|r| r.mass),
    })
}

pub fn robot(robot: &Robot) -> Value {
    json!({
        "root": robot.root.to_string(),
        "name": robot.name,
        "mass": robot.mass,
        "tool": robot.tool.map(|t| t.to_string()),
        "joints": robot.joints.iter().map(|j| json!({
            "uid": j.uid.to_string(),
            "name": j.name,
            "kind": j.kind.tag(),
            "position": j.position,
            "lower": j.lower,
            "upper": j.upper,
            "limited": j.kind.limited(),
            "slides": j.kind.slides(),
            "moves": j.kind.moves(),
            "origin": [j.origin.x, j.origin.y, j.origin.z],
            "axis": [j.axis.x, j.axis.y, j.axis.z],
            "out_of_limits": j.out_of_limits(),
        })).collect::<Vec<_>>(),
    })
}
