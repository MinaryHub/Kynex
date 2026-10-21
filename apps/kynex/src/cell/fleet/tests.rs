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

#[test]
fn a_station_that_needs_a_person_waits_for_one_to_walk_to_it() {
    // One person, 6 m away; a manned station; a part every 30 s.
    let (doc, mut cell, u) = floor(&[
        ("operator", 0.0, -3.0, Value::Null),
        ("flow_source", 0.0, 0.0, json!({"feed": 30.0})),
        ("flow_station", 2.0, 0.0, json!({"cycle": 10.0, "crew": 1})),
        ("flow_sink", 4.0, 0.0, Value::Null),
    ]);
    let (person, source, station, sink) = (u[0], u[1], u[2], u[3]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    // The part there at once; the person on the way, not yet working.
    let t = run(&mut cell, &doc, 0.0, 2.5);
    assert_eq!(cell.stations[&station].crew, Some(person));
    assert!(cell.stations[&station].working.is_none());
    let walking = cell.operator_activities()[0].2;
    assert_eq!(walking, "walk");
    // About 3 m at 1.2 m/s: there and at work by 5 s, done 10 s later.
    let t = run(&mut cell, &doc, t, 3.5);
    assert!(cell.stations[&station].working.is_some());
    assert_eq!(cell.operator_activities()[0].2, "work");
    run(&mut cell, &doc, t, 12.0);
    assert_eq!(cell.flow_summary()["sunk"], 1);
    // Free again where they stand.
    assert_eq!(cell.stations[&station].crew, None);
    assert_eq!(cell.operator_activities()[0].2, "idle");
}

#[test]
fn a_person_carries_parts_by_hand() {
    let (doc, mut cell, u) = floor(&[
        ("operator", 1.0, -2.0, Value::Null),
        ("flow_source", 0.0, 0.0, json!({"feed": 60.0, "carrier": 2})),
        ("flow_station", 5.0, 0.0, json!({"cycle": 5.0})),
        ("flow_sink", 7.0, 0.0, Value::Null),
    ]);
    let (person, source, station, sink) = (u[0], u[1], u[2], u[3]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    // Walked over, picked up (2 s): in hand on the way.
    let t = run(&mut cell, &doc, 0.0, 6.0);
    let in_hand = cell
        .parts()
        .iter()
        .find(|p| matches!(p.flow, Some(Place::Riding { amr, .. }) if amr == person))
        .map(|p| p.world.translation);
    let z = in_hand.expect("a part in hand by 6 s").z;
    assert!(z > 1.0 && z < 1.2, "at waist height: {z}");
    run(&mut cell, &doc, t, 20.0);
    assert_eq!(cell.flow_summary()["sunk"], 1);
    assert!(cell.flow_summary()["problem"].is_null());
}

#[test]
fn an_amr_low_on_charge_goes_to_charge_and_back_to_work() {
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, -1.5, json!({"battery": 1.0, "charge": 0.5})),
        ("amr_stop", 0.0, -1.0, Value::Null),
        ("amr_stop", 6.0, -1.0, Value::Null),
        ("amr_stop", 3.0, -3.0, json!({"charger": 1})),
        ("flow_source", 0.0, 0.0, json!({"feed": 10.0, "carrier": 1})),
        ("flow_station", 6.0, 0.0, json!({"cycle": 1.0})),
        ("flow_sink", 8.0, 0.0, Value::Null),
    ]);
    let (amr, s1, s2, charger, source, station, sink) = (u[0], u[1], u[2], u[3], u[4], u[5], u[6]);
    lanes(&mut cell, &doc, s1, &[s2, charger]);
    lanes(&mut cell, &doc, s2, &[s1, charger]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    // A minute's driving on a full battery: low within a few trips.
    let start = Instant::now();
    let (mut t, mut charged, mut low) = (0.0, false, 1.0f64);
    while t < 600.0 {
        t += 0.1;
        cell.tick(&doc, t, start + Duration::from_secs_f64(t));
        low = low.min(cell.energy.get(&amr).copied().unwrap_or(1.0));
        if cell.charging.contains(&amr) && cell.rovers[&amr].legs.is_empty() {
            charged = true;
            assert!(cell
                .activities(&doc)
                .iter()
                .any(|(o, _, a)| *o == amr && *a == "charge"));
        }
    }
    assert!(low < 0.2, "it ran low: {low}");
    assert!(charged, "and charged");
    // Back at work after: parts kept coming out, the AMR's round trip
    // (some 34 s a part) the pace.
    assert!(
        cell.flow_summary()["sunk"].as_u64().unwrap() >= 14,
        "{}",
        cell.flow_summary()
    );
}

/// Kept busy — parts always waiting, two taken at once — it is still sent
/// to charge once low, not given job after job until flat.
#[test]
fn a_busy_amr_low_on_charge_still_goes_to_charge() {
    let (doc, mut cell, u) = floor(&[
        (
            "amr",
            0.0,
            -1.5,
            json!({"battery": 1.0, "charge": 0.5, "capacity": 2}),
        ),
        ("amr_stop", 0.0, -1.0, Value::Null),
        ("amr_stop", 6.0, -1.0, Value::Null),
        ("amr_stop", 3.0, -3.0, json!({"charger": 1})),
        (
            "flow_source",
            0.0,
            0.0,
            json!({"feed": 3.0, "carrier": 1, "capacity": 6}),
        ),
        (
            "flow_station",
            6.0,
            0.0,
            json!({"cycle": 1.0, "capacity": 4}),
        ),
        ("flow_sink", 8.0, 0.0, Value::Null),
    ]);
    let (amr, s1, s2, charger, source, station, sink) = (u[0], u[1], u[2], u[3], u[4], u[5], u[6]);
    lanes(&mut cell, &doc, s1, &[s2, charger]);
    lanes(&mut cell, &doc, s2, &[s1, charger]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    let start = Instant::now();
    let (mut t, mut charged, mut low) = (0.0, false, 1.0f64);
    while t < 600.0 {
        t += 0.1;
        cell.tick(&doc, t, start + Duration::from_secs_f64(t));
        low = low.min(cell.energy.get(&amr).copied().unwrap_or(1.0));
        charged |= cell.charging.contains(&amr) && cell.rovers[&amr].legs.is_empty();
    }
    assert!(charged, "it charged");
    assert!(low > 0.0, "never run flat: {low}");
}

/// In a running flow, an AMR left standing at a stop with nothing to do
/// moves aside for one that has to pass — else a second and third AMR
/// would only stand in the first one's way.
#[test]
fn an_idle_amr_in_the_way_moves_aside_in_a_running_flow() {
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, -1.0, Value::Null),
        ("amr", 4.0, -1.0, Value::Null),
        ("amr_stop", 0.0, -1.0, Value::Null),
        ("amr_stop", 4.0, -1.0, Value::Null),
        ("amr_stop", 8.0, -1.0, Value::Null),
        ("amr_stop", 4.0, -4.0, Value::Null),
        ("flow_source", 0.0, 0.0, json!({"feed": 20.0, "carrier": 1})),
        ("flow_station", 8.0, 0.0, json!({"cycle": 1.0})),
        ("flow_sink", 10.0, 0.0, Value::Null),
    ]);
    let (one, two, a, b, c, side) = (u[0], u[1], u[2], u[3], u[4], u[5]);
    let (source, station, sink) = (u[6], u[7], u[8]);
    lanes(&mut cell, &doc, a, &[b]);
    lanes(&mut cell, &doc, b, &[a, c, side]);
    lanes(&mut cell, &doc, c, &[b]);
    lanes(&mut cell, &doc, side, &[b]);
    // The second parked on B, the one stop between source and station.
    cell.drive(&doc, two, b).unwrap();
    let t = run(&mut cell, &doc, 0.0, 2.0);
    assert!((at(&cell, two).x - 4.0).abs() < 1e-6);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    // Whichever carries them, one of the two stands aside on the spur at
    // some time: off B, the one way through.
    let (mut t, mut aside) = (t, false);
    for _ in 0..90 {
        t = run(&mut cell, &doc, t, 1.0);
        aside |= [one, two].iter().any(|r| {
            let p = at(&cell, *r);
            (p.x - 4.0).abs() < 1e-6 && (p.y + 4.0).abs() < 1e-6
        });
    }
    let s = cell.flow_summary();
    assert!(s["sunk"].as_u64().unwrap() >= 2, "parts got through: {s}");
    assert!(s["problem"].is_null(), "{s}");
    assert!(aside, "one went aside");
}

/// With nothing to carry, an AMR in a running flow parks on a spur — a
/// stop at a lane's end that serves nothing — and not where others load.
#[test]
fn an_amr_with_nothing_to_do_parks_off_the_way() {
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, -1.0, Value::Null),
        ("amr_stop", 0.0, -1.0, Value::Null),
        ("amr_stop", 6.0, -1.0, Value::Null),
        ("amr_stop", 3.0, -6.0, Value::Null),
        (
            "flow_source",
            0.0,
            0.0,
            json!({"feed": 600.0, "carrier": 1}),
        ),
        ("flow_station", 6.0, 0.0, json!({"cycle": 1.0})),
        ("flow_sink", 8.0, 0.0, Value::Null),
    ]);
    let (amr, a, b, spur, source, station, sink) = (u[0], u[1], u[2], u[3], u[4], u[5], u[6]);
    lanes(&mut cell, &doc, a, &[b]);
    lanes(&mut cell, &doc, b, &[a, spur]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    assert_eq!(
        cell.parking(&doc),
        vec![spur],
        "the spur, not the ends served"
    );
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    // The one part carried, then off to park.
    run(&mut cell, &doc, 0.0, 60.0);
    assert_eq!(cell.flow_summary()["sunk"], 1);
    let p = at(&cell, amr);
    assert!(
        (p.x - 3.0).abs() < 1e-6 && (p.y + 6.0).abs() < 1e-6,
        "parked: {p}"
    );
}

/// A loop driven one way round: three AMRs busy on it never meet head on,
/// and the parts keep coming through.
#[test]
fn amrs_on_a_one_way_loop_never_meet_head_on() {
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, -1.0, json!({"capacity": 1})),
        ("amr", 6.0, -1.0, json!({"capacity": 1})),
        ("amr", 6.0, -5.0, json!({"capacity": 1})),
        ("amr_stop", 0.0, -1.0, Value::Null),
        ("amr_stop", 6.0, -1.0, Value::Null),
        ("amr_stop", 6.0, -5.0, Value::Null),
        ("amr_stop", 0.0, -5.0, Value::Null),
        (
            "flow_source",
            0.0,
            0.0,
            json!({"feed": 4.0, "carrier": 1, "capacity": 20}),
        ),
        (
            "flow_station",
            6.0,
            0.0,
            json!({"cycle": 1.0, "capacity": 10}),
        ),
        ("flow_sink", 8.0, 0.0, Value::Null),
    ]);
    let (a, b, c, d) = (u[3], u[4], u[5], u[6]);
    let (source, station, sink) = (u[7], u[8], u[9]);
    for (from, to) in [(a, b), (b, c), (c, d), (d, a)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "amr_lanes", "object": from.to_string(), "next": [to.to_string()], "oneway": true}),
        );
    }
    assert_eq!(cell.lanes[&b], vec![c], "one way: B leads on to C only");
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    run(&mut cell, &doc, 0.0, 600.0);
    let s = cell.flow_summary();
    assert!(s["problem"].is_null(), "{s}");
    // One loads at A and unloads at B at a time, 5 s each, the others
    // queued behind: a part every 20 s or so, and none stuck.
    assert!(s["sunk"].as_u64().unwrap() >= 25, "{}", s["sunk"]);
}

/// One who works at certain stations carries only what comes from or goes
/// to them: the part is left for one who may work anywhere, though the
/// station's own person stands nearer.
#[test]
fn a_person_kept_to_a_station_carries_only_its_parts() {
    let (doc, mut cell, u) = floor(&[
        ("operator", 0.5, -1.5, Value::Null),
        ("operator", 9.0, -1.5, Value::Null),
        (
            "flow_source",
            0.0,
            0.0,
            json!({"feed": 600.0, "carrier": 2}),
        ),
        ("flow_station", 5.0, 0.0, json!({"cycle": 5.0})),
        ("flow_station", 20.0, 0.0, json!({"cycle": 5.0})),
        ("flow_sink", 22.0, 0.0, Value::Null),
    ]);
    let (kept, free, source, station, other, sink) = (u[0], u[1], u[2], u[3], u[4], u[5]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    ask(
        &mut cell,
        &doc,
        json!({"op": "operator_skills", "object": kept.to_string(), "stations": [other.to_string()]}),
    );
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    run(&mut cell, &doc, 0.0, 1.0);
    let carrying: Vec<u64> = cell.jobs.iter().filter_map(|j| j.amr).collect();
    assert_eq!(
        carrying,
        vec![free],
        "the one free to, not the one beside it"
    );
}

/// A person walking swings their legs and arms, the two legs opposite;
/// standing with nothing to do, they stand as they were made.
#[test]
fn a_person_walks_with_their_legs() {
    let (doc, mut cell, u) = floor(&[
        ("operator", 0.0, -3.0, Value::Null),
        ("flow_source", 0.0, 0.0, json!({"feed": 30.0})),
        ("flow_station", 2.0, 0.0, json!({"cycle": 5.0, "crew": 1})),
        ("flow_sink", 4.0, 0.0, Value::Null),
    ]);
    let (person, source, station, sink) = (u[0], u[1], u[2], u[3]);
    for (from, to) in [(source, station), (station, sink)] {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": from.to_string(), "next": [to.to_string()]}),
        );
    }
    let foot = |cell: &mut Cell, name: &'static str| {
        let mut shown = doc.clone();
        cell.apply(&mut shown);
        let root = id_of(&shown, person).unwrap();
        let shoe = part_named(&shown, root, name).unwrap();
        // Where the shoe is from the person's middle, along their way.
        let body = shown.world_transform(root).inverse();
        (body * shown.world_transform(shoe)).translation
    };
    let (rest_l, rest_r) = (foot(&mut cell, "shoe_left"), foot(&mut cell, "shoe_right"));
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    // The part out at 2 s; on the way to the station, mid-stride.
    let mut t = 0.0;
    let mut apart: f64 = 0.0;
    while t < 4.5 {
        t = run(&mut cell, &doc, t, 0.1);
        let (l, r) = (foot(&mut cell, "shoe_left"), foot(&mut cell, "shoe_right"));
        assert!(
            (l.x - rest_l.x) * (r.x - rest_r.x) <= 1e-9,
            "the legs go opposite ways"
        );
        apart = apart.max((l.x - r.x).abs());
    }
    assert!(apart > 0.3, "a stride: {apart}");
    assert_eq!(cell.operator_activities()[0].2, "walk");
    // Done and free: as made.
    run(&mut cell, &doc, t, 20.0);
    assert_eq!(cell.operator_activities()[0].2, "idle");
    let l = foot(&mut cell, "shoe_left");
    assert!((l - rest_l).length() < 1e-9, "{l} {rest_l}");
}

#[test]
fn two_amrs_head_on_are_got_apart() {
    // A lane A–B, and C off to the side linked to both: one AMR at A, one
    // at B, each sent to the other's stop along A–B.
    let (doc, mut cell, u) = floor(&[
        ("amr", 0.0, 0.0, Value::Null),
        ("amr", 4.0, 0.0, Value::Null),
        ("amr_stop", 0.0, 0.0, Value::Null),
        ("amr_stop", 4.0, 0.0, Value::Null),
        ("amr_stop", 2.0, 3.0, Value::Null),
    ]);
    let (one, two, a, b, c) = (u[0], u[1], u[2], u[3], u[4]);
    lanes(&mut cell, &doc, a, &[b, c]);
    lanes(&mut cell, &doc, c, &[a, b]);
    // Each at its own stop, holding it.
    cell.drive(&doc, one, a).unwrap();
    cell.drive(&doc, two, b).unwrap();
    let t = run(&mut cell, &doc, 0.0, 3.0);
    cell.drive(&doc, one, b).unwrap();
    cell.drive(&doc, two, a).unwrap();
    run(&mut cell, &doc, t, 40.0);
    let (p1, p2) = (at(&cell, one), at(&cell, two));
    assert!((p1.x - 4.0).abs() < 1e-6 && p1.y.abs() < 1e-6, "{p1}");
    assert!(p2.x.abs() < 1e-6 && p2.y.abs() < 1e-6, "{p2}");
}

#[test]
fn operators_work_only_where_they_may_and_on_their_shift() {
    let (doc, mut cell, u) = floor(&[
        (
            "operator",
            2.0,
            -2.0,
            json!({"shift_from": 0.0, "shift_to": 0.5}),
        ),
        ("operator", 6.0, -2.0, Value::Null),
        ("flow_source", 0.0, 0.0, json!({"feed": 20.0})),
        ("flow_station", 2.0, 0.0, json!({"cycle": 5.0, "crew": 1})),
        ("flow_station", 6.0, 0.0, json!({"cycle": 5.0, "crew": 1})),
        ("flow_sink", 8.0, 0.0, Value::Null),
    ]);
    let (anna, bo, source, s1, s2, sink) = (u[0], u[1], u[2], u[3], u[4], u[5]);
    let s = |x: u64| x.to_string();
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_next", "object": s(source), "next": [s(s1)]}),
    );
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_next", "object": s(s1), "next": [s(s2)]}),
    );
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_next", "object": s(s2), "next": [s(sink)]}),
    );
    // Anna at station 1 only, Bo at station 2 only.
    ask(
        &mut cell,
        &doc,
        json!({"op": "operator_skills", "object": s(anna), "stations": [s(s1)]}),
    );
    ask(
        &mut cell,
        &doc,
        json!({"op": "operator_skills", "object": s(bo), "stations": [s(s2)]}),
    );
    assert_eq!(cell.to_saved()[0]["skills"], json!([s(s1)]));
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    let start = Instant::now();
    let (mut t, mut crews) = (0.0, std::collections::BTreeSet::new());
    let mut off = false;
    let mut out_at_half = 0;
    while t < 3600.0 {
        t += 0.2;
        cell.tick(&doc, t, start + Duration::from_secs_f64(t));
        for (station, st) in &cell.stations {
            if let Some(o) = st.crew {
                crews.insert((*station, o));
            }
        }
        off |= cell
            .operator_activities()
            .iter()
            .any(|(o, _, a)| *o == anna && *a == "off");
        if (t - 1800.0).abs() < 0.1 {
            out_at_half = cell.flow_summary()["sunk"].as_u64().unwrap();
        }
    }
    // Each only at their own station.
    assert!(
        crews
            .iter()
            .all(|(st, o)| (*st == s1 && *o == anna) || (*st == s2 && *o == bo)),
        "{crews:?}"
    );
    // Anna's shift is the first half hour: after it, station 1 waits.
    assert!(off);
    assert!(out_at_half > 50, "{out_at_half}");
    let end = cell.flow_summary()["sunk"].as_u64().unwrap();
    assert!(end <= out_at_half + 2, "{out_at_half} then {end}");
}

/// A place no robot can reach still gets its parts: the flow is the
/// user's, never sent another way. The part goes along its link in the
/// time a robot would take, and the place says no robot reaches it.
#[test]
fn parts_no_robot_can_take_still_go_where_the_link_says() {
    let (doc, mut cell, u) = floor(&[
        ("flow_source", 0.0, 0.0, json!({"feed": 2.0, "carrier": 3})),
        ("flow_station", 2.0, 1.0, json!({"cycle": 3.0})),
        ("flow_station", 2.0, -1.0, json!({"cycle": 3.0})),
        ("flow_sink", 4.0, 0.0, json!({})),
    ]);
    ask(&mut cell, &doc, json!({"op": "flow_next", "object": u[0].to_string(), "next": [u[1].to_string(), u[2].to_string()]}));
    for s in [u[1], u[2]] {
        ask(&mut cell, &doc, json!({"op": "flow_next", "object": s.to_string(), "next": [u[3].to_string()]}));
    }
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    let start = Instant::now();
    let (mut t, mut skipped) = (0.0, 0);
    while t < 200.0 {
        t += 0.1;
        cell.tick(&doc, t, start + Duration::from_secs_f64(t));
        // A robot, as `carry` is: the far station out of its reach.
        for (part, _, _, [_, to], _) in cell.robot_jobs(&doc) {
            if to == u[2] {
                cell.robot_skip(part, "닿지 않음".into());
                skipped += 1;
            } else {
                cell.robot_take(part, 1);
                cell.robot_done(part, None);
            }
        }
    }
    let f = |s: u64| cell.components.iter().find(|c| c.object == s).map(|c| cell.flow_json(&doc, c)).unwrap();
    // Both stations worked, as the links say; none sent elsewhere.
    assert!(skipped >= 20, "{skipped}");
    assert!(f(u[1])["made"].as_u64().unwrap() >= 20, "{}", f(u[1]));
    assert!(f(u[2])["made"].as_u64().unwrap() >= 20, "{}", f(u[2]));
    assert!(f(u[2])["unreachable"].is_string());
}

/// Each link says how its parts go and, by robot, which robot: the old
/// sender-wide 보내는 방법 is given to its links; a link set by robot
/// makes jobs for that robot alone; and it is all kept in the file.
#[test]
fn a_link_says_how_its_parts_go_and_which_robot() {
    let (doc, mut cell, u) = floor(&[
        ("flow_source", 0.0, 0.0, json!({"feed": 2.0, "carrier": 1})),
        ("flow_station", 2.0, 1.0, json!({"cycle": 3.0})),
        ("flow_station", 2.0, -1.0, json!({"cycle": 3.0})),
        ("flow_sink", 4.0, 0.0, json!({})),
    ]);
    let s = |x: u64| x.to_string();
    ask(&mut cell, &doc, json!({"op": "flow_next", "object": s(u[0]), "next": [s(u[1])]}));
    // The source's AMR, onto its link; its own let go.
    assert_eq!(cell.carrier(u[0], u[1]).by, 1);
    assert!(cell.components.iter().all(|c| c.props.carrier == 0));
    // A new link, by robot 7: made, and carried so.
    ask(&mut cell, &doc, json!({"op": "flow_carrier", "object": s(u[0]), "to": s(u[2]), "by": 3, "robot": "7"}));
    assert_eq!(cell.carrier(u[0], u[2]), Carrier { by: 3, robot: Some(7) });
    // Kept, and read back.
    let saved = cell.to_saved();
    let mut again = Cell::from_saved(saved);
    again.adopt(&doc);
    assert_eq!(again.carrier(u[0], u[2]), Carrier { by: 3, robot: Some(7) });
    assert_eq!(again.carrier(u[0], u[1]).by, 1);
    // A job along it is for robot 7.
    ask(&mut cell, &doc, json!({"op": "flow_carrier", "object": s(u[0]), "to": s(u[1]), "by": 0}));
    ask(&mut cell, &doc, json!({"op": "flow_next", "object": s(u[0]), "next": [s(u[2])]}));
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    let start = Instant::now();
    let mut t = 0.0;
    while t < 10.0 && cell.robot_jobs(&doc).is_empty() {
        t += 0.1;
        cell.tick(&doc, t, start + Duration::from_secs_f64(t));
    }
    let jobs = cell.robot_jobs(&doc);
    assert!(!jobs.is_empty());
    assert!(jobs.iter().all(|j| j.4 == Some(7)));
    // A link undone takes its carrier with it.
    ask(&mut cell, &doc, json!({"op": "flow_next", "object": s(u[0]), "next": []}));
    assert_eq!(cell.carrier(u[0], u[2]).by, 0);
}
