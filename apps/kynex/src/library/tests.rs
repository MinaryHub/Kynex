use kynex_kin::Kinematics;

use super::*;

fn ask(library: &mut Library, req: Value) -> (Value, RoleChanges) {
    let (answer, roles) = library.handle(&req.to_string());
    (serde_json::from_str(&answer).unwrap(), roles)
}

fn shown(library: &mut Library) -> (Document, Changes) {
    let mut doc = Document::new();
    let changes = library.apply(&mut doc);
    (doc, changes)
}

fn named(doc: &Document, name: &str) -> ObjectId {
    doc.objects()
        .find(|(_, o)| o.name == name)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn a_cnc_machine_comes_with_its_role_and_stands_where_placed() {
    let mut library = Library::default();
    let (a, roles) = ask(
        &mut library,
        json!({"op": "place", "template": "cnc_lathe", "at": [2.0, 0.0, 0.0], "yaw": std::f64::consts::FRAC_PI_2}),
    );
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(roles.added.len(), 1);
    let (uid, name, kind) = roles.added[0].clone();
    assert_eq!((name.as_str(), kind), ("CNC 선반 1", Kind::Cnc));
    assert_eq!(uid, EQUIPMENT + PER_ITEM);

    let (doc, changes) = shown(&mut library);
    assert!(changes
        .changed
        .contains(&(DatablockKind::Object, Uid::from_raw(uid))));
    // Turned a quarter round, the machine's front (−Y) faces +X, so its
    // chuck, 0.35 m in from the front, lies 0.35 m along −X from x = 2.
    let chuck = doc.world_transform(named(&doc, "chuck")).translation;
    assert!(
        (chuck - DVec3::new(2.0 - 0.35, 0.0, 0.91)).length() < 1e-9,
        "{chuck}"
    );
    // Its parts hang under the item's root, under its identity.
    let root = doc.object_by_uid(Uid::from_raw(uid)).unwrap();
    assert_eq!(doc.object(root).unwrap().name, "CNC 선반 1");
}

#[test]
fn a_robot_from_the_library_is_a_robot() {
    let mut library = Library::default();
    let (_, roles) = ask(
        &mut library,
        json!({"op": "place", "template": "ur10e_tending", "at": [0.0, 0.0, 0.7]}),
    );
    // Its two grip points.
    let grips: Vec<_> = roles
        .added
        .iter()
        .filter(|r| r.2 == Kind::Gripper)
        .map(|r| r.1.clone())
        .collect();
    assert_eq!(grips, vec!["grip_a", "grip_b"]);
    let (doc, _) = shown(&mut library);
    let robots = kynex_model::robots(&doc);
    assert_eq!(robots.len(), 1);
    assert_eq!(
        robots[0].name, "UR10e + 이중 그리퍼 1",
        "named after the item"
    );
    let kin = Kinematics::of(&doc, robots[0].root).unwrap();
    assert_eq!(kin.vars.len(), 6);
    // Its base link stands where the item was placed, 0.7 m up.
    let base_link = kin.uids().next().unwrap();
    let base = kin.world_of(&kin.current, base_link).unwrap().translation;
    assert!((base.z - 0.7).abs() < 1e-9, "{base}");
}

#[test]
fn moving_and_resizing_keep_identities_and_say_what_went() {
    let mut library = Library::default();
    ask(&mut library, json!({"op": "place", "template": "fence"}));
    let (doc, _) = shown(&mut library);
    let before = doc.object_count();
    // Moved: the same objects, elsewhere.
    ask(
        &mut library,
        json!({"op": "move", "item": 1, "at": [1.0, 2.0, 0.0]}),
    );
    let (moved, changes) = shown(&mut library);
    assert_eq!(moved.object_count(), before);
    assert!(changes.removed.is_empty());
    // Made smaller: fewer posts, and those that went are said to have gone.
    ask(
        &mut library,
        json!({"op": "resize", "item": 1, "params": {"length": 1.2, "width": 1.2}}),
    );
    let (small, changes) = shown(&mut library);
    assert!(small.object_count() < before);
    assert_eq!(changes.removed.len(), before - small.object_count());
    // Taken away: everything it had goes.
    let (_, roles) = ask(&mut library, json!({"op": "remove", "item": 1}));
    assert!(roles.added.is_empty());
    let (empty, changes) = shown(&mut library);
    assert_eq!(empty.object_count(), 0);
    assert_eq!(changes.removed.len(), small.object_count());
}

#[test]
fn placed_equipment_is_saved_and_comes_back_the_same() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "infeed_station", "at": [-0.8, 0.0, 0.0], "params": {"height": 0.9}}),
    );
    ask(&mut library, json!({"op": "place", "template": "pedestal"}));
    assert!(library.take_items_changed());
    let (doc, _) = shown(&mut library);
    let mut again = Library::from_saved(&library.to_saved());
    let (doc2, _) = shown(&mut again);
    let uids = |d: &Document| {
        let mut v: Vec<u64> = d
            .objects()
            .filter_map(|(id, _)| d.object_uid(id))
            .map(|u| u.raw())
            .collect();
        v.sort();
        v
    };
    assert_eq!(uids(&doc), uids(&doc2));
    let plate = doc2.world_transform(named(&doc2, "pallet")).translation;
    assert!(
        (plate - DVec3::new(-0.8, 0.0, 0.89)).length() < 1e-9,
        "{plate}"
    );
    // The next one placed is numbered on.
    let (a, _) = ask(&mut again, json!({"op": "place", "template": "pedestal"}));
    assert_eq!(a["item"], 3);
}

#[test]
fn an_unknown_template_or_item_is_refused() {
    let mut library = Library::default();
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "teleporter"}),
    );
    assert_eq!(a["ok"], false);
    let (a, _) = ask(&mut library, json!({"op": "move", "item": 9}));
    assert_eq!(a["ok"], false);
}
