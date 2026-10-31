//! The block assembly demo, built: blocks of three colours — yellow, red
//! and blue — kept on a slide rack in the material store, picked at random
//! by a robot onto the outfeed conveyor; AMRs with a jointed arm take each
//! block to a mock-up machine of its colour — two of each, either side of
//! the aisle —, which keeps it a while as if it worked on it; then to the
//! assembly, where a yellow block and a red or blue one are put together
//! side by side; to the vision inspector, which checks the two colours are
//! as they should be; and to the finished-goods store, onto its shelves.
//! Run
//!
//!   cargo test -p kynex --release -- --ignored make_block_assembly --nocapture
//!
//! to write `apps/kynex/demos/block_assembly.json` anew.

use kreate_scene::Document;
use serde_json::{json, Value};

use super::build::{ask, clashes, object, Plant};
use crate::cell::Cell;
use crate::library::Library;

/// Where the demo is written.
const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/demos/block_assembly.json");

/// The blocks' products: yellow is A, blue B, red E (see
/// `library::blocks::block_colour`).
const YELLOW: u64 = 0;
const BLUE: u64 = 1;
const RED: u64 = 4;

/// The mock-up machines: each one's name, its colour's product, its band
/// (`library::blocks::BANDS`) and where along the aisle it stands.
const MACHINES: [(&str, u64, f64, f64); 3] = [
    ("노랑", YELLOW, 0.0, -3.0),
    ("빨강", RED, 1.0, 0.0),
    ("파랑", BLUE, 2.0, 3.0),
];

/// The AMRs' stops, round the aisle one way: east along its north lane,
/// back west along its south one (name, x, y).
const LOOP: [(&str, f64, f64); 12] = [
    ("컨베이어 정지 위치", -9.4, 0.6),
    ("노랑 북 정지 위치", -3.0, 0.6),
    ("빨강 북 정지 위치", 0.0, 0.6),
    ("파랑 북 정지 위치", 3.0, 0.6),
    ("조립 정지 위치", 6.5, 0.6),
    ("완제품 창고 정지 위치", 12.0, 0.6),
    ("창고 회차 정지 위치", 12.0, -0.6),
    ("검사 정지 위치", 6.5, -0.6),
    ("파랑 남 정지 위치", 3.0, -0.6),
    ("빨강 남 정지 위치", 0.0, -0.6),
    ("노랑 남 정지 위치", -3.0, -0.6),
    ("소재 회차 정지 위치", -9.4, -0.6),
];

/// Where the AMRs park with nothing to do: spurs off the loop's turns, two
/// at each end — a stop has lanes with four others at most — and the stop
/// each is a spur off.
const PARKING: [(f64, f64, &str); 4] = [
    (-10.1, -2.2, "소재 회차 정지 위치"),
    (-8.7, -2.2, "소재 회차 정지 위치"),
    (11.3, -2.2, "창고 회차 정지 위치"),
    (12.7, -2.2, "창고 회차 정지 위치"),
];

/// The plant's library: its building, the material store, the machines,
/// the assembly and inspection, and the finished-goods store.
fn lay_out() -> Plant {
    let mut p = Plant {
        library: Library::default(),
        items: Vec::new(),
    };
    let half = std::f64::consts::FRAC_PI_2;
    // The building: 34 × 18 m, 6 m high, a roll door at the store's end.
    for (name, at, yaw, length) in [
        ("벽 북", [0.0, 9.0, 0.0], 0.0, 34.0),
        ("벽 남", [0.0, -9.0, 0.0], 0.0, 34.0),
        ("벽 서", [-17.0, 0.0, 0.0], half, 18.0),
        ("벽 동 북", [17.0, 5.0, 0.0], half, 8.0),
        ("벽 동 남", [17.0, -5.0, 0.0], half, 8.0),
    ] {
        p.place(
            "factory_wall",
            name,
            at,
            yaw,
            json!({"length": length, "height": 6.0}),
            0.0,
        );
    }
    p.place(
        "roll_door",
        "출하 문",
        [16.7, 0.0, 0.0],
        half,
        json!({"width": 2.0, "height": 3.0}),
        0.0,
    );
    for (k, y) in [-5.0, 0.0, 5.0].into_iter().enumerate() {
        p.place(
            "ceiling_light",
            &format!("천장 조명 {}", k + 1),
            [0.0, y, 0.0],
            0.0,
            json!({"count": 8.0, "spacing": 4.0, "height": 5.6}),
            0.0,
        );
    }
    // The floor: the stores, the machines' area, the assembly and the
    // aisle the AMRs go round.
    for (name, at, kind, l, w) in [
        ("소재 창고 구역", [-12.5, 4.0, 0.0], 3.0, 7.0, 5.0),
        ("가공 구역", [0.0, 0.0, 0.0], 1.0, 10.0, 8.0),
        ("조립·검사 구역", [6.5, 0.0, 0.0], 1.0, 3.0, 8.0),
        ("완제품 창고 구역", [12.0, 3.0, 0.0], 3.0, 7.4, 4.0),
        ("AMR 통로", [1.3, 0.0, 0.0], 0.0, 23.4, 2.0),
    ] {
        p.place(
            "floor_zone",
            name,
            at,
            0.0,
            json!({"kind": kind, "length": l, "width": w}),
            0.0,
        );
    }

    // The material store: blocks come in, wait on the slide rack, and a
    // robot on a pedestal before it picks them onto the outfeed conveyor
    // behind it: out of the rack's open front, turned round, nothing in the
    // way.
    // Blocks come in behind the rack, not across where the robot works.
    p.place(
        "flow_source",
        "소재 입고",
        [-12.5, 5.2, 0.0],
        0.0,
        json!({"feed": 10.0, "look": crate::cell::BLOCK as f64}),
        0.0,
    );
    // A slide rack of one shelf before the robot, its front to it: it
    // picks from above, no shelf over the one it picks from, the posts out
    // of its arm's way as it swings round.
    p.place(
        "flow_rack",
        "소재 슬라이드 랙",
        [-12.5, 4.2, 0.0],
        0.0,
        json!({"levels": 1.0, "capacity": 10.0, "style": 1.0, "width": 2.2}),
        300.0,
    );
    p.place(
        "pedestal",
        "로봇 받침대",
        [-12.5, 3.25, 0.0],
        0.0,
        json!({"height": 0.9, "size": 0.3}),
        0.0,
    );
    p.place(
        "ur10e_tending",
        "픽업 로봇",
        [-12.5, 3.25, 0.9],
        0.0,
        json!({}),
        4500.0,
    );
    p.place(
        "conveyor",
        "배출 컨베이어",
        [-10.75, 2.5, 0.0],
        0.0,
        json!({"length": 2.5, "width": 0.5}),
        800.0,
    );

    // The machines: a yellow, a red and a blue pair, either side of the
    // aisle, their fronts to it, a table before each the AMRs load at.
    for (colour, _, band, x) in MACHINES {
        for (side, y, yaw) in [("북", 1.0, -half), ("남", -1.0, half)] {
            p.place(
                "mock_machine",
                &format!("{colour} 목업 장비 {side}"),
                [x, y * 2.9, 0.0],
                yaw,
                json!({ "band": band }),
                2000.0,
            );
            p.place(
                "flow_station",
                &format!("{colour} 가공 {side}"),
                [x, y * 1.9, 0.0],
                0.0,
                json!({"cycle": 60.0}),
                0.0,
            );
        }
    }

    // The assembly: a jig each side of its table, a yellow block on the
    // left one, a red or blue on the right; a robot behind it puts them
    // together.
    p.place(
        "flow_buffer",
        "좌측 지그 (노랑)",
        [5.75, 1.9, 0.0],
        0.0,
        json!({"capacity": 4.0, "base": 3.0}),
        0.0,
    );
    p.place(
        "flow_buffer",
        "우측 지그 (빨강·파랑)",
        [7.25, 1.9, 0.0],
        0.0,
        json!({"capacity": 4.0, "base": 3.0}),
        0.0,
    );
    p.place(
        "flow_station",
        "조립",
        [6.5, 1.9, 0.0],
        0.0,
        json!({"cycle": 25.0, "look": crate::cell::BLOCKS as f64}),
        0.0,
    );
    p.place(
        "pedestal",
        "조립 로봇 받침대",
        [6.5, 2.9, 0.0],
        0.0,
        json!({"height": 0.3}),
        0.0,
    );
    p.place(
        "ur5e",
        "조립 로봇",
        [6.5, 2.9, 0.3],
        -half,
        json!({}),
        3500.0,
    );

    // The inspection: the vision inspector, a table before it.
    p.place(
        "vision_inspector",
        "비전 검사기",
        [6.5, -2.75, 0.0],
        half,
        json!({}),
        2500.0,
    );
    p.place(
        "flow_station",
        "색상 조합 검사",
        [6.5, -1.9, 0.0],
        0.0,
        json!({"cycle": 15.0}),
        0.0,
    );

    // The finished-goods store: four slide racks of three shelves.
    for (k, x) in [9.45, 11.15, 12.85, 14.55].into_iter().enumerate() {
        p.place(
            "flow_rack",
            &format!("완제품 슬라이드 랙 {}", k + 1),
            [x, 2.2, 0.0],
            0.0,
            json!({"levels": 3.0, "capacity": 30.0, "style": 1.0, "width": 1.6}),
            400.0,
        );
    }

    // The AMRs' stops and their parking places, and the AMRs, each a
    // mobile manipulator, parked.
    for (name, x, y) in LOOP {
        p.place("amr_stop", name, [x, y, 0.0], 0.0, json!({}), 0.0);
    }
    for (k, (x, y, _)) in PARKING.into_iter().enumerate() {
        p.place(
            "amr_stop",
            &format!("AMR 대기 위치 {}", k + 1),
            [x, y, 0.0],
            0.0,
            json!({}),
            0.0,
        );
        p.place(
            "amr",
            &format!("AMR {}", k + 1),
            [x, y, 0.0],
            0.0,
            json!({"arm": 1.0}),
            9000.0,
        );
    }
    p
}

/// The plant built: its cell, the library as saved, ready to save as a
/// project.
fn build() -> (Vec<Value>, Vec<Value>) {
    let mut plant = lay_out();
    let mut doc = Document::new();
    plant.library.apply(&mut doc);
    let roles = plant.library.roles();
    let mut cell = Cell::default();
    for (uid, name, kind) in &roles {
        cell.ensure(*uid, name.clone(), *kind);
    }
    cell.adopt(&doc);
    // The conveyor's role is its belt's, by that piece's name: the one
    // conveyor's, under the item's.
    let belt = roles
        .iter()
        .find(|r| r.2 == crate::cell::Kind::Conveyor)
        .map(|r| r.0.to_string())
        .unwrap();
    let o = |name: &str| match name {
        "배출 컨베이어" => belt.clone(),
        _ => object(&roles, name),
    };
    let role = |cell: &mut Cell, name: &str, kind: &str, props: Value| {
        ask(
            cell,
            &doc,
            json!({"op": "role", "object": o(name), "kind": kind, "props": props}),
        );
    };
    let next = |cell: &mut Cell, from: &str, to: &[&str]| {
        let to: Vec<String> = to.iter().map(|n| o(n)).collect();
        ask(
            cell,
            &doc,
            json!({"op": "flow_next", "object": o(from), "next": to}),
        );
    };
    let only = |cell: &mut Cell, from: &str, to: &str, products: &[u64]| {
        ask(
            cell,
            &doc,
            json!({"op": "flow_products", "object": o(from), "to": o(to), "products": products}),
        );
    };
    let block = crate::cell::BLOCK;
    // A block every 10 s, half of them yellow, a quarter
    // each red and blue: a yellow for every red or blue, as the assembly
    // takes them — drawn from a bag, two yellow, a red and a blue shuffled,
    // so that neither side gets far ahead and fills its jig while the other
    // waits. Faster than the line takes them, the rack fills, and none is
    // put in while it is full: the robot always has blocks to pick from.
    role(
        &mut cell,
        "소재 입고",
        "source",
        json!({"feed": 10.0, "look": block, "capacity": 1, "policy": 1}),
    );
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_products", "object": o("소재 입고"), "mix": [2.0, 1.0, 0.0, 0.0, 1.0]}),
    );
    next(&mut cell, "소재 입고", &["소재 슬라이드 랙"]);
    // The robot picks from the rack onto the conveyor's start.
    role(
        &mut cell,
        "소재 슬라이드 랙",
        "buffer",
        json!({"capacity": 10, "carrier": 3}),
    );
    next(&mut cell, "소재 슬라이드 랙", &["배출 컨베이어"]);
    // From the conveyor's end, by AMR, to a machine of the block's colour,
    // the two of a pair sharing them evenly.
    role(
        &mut cell,
        "배출 컨베이어",
        "conveyor",
        json!({"speed": 0.25, "carrier": 1, "rule": 3}),
    );
    let all: Vec<String> = MACHINES
        .iter()
        .flat_map(|(c, ..)| [format!("{c} 가공 북"), format!("{c} 가공 남")])
        .collect();
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    next(&mut cell, "배출 컨베이어", &all);
    for (colour, product, ..) in MACHINES {
        for side in ["북", "남"] {
            let station = format!("{colour} 가공 {side}");
            only(&mut cell, "배출 컨베이어", &station, &[product]);
            // A minute in the machine, give or take ten seconds; then by
            // AMR to the assembly's jig for its colour.
            role(
                &mut cell,
                &station,
                "station",
                json!({"cycle": 60.0, "dist": 1, "spread": 10.0, "carrier": 1, "capacity": 2}),
            );
            let jig = if product == YELLOW {
                "좌측 지그 (노랑)"
            } else {
                "우측 지그 (빨강·파랑)"
            };
            next(&mut cell, &station, &[jig]);
        }
    }
    // A block from each jig, the yellow first, put together side by side.
    for jig in ["좌측 지그 (노랑)", "우측 지그 (빨강·파랑)"] {
        role(&mut cell, jig, "buffer", json!({"capacity": 4}));
        next(&mut cell, jig, &["조립"]);
    }
    role(
        &mut cell,
        "조립",
        "station",
        json!({"cycle": 25.0, "join": 1, "look": crate::cell::BLOCKS, "carrier": 1}),
    );
    next(&mut cell, "조립", &["색상 조합 검사"]);
    // The inspection: one in fifty not as it should be, taken out.
    role(
        &mut cell,
        "색상 조합 검사",
        "station",
        json!({"cycle": 15.0, "scrap": 0.02, "carrier": 1}),
    );
    next(
        &mut cell,
        "색상 조합 검사",
        &[
            "완제품 슬라이드 랙 1",
            "완제품 슬라이드 랙 2",
            "완제품 슬라이드 랙 3",
            "완제품 슬라이드 랙 4",
        ],
    );
    for k in 1..=4 {
        role(
            &mut cell,
            &format!("완제품 슬라이드 랙 {k}"),
            "buffer",
            json!({"capacity": 30}),
        );
    }
    // Each process, the machine it is done on.
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (colour, ..) in MACHINES {
        for side in ["북", "남"] {
            pairs.push((format!("{colour} 가공 {side}"), format!("{colour} 목업 장비 {side}")));
        }
    }
    pairs.push(("색상 조합 검사".into(), "비전 검사기".into()));
    let pairs: Vec<(&str, &str)> = pairs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    super::build::hosts(&plant, &mut cell, &doc, &roles, &pairs);
    // The AMRs' lanes: round the loop one way, each parking place a spur
    // off one of its ends, both ways.
    for k in 0..LOOP.len() {
        let (a, b) = (LOOP[k].0, LOOP[(k + 1) % LOOP.len()].0);
        ask(
            &mut cell,
            &doc,
            json!({"op": "amr_lane", "from": o(a), "to": o(b), "way": "to"}),
        );
    }
    for (k, (.., off)) in PARKING.into_iter().enumerate() {
        ask(
            &mut cell,
            &doc,
            json!({"op": "amr_lane", "from": o(&format!("AMR 대기 위치 {}", k + 1)), "to": o(off), "way": "both"}),
        );
    }
    for k in 1..=PARKING.len() {
        role(&mut cell, &format!("AMR {k}"), "amr", json!({"speed": 1.2}));
    }
    (cell.to_saved(), plant.library.to_saved())
}

#[test]
#[ignore]
fn make_block_assembly() {
    let (cell, library) = build();
    let scenarios = vec![crate::scenario::snapshot(
        "1. 기준 (로봇 팔 AMR 4대)",
        Vec::new(),
        cell.clone(),
        library.clone(),
    )];
    for s in &scenarios {
        let r = crate::scenario::simulate_seeded(&Document::new(), s, 7200.0, 1);
        assert_eq!(r["ok"], true, "{r}");
        println!("{}: {r}", s["name"].as_str().unwrap());
    }
    let project = json!({
        "version": crate::program::FILE_VERSION,
        "programs": [],
        "cell": cell,
        "library": library,
        "scenarios": scenarios,
        "plc": {},
        "floor": {},
    });
    std::fs::write(FILE, serde_json::to_string_pretty(&project).unwrap()).unwrap();
    println!("written {FILE}");
}

#[test]
fn the_block_assembly_has_no_clashes() {
    let found = clashes(&mut lay_out());
    assert!(
        found.is_empty(),
        "{} clashes:\n{}",
        found.len(),
        found.join("\n")
    );
}
