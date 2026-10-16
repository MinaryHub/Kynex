use super::*;

fn ask(cell: &mut Cell, doc: &Document, req: Value) -> Value {
    let a: Value = serde_json::from_str(&cell.handle(doc, &req.to_string())).unwrap();
    assert_eq!(a["ok"], true, "{req}: {a}");
    a
}

/// The library's `items` (template, x, y, settings) placed; the cell with
/// their roles, in order.
fn floor(items: &[(&str, f64, f64, Value)]) -> (Document, Cell, Vec<u64>) {
    let mut library = crate::library::Library::default();
    for (template, x, y, _) in items {
        library
            .handle(&json!({"op": "place", "template": template, "at": [x, y, 0.0]}).to_string());
    }
    let mut doc = Document::new();
    library.apply(&mut doc);
    let mut cell = Cell::default();
    let mut uids = Vec::new();
    for (k, (uid, name, kind)) in library.roles().into_iter().enumerate() {
        cell.ensure(uid, name, kind);
        if !items[k].3.is_null() {
            ask(
                &mut cell,
                &doc,
                json!({"op": "role", "object": uid.to_string(), "kind": kind.tag(), "props": items[k].3}),
            );
        }
        uids.push(uid);
    }
    (doc, cell, uids)
}

fn lanes(cell: &mut Cell, doc: &Document, from: u64, to: &[u64]) {
    let next: Vec<String> = to.iter().map(u64::to_string).collect();
    ask(
        cell,
        doc,
        json!({"op": "amr_lanes", "object": from.to_string(), "next": next}),
    );
}

fn run(cell: &mut Cell, doc: &Document, from: f64, seconds: f64) -> f64 {
    let start = Instant::now();
    let mut t = from;
    while t < from + seconds {
        t += 0.05;
        cell.tick(doc, t, start + Duration::from_secs_f64(t));
    }
    t
}

fn at(cell: &Cell, amr: u64) -> DVec3 {
    cell.rovers[&amr].at
}

/// A square of stops 4 m a side, linked round three sides: A B C D.
fn square() -> (Document, Cell, [u64; 5]) {
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, 0.0, Value::Null),
        ("amr_stop", 0.0, 0.0, Value::Null),
        ("amr_stop", 4.0, 0.0, Value::Null),
        ("amr_stop", 4.0, 4.0, Value::Null),
        ("amr_stop", 0.0, 4.0, Value::Null),
    ]);
    // Each stop's whole list: links go both ways.
    lanes(&mut cell, &doc, u[1], &[u[2]]);
    lanes(&mut cell, &doc, u[2], &[u[1], u[3]]);
    lanes(&mut cell, &doc, u[3], &[u[2], u[4]]);
    (doc, cell, [u[0], u[1], u[2], u[3], u[4]])
}

#[test]
fn an_amr_drives_the_lanes_the_shortest_way_round() {
    let (doc, mut cell, [amr, _a, b, c, d]) = square();
    // Both ways: C knows B, and D knows C.
    assert!(cell.lanes[&c].contains(&b) && cell.lanes[&d].contains(&c));
    cell.drive(&doc, amr, d).unwrap();
    // 1 m/s: past B by 4.5 s, at C by 9 s; never straight across.
    let t = run(&mut cell, &doc, 0.0, 5.6);
    assert!(at(&cell, amr).x > 3.5, "{}", at(&cell, amr));
    // 12 m, two quarter turns and a half turn to face as D does: 18.3 s.
    run(&mut cell, &doc, t, 14.0);
    let p = at(&cell, amr);
    assert!((p.x).abs() < 1e-6 && (p.y - 4.0).abs() < 1e-6, "{p}");
    assert!(cell.signal(&doc, amr, Signal::Arrived));
    // Not on the lanes: refused, said why.
    let (doc2, mut cell2, u) = floor(&[
        ("amr", 0.0, 0.0, Value::Null),
        ("amr_stop", 0.0, 0.0, Value::Null),
        ("amr_stop", 4.0, 0.0, Value::Null),
        ("amr_stop", 8.0, 0.0, Value::Null),
        ("amr_stop", 12.0, 0.0, Value::Null),
    ]);
    lanes(&mut cell2, &doc2, u[1], &[u[2]]);
    lanes(&mut cell2, &doc2, u[3], &[u[4]]);
    assert!(cell2.drive(&doc2, u[0], u[4]).is_err());
}

#[test]
fn an_amr_waits_for_a_stop_another_holds() {
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, 0.0, Value::Null),
        ("amr", 8.0, 0.0, Value::Null),
        ("amr_stop", 0.0, 0.0, Value::Null),
        ("amr_stop", 4.0, 0.0, Value::Null),
        ("amr_stop", 8.0, 0.0, Value::Null),
        ("amr_stop", 8.0, 4.0, Value::Null),
    ]);
    let (one, two, a, b, c, d) = (u[0], u[1], u[2], u[3], u[4], u[5]);
    lanes(&mut cell, &doc, a, &[b]);
    lanes(&mut cell, &doc, b, &[a, c]);
    lanes(&mut cell, &doc, c, &[b, d]);
    // The second drives to B and stays there, holding it.
    cell.drive(&doc, two, b).unwrap();
    let t = run(&mut cell, &doc, 0.0, 8.0);
    assert!((at(&cell, two).x - 4.0).abs() < 1e-6);
    // The first, on its way to C, waits at A for B to be free.
    cell.drive(&doc, one, c).unwrap();
    let t = run(&mut cell, &doc, t, 10.0);
    assert!(at(&cell, one).x.abs() < 1e-6, "{}", at(&cell, one));
    // B let go of: the first goes on through it.
    cell.drive(&doc, two, d).unwrap();
    run(&mut cell, &doc, t, 20.0);
    assert!((at(&cell, one).x - 8.0).abs() < 1e-6, "{}", at(&cell, one));
    assert!((at(&cell, two).y - 4.0).abs() < 1e-6, "{}", at(&cell, two));
}

#[test]
fn a_flow_sends_its_parts_by_amr() {
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, -1.5, Value::Null),
        ("amr_stop", 0.0, -1.0, Value::Null),
        ("amr_stop", 6.0, -1.0, Value::Null),
        ("flow_source", 0.0, 0.0, json!({"feed": 30.0, "carrier": 1})),
        ("flow_station", 6.0, 0.0, json!({"cycle": 10.0})),
        ("flow_sink", 8.0, 0.0, Value::Null),
    ]);
    let (amr, s1, s2, source, station, sink) = (u[0], u[1], u[2], u[3], u[4], u[5]);
    lanes(&mut cell, &doc, s1, &[s2]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    // Fetched at 1 m/s, loaded 5 s, 6 m, unloaded 5 s, 10 s worked.
    let t = run(&mut cell, &doc, 0.0, 12.0);
    let riding = cell
        .parts()
        .iter()
        .any(|p| matches!(p.flow, Some(Place::Riding { amr: a, .. }) if a == amr));
    assert!(
        riding,
        "on board by 12 s: {:?}",
        cell.parts().iter().map(|p| &p.flow).collect::<Vec<_>>()
    );
    run(&mut cell, &doc, t, 40.0);
    let s = cell.flow_summary();
    assert!(s["sunk"].as_u64().unwrap() >= 1, "{s}");
    assert!(s["problem"].is_null(), "{s}");
}
