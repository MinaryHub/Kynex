//! Kreate edits, KYNEX sees: a real relay, a real Kreate participant (the
//! same `Session` the application drives each frame), and the link.

use std::time::{Duration, Instant};

use kreate_collab::{Relay, RelayConfig, Session};
use kreate_math::{DTransform, DVec3};
use kreate_scene::robot::{Joint, JointKind, RobotPart};
use kreate_scene::{Document, Object, ObjectData, ObjectId};
use kynex_link::{KreateLink, LinkConfig, LinkEvent};
use kynex_model::Model;

const KEY: &str = "studio";

fn relay() -> Relay {
    Relay::start(
        "127.0.0.1:0",
        RelayConfig {
            key: KEY.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn link_to(relay: &Relay) -> KreateLink {
    let mut config = LinkConfig::new(format!("127.0.0.1:{}", relay.addr().port()));
    config.key = KEY.into();
    config.retry = Duration::from_millis(20);
    KreateLink::new(config)
}

/// A base, a revolute shoulder 1 m up, and an arm hanging from it.
fn arm(doc: &mut Document) -> ObjectId {
    let mut base = Object::new("base", ObjectData::Empty);
    base.robot = Some(RobotPart::Link { inertial: None });
    let base = doc.add_object(base);
    let rest = DTransform {
        translation: DVec3::new(0.0, 0.0, 1.0),
        ..DTransform::IDENTITY
    };
    let mut joint = Object::new("shoulder", ObjectData::Empty);
    joint.transform = rest;
    joint.robot = Some(RobotPart::Joint(Joint {
        kind: JointKind::Revolute,
        rest,
        lower: -2.0,
        upper: 2.0,
        ..Default::default()
    }));
    let joint = doc.add_object(joint);
    doc.set_parent(joint, Some(base)).unwrap();
    let mut link = Object::new("upper_arm", ObjectData::Empty);
    // Half a metre out from the shoulder: parenting keeps where it stands.
    link.transform.translation = DVec3::new(0.5, 0.0, 1.0);
    link.robot = Some(RobotPart::Link { inertial: None });
    let link = doc.add_object(link);
    doc.set_parent(link, Some(joint)).unwrap();
    joint
}

/// Runs Kreate's frame and KYNEX's poll until `done` holds of what KYNEX has.
fn pump(
    kreate: &mut Session,
    doc: &mut Document,
    link: &mut KreateLink,
    events: &mut Vec<LinkEvent>,
    mut done: impl FnMut(&KreateLink) -> bool,
) -> Duration {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        kreate.tick(doc, None);
        kreate.flush(doc);
        events.extend(link.poll());
        if done(link) {
            return start.elapsed();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("KYNEX never caught up; it saw {events:?}");
}

fn shoulder(model: &Model) -> f64 {
    model.robots[0].joints[0].position
}

#[test]
fn an_edit_in_kreate_reaches_kynex_live() {
    let relay = relay();
    let mut doc = Document::new();
    doc.add_mesh_object("Table", kreate_mesh::primitives::cube(1.0));
    let joint = arm(&mut doc);
    let mut kreate =
        Session::join(&format!("127.0.0.1:{}", relay.addr().port()), "Kim", KEY).unwrap();
    let mut link = link_to(&relay);
    let mut events = Vec::new();

    // KYNEX gets the scene as it stands.
    pump(&mut kreate, &mut doc, &mut link, &mut events, |l| {
        l.is_live()
    });
    let model = Model::from_document(link.document());
    assert_eq!(model.bodies.len(), 4);
    assert_eq!(model.robots.len(), 1);
    assert_eq!(model.robots[0].joints[0].kind, JointKind::Revolute);
    assert!(shoulder(&model).abs() < 1e-9);

    // Turning the shoulder in Kreate turns it in KYNEX.
    let RobotPart::Joint(j) = doc.object(joint).unwrap().robot.clone().unwrap() else {
        unreachable!()
    };
    doc.object_mut(joint).unwrap().transform = j.place(0.7);
    let took = pump(&mut kreate, &mut doc, &mut link, &mut events, |l| {
        (shoulder(&Model::from_document(l.document())) - 0.7).abs() < 1e-9
    });
    assert!(took < Duration::from_millis(500), "took {took:?}");
    let model = Model::from_document(link.document());
    let arm_end = model
        .bodies
        .values()
        .find(|b| b.name == "upper_arm")
        .unwrap();
    // Half a metre out along X, turned 0.7 rad about Z, 1 m up.
    let expect = DVec3::new(0.5 * 0.7f64.cos(), 0.5 * 0.7f64.sin(), 1.0);
    assert!((arm_end.world.translation - expect).length() < 1e-9);

    // A joint's limits, changed in Kreate, change in KYNEX.
    if let Some(RobotPart::Joint(j)) = &mut doc.object_mut(joint).unwrap().robot {
        j.upper = 0.5;
    }
    pump(&mut kreate, &mut doc, &mut link, &mut events, |l| {
        Model::from_document(l.document()).robots[0].joints[0].upper == 0.5
    });
    assert!(Model::from_document(link.document()).robots[0].joints[0].out_of_limits());

    // Adding and deleting objects.
    doc.add_mesh_object("Conveyor", kreate_mesh::primitives::cube(2.0));
    let table = doc.objects().find(|(_, o)| o.name == "Table").unwrap().0;
    doc.remove_object(table);
    pump(&mut kreate, &mut doc, &mut link, &mut events, |l| {
        let names: Vec<_> = l
            .document()
            .objects()
            .map(|(_, o)| o.name.clone())
            .collect();
        names.contains(&"Conveyor".to_string()) && !names.contains(&"Table".to_string())
    });

    assert!(events.iter().any(|e| matches!(e, LinkEvent::Changed(_))));
}

#[test]
fn kynex_arriving_first_does_not_empty_the_session() {
    let relay = relay();
    let mut link = link_to(&relay);
    // Nobody has opened the session with a scene: KYNEX waits rather than
    // seeding it with nothing.
    let events = link.wait(Duration::from_secs(5));
    assert_eq!(events, vec![LinkEvent::WaitingForScene]);
    assert!(!link.is_live());

    let mut doc = Document::new();
    arm(&mut doc);
    let mut kreate =
        Session::join(&format!("127.0.0.1:{}", relay.addr().port()), "Kim", KEY).unwrap();
    let mut events = Vec::new();
    pump(&mut kreate, &mut doc, &mut link, &mut events, |l| {
        l.is_live()
    });
    assert_eq!(doc.object_count(), 3, "Kreate kept its own scene");
    assert_eq!(link.document().object_count(), 3, "and KYNEX has it");
    assert!(kreate.waiting() == 0);
}

#[test]
fn the_wrong_key_is_reported_once_however_often_it_retries() {
    let relay = relay();
    let mut config = LinkConfig::new(format!("127.0.0.1:{}", relay.addr().port()));
    config.key = "wrong".into();
    config.retry = Duration::from_millis(10);
    let mut link = KreateLink::new(config);
    let mut events = Vec::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(300) {
        events.extend(link.poll());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(events, vec![LinkEvent::Offline("wrong session key".into())]);
}
