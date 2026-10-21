//! The oil refinery demo, built: 120,000 barrels of crude a day, in lots
//! of 1,000 barrels — crude unloaded at the jetty into the tank farm,
//! desalted, distilled in the crude unit into four cuts: naphtha
//! hydrotreated and reformed to petrol, kerosene hydrotreated to jet fuel,
//! gas oil hydrotreated to diesel, and the residue distilled again under
//! vacuum, its gas oil cracked in the FCC to petrol and its residue to the
//! heavy fuel oil tank —, the product tanks, the flare, the control room,
//! and six scenarios. The crude unit is the refinery's heart, its pace the
//! design's; every unit after it is sized above the cut it is given, after
//! its breakdowns, so the refinery runs at the crude unit's pace. Run
//!
//!   cargo test -p kynex --release -- --ignored make_refinery --nocapture
//!
//! to write `apps/kynex/demos/refinery.json` anew.

use kreate_scene::Document;
use serde_json::{json, Value};

use super::build::{ask, changed, clashes, object, Plant};
use crate::cell::Cell;
use crate::library::Library;

/// Where the demo is written.
const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/demos/refinery.json");

/// The refinery's library: its tank farms, process units, pipe rack,
/// flare and control room, in the open.
fn lay_out() -> Plant {
    let mut p = Plant {
        library: Library::default(),
        items: Vec::new(),
    };
    // The ground: the process area, the tank farms, the roads.
    p.place(
        "floor_zone",
        "공정 지역",
        [-15.0, 0.0, 0.0],
        0.0,
        json!({"kind": 1.0, "length": 250.0, "width": 110.0}),
        0.0,
    );
    p.place(
        "floor_zone",
        "원유 탱크 지역",
        [-175.0, 0.0, 0.0],
        0.0,
        json!({"kind": 3.0, "length": 60.0, "width": 150.0}),
        0.0,
    );
    p.place(
        "floor_zone",
        "제품 탱크 지역",
        [165.0, -5.0, 0.0],
        0.0,
        json!({"kind": 3.0, "length": 90.0, "width": 140.0}),
        0.0,
    );
    p.place(
        "floor_zone",
        "남쪽 도로",
        [0.0, -68.0, 0.0],
        0.0,
        json!({"kind": 0.0, "length": 420.0, "width": 8.0}),
        0.0,
    );
    p.place(
        "floor_zone",
        "동쪽 도로",
        [118.0, 0.0, 0.0],
        0.0,
        json!({"kind": 0.0, "length": 8.0, "width": 130.0}),
        0.0,
    );

    // Crude: unloaded at the jetty, into three tanks.
    p.place(
        "flow_source",
        "원유 하역 (부두)",
        [-215.0, 0.0, 0.0],
        0.0,
        json!({"feed": 600.0, "look": 17.0}),
        0.0,
    );
    for (k, y) in [(1, -48.0), (2, 0.0), (3, 48.0)] {
        p.place(
            "storage_tank",
            &format!("원유 탱크 {k}"),
            [-175.0, y, 0.0],
            0.0,
            json!({"diameter": 38.0, "height": 16.0}),
            300000.0,
        );
    }
    p.place(
        "flow_buffer",
        "원유 탱크 저장",
        [-175.0, 0.0, 0.0],
        0.0,
        json!({"capacity": 60.0}),
        0.0,
    );

    // The main pipe rack, down the middle, in lengths of 59 m a little
    // apart.
    for (k, x) in [(1, -115.0), (2, -55.0), (3, 5.0), (4, 65.0)] {
        p.place(
            "pipe_rack",
            &format!("파이프 랙 {k}"),
            [x, 0.0, 0.0],
            0.0,
            json!({"length": 59.0, "height": 8.0}),
            0.0,
        );
    }

    // North of the rack: the desalter, the crude and vacuum units, the
    // naphtha unit and the two hydrotreaters.
    p.place(
        "horizontal_vessel",
        "탈염기",
        [-118.0, 30.0, 0.0],
        0.0,
        json!({"diameter": 4.0, "length": 20.0}),
        200000.0,
    );
    p.place(
        "flow_station",
        "탈염",
        [-118.0, 30.0, 0.0],
        0.0,
        json!({"cycle": 640.0, "look": 17.0}),
        0.0,
    );
    p.place(
        "fired_heater",
        "상압 가열로",
        [-92.0, 32.0, 0.0],
        0.0,
        json!({"length": 18.0, "width": 9.0, "height": 16.0}),
        1000000.0,
    );
    p.place(
        "distillation_column",
        "상압 증류탑 (CDU)",
        [-72.0, 32.0, 0.0],
        0.0,
        json!({"diameter": 6.0, "height": 55.0}),
        2000000.0,
    );
    p.place(
        "flow_station",
        "상압 증류",
        [-72.0, 32.0, 0.0],
        0.0,
        json!({"cycle": 720.0, "makes": 4.0, "look": 18.0}),
        0.0,
    );
    p.place(
        "air_cooler",
        "상압 공랭기",
        [-72.0, 14.0, 0.0],
        0.0,
        json!({"length": 24.0, "width": 7.0, "height": 9.0}),
        100000.0,
    );
    p.place(
        "fired_heater",
        "감압 가열로",
        [-52.0, 32.0, 0.0],
        0.0,
        json!({"length": 16.0, "width": 8.0, "height": 14.0}),
        800000.0,
    );
    p.place(
        "distillation_column",
        "감압 증류탑 (VDU)",
        [-34.0, 32.0, 0.0],
        0.0,
        json!({"diameter": 8.0, "height": 40.0}),
        1500000.0,
    );
    p.place(
        "flow_station",
        "감압 증류",
        [-34.0, 32.0, 0.0],
        0.0,
        json!({"cycle": 660.0, "makes": 2.0, "look": 22.0}),
        0.0,
    );
    p.place(
        "air_cooler",
        "감압 공랭기",
        [-34.0, 14.0, 0.0],
        0.0,
        json!({"length": 20.0, "width": 7.0, "height": 9.0}),
        80000.0,
    );
    p.place(
        "fired_heater",
        "개질 가열로",
        [-12.0, 32.0, 0.0],
        0.0,
        json!({"length": 14.0, "width": 8.0, "height": 14.0}),
        600000.0,
    );
    for (k, x) in [(1, 2.0), (2, 9.0), (3, 16.0)] {
        p.place(
            "reactor",
            &format!("개질 반응기 {k}"),
            [x, 30.0, 0.0],
            0.0,
            json!({"diameter": 3.0, "height": 15.0}),
            300000.0,
        );
    }
    p.place(
        "flow_station",
        "나프타 수첨·개질",
        [9.0, 30.0, 0.0],
        0.0,
        json!({"cycle": 650.0, "look": 19.0}),
        0.0,
    );
    p.place(
        "reactor",
        "등유 수첨 반응기",
        [32.0, 32.0, 0.0],
        0.0,
        json!({"diameter": 3.5, "height": 18.0}),
        400000.0,
    );
    p.place(
        "horizontal_vessel",
        "등유 수첨 분리 드럼",
        [32.0, 18.0, 0.0],
        0.0,
        json!({"diameter": 3.0, "length": 10.0}),
        50000.0,
    );
    p.place(
        "flow_station",
        "등유 수첨",
        [32.0, 32.0, 0.0],
        0.0,
        json!({"cycle": 610.0, "look": 20.0}),
        0.0,
    );
    p.place(
        "reactor",
        "경유 수첨 반응기",
        [52.0, 32.0, 0.0],
        0.0,
        json!({"diameter": 4.0, "height": 20.0}),
        500000.0,
    );
    p.place(
        "horizontal_vessel",
        "경유 수첨 분리 드럼",
        [52.0, 18.0, 0.0],
        0.0,
        json!({"diameter": 3.0, "length": 10.0}),
        50000.0,
    );
    p.place(
        "flow_station",
        "경유 수첨",
        [52.0, 32.0, 0.0],
        0.0,
        json!({"cycle": 630.0, "look": 21.0}),
        0.0,
    );

    // South of the rack: the FCC, the control room, the flare.
    p.place(
        "fcc_unit",
        "FCC 장치",
        [-25.0, -35.0, 0.0],
        0.0,
        json!({}),
        8000000.0,
    );
    p.place(
        "flow_station",
        "FCC 분해",
        [-25.0, -35.0, 0.0],
        0.0,
        json!({"cycle": 620.0, "look": 19.0}),
        0.0,
    );
    p.place(
        "control_room",
        "중앙 제어실",
        [-110.0, -38.0, 0.0],
        0.0,
        json!({"length": 30.0, "width": 15.0, "height": 6.0}),
        0.0,
    );
    p.place(
        "flare_stack",
        "플레어 스택",
        [70.0, -45.0, 0.0],
        0.0,
        json!({"height": 90.0}),
        0.0,
    );

    // The product tanks: petrol, jet fuel, diesel and heavy fuel oil, and
    // LPG in two spheres.
    let tanks = [
        ("휘발유 탱크 1", 145.0, 38.0, 28.0),
        ("휘발유 탱크 2", 182.0, 38.0, 28.0),
        ("항공유 탱크", 145.0, -5.0, 24.0),
        ("경유 탱크", 182.0, -5.0, 28.0),
        ("벙커C 탱크", 145.0, -45.0, 24.0),
    ];
    for (name, x, y, d) in tanks {
        p.place(
            "storage_tank",
            name,
            [x, y, 0.0],
            0.0,
            json!({"diameter": d, "height": 15.0}),
            200000.0,
        );
    }
    for (k, x) in [(1, 174.0), (2, 194.0)] {
        p.place(
            "lpg_sphere",
            &format!("LPG 구형 탱크 {k}"),
            [x, -48.0, 0.0],
            0.0,
            json!({"diameter": 16.0}),
            150000.0,
        );
    }
    p.place(
        "flow_sink",
        "휘발유 출하",
        [145.0, 38.0, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place(
        "flow_sink",
        "항공유 출하",
        [145.0, -5.0, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place(
        "flow_sink",
        "경유 출하",
        [182.0, -5.0, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place(
        "flow_sink",
        "벙커C 출하",
        [145.0, -45.0, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p
}

/// The refinery built: its cell, the library as saved.
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
    let o = |name: &str| object(&roles, name);
    let role = |cell: &mut Cell, name: &str, kind: &str, props: Value| {
        ask(
            cell,
            &doc,
            json!({"op": "role", "object": o(name), "kind": kind, "props": props}),
        );
    };
    // Each unit's time a lot takes, seconds, and its breakdowns: the crude
    // unit 720 s — five lots an hour, 120,000 barrels a day —, the units
    // after it each given one cut of each lot, sized 5–15 % above that
    // after their breakdowns. Lots go between units at 3 m/s.
    role(
        &mut cell,
        "원유 하역 (부두)",
        "source",
        json!({"feed": 600.0, "look": 17, "speed": 3.0}),
    );
    role(
        &mut cell,
        "원유 탱크 저장",
        "buffer",
        json!({"capacity": 60, "speed": 3.0}),
    );
    role(
        &mut cell,
        "탈염",
        "station",
        json!({"cycle": 640.0, "look": 17, "capacity": 4, "speed": 3.0, "mtbf": 86400.0, "mttr": 3600.0}),
    );
    // The crude unit: one lot in, four cuts out, one to each unit after
    // it, in turn.
    role(
        &mut cell,
        "상압 증류",
        "station",
        json!({"cycle": 720.0, "makes": 4, "look": 18, "capacity": 4, "speed": 3.0, "rule": 0, "mtbf": 172800.0, "mttr": 7200.0}),
    );
    role(
        &mut cell,
        "나프타 수첨·개질",
        "station",
        json!({"cycle": 650.0, "look": 19, "capacity": 6, "speed": 3.0, "mtbf": 43200.0, "mttr": 3600.0}),
    );
    role(
        &mut cell,
        "등유 수첨",
        "station",
        json!({"cycle": 610.0, "look": 20, "capacity": 6, "speed": 3.0, "mtbf": 86400.0, "mttr": 3600.0}),
    );
    role(
        &mut cell,
        "경유 수첨",
        "station",
        json!({"cycle": 630.0, "look": 21, "capacity": 6, "speed": 3.0, "mtbf": 86400.0, "mttr": 3600.0}),
    );
    // The vacuum unit: the residue in, its gas oil and its residue out.
    role(
        &mut cell,
        "감압 증류",
        "station",
        json!({"cycle": 660.0, "makes": 2, "look": 22, "capacity": 6, "speed": 3.0, "rule": 0, "mtbf": 172800.0, "mttr": 7200.0}),
    );
    role(
        &mut cell,
        "FCC 분해",
        "station",
        json!({"cycle": 620.0, "look": 19, "capacity": 6, "speed": 3.0, "mtbf": 43200.0, "mttr": 5400.0}),
    );
    let link = |cell: &mut Cell, from: &str, to: &[&str]| {
        let next: Vec<String> = to.iter().map(|n| o(n)).collect();
        ask(
            cell,
            &doc,
            json!({"op": "flow_next", "object": o(from), "next": next}),
        );
    };
    link(&mut cell, "원유 하역 (부두)", &["원유 탱크 저장"]);
    link(&mut cell, "원유 탱크 저장", &["탈염"]);
    link(&mut cell, "탈염", &["상압 증류"]);
    link(
        &mut cell,
        "상압 증류",
        &["나프타 수첨·개질", "등유 수첨", "경유 수첨", "감압 증류"],
    );
    link(&mut cell, "나프타 수첨·개질", &["휘발유 출하"]);
    link(&mut cell, "등유 수첨", &["항공유 출하"]);
    link(&mut cell, "경유 수첨", &["경유 출하"]);
    link(&mut cell, "감압 증류", &["FCC 분해", "벙커C 출하"]);
    link(&mut cell, "FCC 분해", &["휘발유 출하"]);
    // Two hours' warm-up, as a refinery fills; running all year, written
    // off over twenty; a lot of product brings in 1,500,000 won.
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_settings", "work": 0, "rest": 0, "warmup": 7200, "hours": 8400, "years": 20, "margin": 1500000}),
    );
    // Each process, the unit it is done in.
    super::build::hosts(
        &plant,
        &mut cell,
        &doc,
        &roles,
        &[
            ("원유 탱크 저장", "원유 탱크 2"),
            ("탈염", "탈염기"),
            ("상압 증류", "상압 증류탑 (CDU)"),
            ("감압 증류", "감압 증류탑 (VDU)"),
            ("나프타 수첨·개질", "개질 반응기 2"),
            ("등유 수첨", "등유 수첨 반응기"),
            ("경유 수첨", "경유 수첨 반응기"),
            ("FCC 분해", "FCC 장치"),
            ("휘발유 출하", "휘발유 탱크 1"),
            ("항공유 출하", "항공유 탱크"),
            ("경유 출하", "경유 탱크"),
            ("벙커C 출하", "벙커C 탱크"),
        ],
    );
    (cell.to_saved(), plant.library.to_saved())
}

#[test]
#[ignore]
fn make_refinery() {
    let (cell, library) = build();
    let snap = |name: &str, cell: Vec<Value>| {
        crate::scenario::snapshot(name, Vec::new(), cell, library.clone())
    };
    let scenarios = vec![
        snap("1. 기준 (원유 12만 배럴/일)", cell.clone()),
        snap(
            "2. 원유 처리 10% 증대 (상압 720초→655초)",
            changed(
                &cell,
                &[
                    ("상압 증류", json!({"cycle": 655.0})),
                    ("원유 하역 (부두)", json!({"feed": 545.0})),
                    ("탈염", json!({"cycle": 580.0})),
                ],
            ),
        ),
        snap(
            "3. FCC 고장 잦음 (MTBF 12시간→4시간)",
            changed(&cell, &[("FCC 분해", json!({"mtbf": 14400.0}))]),
        ),
        snap(
            "4. 개질 설비 증강 (650초→560초)",
            changed(&cell, &[("나프타 수첨·개질", json!({"cycle": 560.0}))]),
        ),
        snap(
            "5. 처리 증대 + 개질 증강",
            changed(
                &cell,
                &[
                    ("상압 증류", json!({"cycle": 655.0})),
                    ("원유 하역 (부두)", json!({"feed": 545.0})),
                    ("탈염", json!({"cycle": 580.0})),
                    ("나프타 수첨·개질", json!({"cycle": 560.0})),
                ],
            ),
        ),
        snap(
            "6. 원유선 입항 지연 (하역 600초→900초)",
            changed(&cell, &[("원유 하역 (부두)", json!({"feed": 900.0}))]),
        ),
    ];
    for s in &scenarios {
        let r = crate::scenario::simulate_seeded(&Document::new(), s, 12.0 * 3600.0, 1);
        assert_eq!(r["ok"], true, "{r}");
        println!(
            "{}: {:.1} product lots/h, bottleneck {}",
            s["name"].as_str().unwrap(),
            r["per_hour"].as_f64().unwrap(),
            r["bottleneck"]
        );
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

/// The demo's scenarios compared as a user would: a day, three runs each,
/// the mean and its interval, and each unit's share at work.
#[test]
#[ignore]
fn compare_refinery() {
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(FILE).unwrap()).unwrap();
    for s in saved["scenarios"].as_array().unwrap() {
        let r = crate::scenario::replicate(&Document::new(), s, 24.0 * 3600.0, 3);
        println!(
            "{}: {:.1} ± {:.1} product lots/h, bottleneck {}",
            s["name"].as_str().unwrap(),
            r["per_hour"].as_f64().unwrap(),
            r["per_hour_ci"].as_f64().unwrap(),
            r["bottleneck"]
        );
        let lanes: Vec<String> = r["machine_lanes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                format!(
                    "{} {:.0}%",
                    l["name"].as_str().unwrap(),
                    l["utilization"].as_f64().unwrap() * 100.0
                )
            })
            .collect();
        println!("    {}", lanes.join(", "));
    }
}

#[test]
fn the_refinery_has_no_clashes() {
    let found = clashes(&mut lay_out());
    assert!(
        found.is_empty(),
        "{} clashes:\n{}",
        found.len(),
        found.join("\n")
    );
}
