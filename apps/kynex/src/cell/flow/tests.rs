use super::*;

fn ask(cell: &mut Cell, doc: &Document, req: Value) -> Value {
    let a: Value = serde_json::from_str(&cell.handle(doc, &req.to_string())).unwrap();
    assert_eq!(a["ok"], true, "{req}: {a}");
    a
}

/// The library's flow equipment, `items` (template, x, its settings),
/// placed along X; each one's role and object.
fn line(items: &[(&str, f64, Value)]) -> (Document, Cell, Vec<u64>) {
    let mut library = crate::library::Library::default();
    for (template, x, params) in items {
        library.handle(
            &json!({"op": "place", "template": template, "at": [x, 0.0, 0.0], "params": params})
                .to_string(),
        );
    }
    let mut doc = Document::new();
    library.apply(&mut doc);
    let mut cell = Cell::default();
    let mut uids = Vec::new();
    for (k, (uid, name, kind)) in library.roles().into_iter().enumerate() {
        cell.ensure(uid, name, kind);
        ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": uid.to_string(), "kind": kind.tag(), "props": items[k].2}),
        );
        uids.push(uid);
    }
    (doc, cell, uids)
}

fn link(cell: &mut Cell, doc: &Document, from: u64, to: &[u64]) {
    let next: Vec<String> = to.iter().map(u64::to_string).collect();
    ask(
        cell,
        doc,
        json!({"op": "flow_next", "object": from.to_string(), "next": next}),
    );
}

/// Runs the cell `seconds` from `from`, by tenths; how long each station
/// was at work, by its object.
fn run(cell: &mut Cell, doc: &Document, from: f64, seconds: f64) -> (f64, BTreeMap<u64, f64>) {
    let start = Instant::now();
    let mut t = from;
    let mut busy = BTreeMap::new();
    while t < from + seconds {
        t += 0.1;
        cell.tick(doc, t, start + Duration::from_secs_f64(t));
        for (uid, _, a) in cell.flow_activities() {
            if a == "cut" {
                *busy.entry(uid).or_insert(0.0) += 0.1;
            }
        }
    }
    (t, busy)
}

#[test]
fn a_line_runs_at_the_pace_of_its_slowest_station() {
    let (doc, mut cell, u) = line(&[
        ("flow_source", 0.0, json!({"feed": 10.0})),
        ("flow_station", 2.0, json!({"cycle": 20.0})),
        ("flow_buffer", 4.0, json!({"capacity": 3})),
        ("flow_station", 6.0, json!({"cycle": 5.0})),
        ("flow_sink", 8.0, json!({})),
    ]);
    link(&mut cell, &doc, u[0], &[u[1]]);
    link(&mut cell, &doc, u[1], &[u[2]]);
    link(&mut cell, &doc, u[2], &[u[3]]);
    link(&mut cell, &doc, u[3], &[u[4]]);
    // A sink sends nothing on; nothing goes back to a source.
    let refused: Value = serde_json::from_str(
        &cell.handle(
            &doc,
            &json!({"op": "flow_next", "object": u[4].to_string(), "next": [u[0].to_string()]})
                .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(refused["ok"], false);
    // Kept with the roles, read back.
    let saved = cell.to_saved();
    assert_eq!(saved[1]["next"][0], u[2].to_string(), "{:?}", saved[1]);

    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    let (_, busy) = run(&mut cell, &doc, 0.0, 300.0);
    let s = cell.flow_summary();
    // One every 20 s, after the first's own way through (about 30 s).
    let sunk = s["sunk"].as_u64().unwrap();
    assert!((12..=14).contains(&sunk), "{s}");
    let lead = s["lead_min"].as_f64().unwrap();
    assert!(lead > 25.0 && lead < 35.0, "{s}");
    // The slow station always at work; the quick one a quarter of the time.
    assert!(busy[&u[1]] > 280.0, "{busy:?}");
    assert!(busy[&u[3]] < 0.3 * 300.0, "{busy:?}");
    // Parts waiting at the source are held there: it makes no more than
    // its way out holds.
    assert!(s["wip"].as_u64().unwrap() <= 5, "{s}");

    // Stopped, it holds; reset, its parts are gone.
    ask(&mut cell, &doc, json!({"op": "flow", "action": "stop"}));
    let before = cell.flow_summary()["sunk"].clone();
    run(&mut cell, &doc, 300.0, 50.0);
    assert_eq!(cell.flow_summary()["sunk"], before);
    ask(&mut cell, &doc, json!({"op": "flow", "action": "reset"}));
    assert!(cell.parts().is_empty());
    assert_eq!(cell.flow_summary()["sunk"], 0);
}

#[test]
fn an_assembly_takes_two_parts_for_one() {
    let (doc, mut cell, u) = line(&[
        ("flow_source", 0.0, json!({"feed": 5.0})),
        ("flow_source", 0.0, json!({"feed": 5.0})),
        (
            "flow_station",
            2.0,
            json!({"cycle": 10.0, "takes": 2, "capacity": 4}),
        ),
        ("flow_sink", 4.0, json!({})),
    ]);
    link(&mut cell, &doc, u[0], &[u[2]]);
    link(&mut cell, &doc, u[1], &[u[2]]);
    link(&mut cell, &doc, u[2], &[u[3]]);
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    run(&mut cell, &doc, 0.0, 100.0);
    let made = cell.stations[&u[2]].made;
    let sunk = cell.flow_summary()["sunk"].as_u64().unwrap();
    assert!(made >= 8, "{made}");
    assert!(sunk + 1 >= made && sunk <= made, "{sunk} of {made}");
    // Two in for each made: what came in is what was used, waits, or is
    // on its way.
    let fed = cell.stations[&u[0]].made + cell.stations[&u[1]].made;
    let left = cell.parts().len() as u64;
    assert_eq!(
        fed,
        2 * made + left - (made - sunk),
        "fed {fed}, made {made}, sunk {sunk}, left {left}"
    );
}

#[test]
fn a_part_goes_to_whichever_station_has_room() {
    let (doc, mut cell, u) = line(&[
        ("flow_source", 0.0, json!({"feed": 5.0})),
        ("flow_station", 2.0, json!({"cycle": 9.0})),
        ("flow_station", 2.0, json!({"cycle": 9.0})),
        ("flow_sink", 4.0, json!({})),
    ]);
    link(&mut cell, &doc, u[0], &[u[1], u[2]]);
    link(&mut cell, &doc, u[1], &[u[3]]);
    link(&mut cell, &doc, u[2], &[u[3]]);
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    let (_, busy) = run(&mut cell, &doc, 0.0, 200.0);
    // Two stations of 9 s share a part every 5 s: both at work, neither
    // left idle.
    assert!(busy[&u[1]] > 150.0 && busy[&u[2]] > 150.0, "{busy:?}");
    let sunk = cell.flow_summary()["sunk"].as_u64().unwrap();
    assert!(sunk >= 36, "{sunk}");
}

#[test]
fn a_station_s_time_set_on_its_equipment_is_its_role_s() {
    let mut library = crate::library::Library::default();
    let (_, placed) = library.handle(
        &json!({"op": "place", "template": "flow_station", "at": [0.0, 0.0, 0.0]}).to_string(),
    );
    assert_eq!(placed.props[0].1["cycle"], 45.0);
    let (_, resized) =
        library.handle(&json!({"op": "resize", "item": 1, "params": {"cycle": 20.0}}).to_string());
    assert_eq!(resized.props[0].1["cycle"], 20.0, "{:?}", resized.props);
}
