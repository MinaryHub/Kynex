//! A model's bodies and robots as JSON: what `kynex link --json` prints and
//! what the viewer's joint panel reads.

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
        "uid": body.uid,
        "name": body.name,
        "parent": body.parent,
        "position": [t.x, t.y, t.z],
        "rotation_xyzw": [r.x, r.y, r.z, r.w],
        "role": body.role.map(|r| format!("{r:?}").to_lowercase()),
        "mass": body.rigid.map(|r| r.mass),
    })
}

pub fn robot(robot: &Robot) -> Value {
    json!({
        "root": robot.root,
        "name": robot.name,
        "mass": robot.mass,
        "joints": robot.joints.iter().map(|j| json!({
            "uid": j.uid,
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
