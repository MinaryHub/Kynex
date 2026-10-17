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

#[test]
fn times_vary_as_their_distribution_says_and_repeat_for_a_seed() {
    let mut cell = Cell::default();
    let sample = |cell: &mut Cell, dist: u32, spread: f64| -> (f64, f64) {
        let xs: Vec<f64> = (0..40000).map(|_| cell.draw(60.0, dist, spread)).collect();
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
        (mean, var.sqrt())
    };
    cell.seed_flow(7);
    // Fixed; uniform ±10 (σ 10/√3); triangular ±10 (σ 10/√6); normal σ 5;
    // exponential (σ the mean).
    for (dist, spread, sd) in [
        (0, 10.0, 0.0),
        (1, 10.0, 10.0 / 3f64.sqrt()),
        (2, 10.0, 10.0 / 6f64.sqrt()),
        (3, 5.0, 5.0),
        (4, 0.0, 60.0),
    ] {
        let (m, s) = sample(&mut cell, dist, spread);
        assert!((m - 60.0).abs() < 0.02 * 60.0, "{dist}: mean {m}");
        assert!(
            (s - sd).abs() < 0.03 * sd.max(1.0),
            "{dist}: sd {s}, not {sd}"
        );
    }
    // One seed, one sequence.
    cell.seed_flow(42);
    let a: Vec<f64> = (0..5).map(|_| cell.draw(10.0, 4, 0.0)).collect();
    cell.seed_flow(42);
    let b: Vec<f64> = (0..5).map(|_| cell.draw(10.0, 4, 0.0)).collect();
    assert_eq!(a, b);
}

#[test]
fn breakdowns_and_scrap_show_in_the_station_s_oee() {
    // A station always fed, 10 s a part; it works 100 s between breakdowns
    // on average and takes 50 s to repair; a fifth of what it makes is
    // scrap.
    let (doc, mut cell, u) = line(&[
        ("flow_source", 0.0, json!({"feed": 5.0, "capacity": 3})),
        (
            "flow_station",
            2.0,
            json!({"cycle": 10.0, "capacity": 3, "mtbf": 100.0, "mttr": 50.0, "scrap": 0.2}),
        ),
        ("flow_sink", 4.0, json!({})),
    ]);
    link(&mut cell, &doc, u[0], &[u[1]]);
    link(&mut cell, &doc, u[1], &[u[2]]);
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow", "action": "start", "seed": 3}),
    );
    let start = Instant::now();
    let mut t = 0.0;
    let mut down = false;
    while t < 20000.0 {
        t += 0.25;
        cell.tick(&doc, t, start + Duration::from_secs_f64(t));
        down |= cell.flow_activities()[0].2 == "down";
    }
    assert!(down, "it broke down");
    let s = cell.flow_summary();
    let st = &s["stations"][0];
    // Available two thirds of the time; nearly always at work when it is;
    // four in five good.
    let a = st["availability"].as_f64().unwrap();
    let q = st["quality"].as_f64().unwrap();
    assert!((a - 2.0 / 3.0).abs() < 0.05, "{st}");
    assert!(st["performance"].as_f64().unwrap() > 0.9, "{st}");
    assert!((q - 0.8).abs() < 0.03, "{st}");
    let oee = st["oee"].as_f64().unwrap();
    assert!((oee - a * st["performance"].as_f64().unwrap() * q).abs() < 1e-9);
    // What reached the sink is the good ones.
    assert_eq!(
        s["sunk"].as_u64().unwrap(),
        st["good"].as_u64().unwrap() - (cell.parts().iter().filter(|p| p.finished).count() as u64)
    );
    assert!(s["scrapped"].as_u64().unwrap() > 0);
}

#[test]
fn products_go_their_own_ways_at_their_own_times() {
    // Half A, half B; station 1 takes A only, station 2 B only.
    let (doc, mut cell, u) = line(&[
        ("flow_source", 0.0, json!({"feed": 5.0, "capacity": 3})),
        ("flow_station", 2.0, json!({"cycle": 4.0, "capacity": 5})),
        ("flow_station", 2.0, json!({"cycle": 4.0, "capacity": 5})),
        ("flow_sink", 4.0, json!({})),
    ]);
    link(&mut cell, &doc, u[0], &[u[1], u[2]]);
    link(&mut cell, &doc, u[1], &[u[3]]);
    link(&mut cell, &doc, u[2], &[u[3]]);
    let s = |x: u64| x.to_string();
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_products", "object": s(u[0]), "mix": [1.0, 1.0]}),
    );
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_products", "object": s(u[0]), "to": s(u[1]), "products": [0]}),
    );
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_products", "object": s(u[0]), "to": s(u[2]), "products": [1]}),
    );
    // Kept with the roles.
    let saved = cell.to_saved();
    assert_eq!(saved[0]["mix"], json!([1.0, 1.0]));
    assert_eq!(saved[0]["routes"][s(u[1])], json!([0]));
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    run(&mut cell, &doc, 0.0, 2000.0);
    let f = cell.flow_summary();
    let by: Vec<(String, u64)> = f["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["product"].as_str().unwrap().to_string(),
                p["sunk"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(by.len(), 2, "{f}");
    let (a, b) = (by[0].1 as f64, by[1].1 as f64);
    assert!((a / (a + b) - 0.5).abs() < 0.1, "{by:?}");
    // Each station made only its own.
    assert_eq!(
        cell.stations[&u[1]].made,
        by[0].1 + cell.stations[&u[1]].out.len() as u64 - 0
    );
}

#[test]
fn a_change_of_product_costs_its_setup() {
    // A and B in turn at random, 10 s and 20 s, 5 s to change over: about
    // 15 + 2.5 s a part, kept fed.
    let (doc, mut cell, u) = line(&[
        ("flow_source", 0.0, json!({"feed": 1.0, "capacity": 5})),
        (
            "flow_station",
            2.0,
            json!({"cycle": 99.0, "setup": 5.0, "capacity": 5}),
        ),
        ("flow_sink", 4.0, json!({})),
    ]);
    link(&mut cell, &doc, u[0], &[u[1]]);
    link(&mut cell, &doc, u[1], &[u[2]]);
    let s = |x: u64| x.to_string();
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_products", "object": s(u[0]), "mix": [1.0, 1.0]}),
    );
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_products", "object": s(u[1]), "cycles": [10.0, 20.0]}),
    );
    ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
    run(&mut cell, &doc, 0.0, 3500.0);
    let made = cell.stations[&u[1]].made as f64;
    assert!((made - 3500.0 / 17.5).abs() < 20.0, "{made}");
}

#[test]
fn a_calendar_rests_the_flow_and_a_warm_up_is_left_out() {
    let build = |settings: Value| {
        let (doc, mut cell, u) = line(&[
            ("flow_source", 0.0, json!({"feed": 10.0})),
            ("flow_station", 2.0, json!({"cycle": 5.0})),
            ("flow_sink", 4.0, json!({})),
        ]);
        link(&mut cell, &doc, u[0], &[u[1]]);
        link(&mut cell, &doc, u[1], &[u[2]]);
        let mut req = settings;
        req["op"] = json!("flow_settings");
        ask(&mut cell, &doc, req);
        ask(&mut cell, &doc, json!({"op": "flow", "action": "start"}));
        (doc, cell)
    };
    // Ten minutes at work, ten at rest: half as many out.
    let (doc, mut free) = build(json!({}));
    run(&mut free, &doc, 0.0, 2400.0);
    let (doc2, mut shifts) = build(json!({"work": 10.0, "rest": 10.0}));
    let start = Instant::now();
    let mut t = 0.0;
    let mut rested = false;
    while t < 2400.0 {
        t += 0.1;
        shifts.tick(&doc2, t, start + Duration::from_secs_f64(t));
        rested |= shifts.flow_activities()[0].2 == "rest";
    }
    assert!(rested);
    let (a, b) = (
        free.flow_summary()["sunk"].as_f64().unwrap(),
        shifts.flow_summary()["sunk"].as_f64().unwrap(),
    );
    assert!((b / a - 0.5).abs() < 0.05, "{a} {b}");
    // Rest is no loss of availability.
    let oee = &shifts.flow_summary()["stations"][0];
    assert!(oee["availability"].as_f64().unwrap() > 0.99, "{oee}");
    // A warm-up of 600 s: its parts not counted, said once.
    let (doc3, mut warm) = build(json!({"warmup": 600.0}));
    let start = Instant::now();
    let (mut t, mut told) = (0.0, 0);
    while t < 1200.0 {
        t += 0.1;
        warm.tick(&doc3, t, start + Duration::from_secs_f64(t));
        told += warm.take_warmed() as u32;
    }
    assert_eq!(told, 1);
    let sunk = warm.flow_summary()["sunk"].as_u64().unwrap();
    assert!((59..=61).contains(&sunk), "{sunk}");
}
