//! The beverage plant demo, built: a PET bottling line for 24,000 bottles
//! an hour, in trays of 24 — preforms blown to bottles, rinsed, filled and
//! capped, inspected, labelled, gathered on an accumulation table, shrink
//! wrapped in packs, palletised sixty packs to a pallet, wrapped, and taken
//! by AMR to the warehouse —, its syrup room beside it, and six scenarios
//! to compare. Run
//!
//!   cargo test -p kynex --release -- --ignored make_beverage_plant --nocapture
//!
//! to write `apps/kynex/demos/beverage_plant.json` anew.

use kreate_scene::Document;
use serde_json::{json, Value};

use super::build::{ask, changed, clashes, object, Plant};
use crate::cell::Cell;
use crate::library::Library;

/// Where the demo is written.
const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/demos/beverage_plant.json");

/// The plant's library: its building, syrup room, line and warehouse.
fn lay_out() -> Plant {
    let mut p = Plant {
        library: Library::default(),
        items: Vec::new(),
    };
    let half = std::f64::consts::FRAC_PI_2;
    // The building: 128 × 52 m, 12 m high, a roll door at each end.
    p.place(
        "factory_wall",
        "벽 북",
        [0.0, 26.0, 0.0],
        0.0,
        json!({"length": 128.0, "height": 12.0}),
        0.0,
    );
    p.place(
        "factory_wall",
        "벽 남",
        [0.0, -26.0, 0.0],
        0.0,
        json!({"length": 128.0, "height": 12.0}),
        0.0,
    );
    p.place(
        "factory_wall",
        "벽 서 북",
        [-64.0, 14.5, 0.0],
        half,
        json!({"length": 23.0, "height": 12.0, "windows": 0.0}),
        0.0,
    );
    p.place(
        "factory_wall",
        "벽 서 남",
        [-64.0, -14.5, 0.0],
        half,
        json!({"length": 23.0, "height": 12.0, "windows": 0.0}),
        0.0,
    );
    p.place(
        "roll_door",
        "프리폼 입고 문",
        [-63.7, 0.0, 0.0],
        half,
        json!({"width": 5.0, "height": 5.0}),
        0.0,
    );
    p.place(
        "factory_wall",
        "벽 동 북",
        [64.0, 11.5, 0.0],
        half,
        json!({"length": 29.0, "height": 12.0, "windows": 0.0}),
        0.0,
    );
    p.place(
        "factory_wall",
        "벽 동 남",
        [64.0, -17.5, 0.0],
        half,
        json!({"length": 17.0, "height": 12.0, "windows": 0.0}),
        0.0,
    );
    p.place(
        "roll_door",
        "출하 문",
        [63.7, -6.0, 0.0],
        half,
        json!({"width": 6.0, "height": 5.0}),
        0.0,
    );
    for k in 0..=10 {
        let x = -60.0 + 12.0 * k as f64;
        for y in [-25.6, 25.6] {
            p.place(
                "factory_column",
                &format!("기둥 {}{}", k + 1, if y < 0.0 { "남" } else { "북" }),
                [x, y, 0.0],
                0.0,
                json!({"height": 12.0}),
                0.0,
            );
        }
    }
    for (k, y) in [-14.0, 0.0, 14.0].into_iter().enumerate() {
        p.place(
            "ceiling_light",
            &format!("천장 조명 {}", k + 1),
            [0.0, y, 0.0],
            0.0,
            json!({"count": 16.0, "spacing": 8.0, "height": 11.5}),
            0.0,
        );
    }
    // The floor: the line's work zone, the aisle, the syrup room, the
    // warehouse.
    p.place(
        "floor_zone",
        "병 라인 구역",
        [-13.0, 0.0, 0.0],
        0.0,
        json!({"kind": 1.0, "length": 94.0, "width": 7.0}),
        0.0,
    );
    p.place(
        "floor_zone",
        "통로",
        [0.0, -7.0, 0.0],
        0.0,
        json!({"kind": 0.0, "length": 124.0, "width": 3.0}),
        0.0,
    );
    p.place(
        "floor_zone",
        "시럽실",
        [-46.0, 17.0, 0.0],
        0.0,
        json!({"kind": 4.0, "length": 32.0, "width": 14.0}),
        0.0,
    );
    p.place(
        "floor_zone",
        "완제품 창고 구역",
        [53.0, 4.0, 0.0],
        0.0,
        json!({"kind": 3.0, "length": 20.0, "width": 30.0}),
        0.0,
    );

    // The syrup room: water treatment, the sugar dissolver, the syrup and
    // blending tanks, and the CIP set that cleans the filler.
    let tanks = [
        ("수처리 탱크 1", -57.0, 21.0, 2.0, 4.0),
        ("수처리 탱크 2", -53.5, 21.0, 2.0, 4.0),
        ("설탕 용해 탱크", -49.0, 21.0, 2.4, 3.0),
        ("시럽 탱크 1", -44.0, 21.0, 2.0, 4.0),
        ("시럽 탱크 2", -40.5, 21.0, 2.0, 4.0),
        ("시럽 탱크 3", -37.0, 21.0, 2.0, 4.0),
        ("조합 탱크 1", -44.0, 14.0, 2.5, 4.0),
        ("조합 탱크 2", -39.5, 14.0, 2.5, 4.0),
        ("CIP 탱크 (가성)", -57.0, 13.0, 1.4, 2.5),
        ("CIP 탱크 (산)", -54.5, 13.0, 1.4, 2.5),
        ("CIP 탱크 (온수)", -52.0, 13.0, 1.4, 2.5),
    ];
    for (name, x, y, d, h) in tanks {
        p.place(
            "process_tank",
            name,
            [x, y, 0.0],
            0.0,
            json!({"diameter": d, "height": h}),
            6000.0,
        );
    }
    p.place(
        "pipe_rack",
        "시럽 배관 (충전기로)",
        [-38.0, 7.5, 0.0],
        0.0,
        json!({"length": 24.0, "height": 5.0}),
        0.0,
    );
    p.place(
        "control_panel",
        "시럽실 제어반",
        [-31.5, 20.0, 0.0],
        0.0,
        json!({"width": 1.6}),
        0.0,
    );

    // The line, down +X along y = 0, conveyors' tops 1.1 m up. Where one
    // machine would hold the line back, two stand side by side, either side
    // of the main conveyor, short conveyors across to each; the main
    // conveyor stops short of each machine in line with it.
    let conveyor = |p: &mut Plant, name: &str, from: f64, to: f64| {
        p.place(
            "bottle_conveyor",
            name,
            [(from + to) / 2.0, 0.0, 0.0],
            0.0,
            json!({"length": to - from, "height": 1.1}),
            2000.0,
        );
    };
    // A conveyor across, at `x`, from the main conveyor's side to `to`
    // (y), its length along Y.
    let across = |p: &mut Plant, name: &str, x: f64, to: f64| {
        let from = 0.3 * to.signum();
        p.place(
            "bottle_conveyor",
            name,
            [x, (from + to) / 2.0, 0.0],
            half,
            json!({"length": (to - from).abs(), "height": 1.1}),
            1000.0,
        );
    };
    p.place(
        "pallet_load",
        "프리폼 옥타빈",
        [-61.0, 3.0, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place(
        "flow_source",
        "프리폼 공급",
        [-58.5, 0.0, 0.0],
        0.0,
        json!({"feed": 2.8, "look": 11.0}),
        0.0,
    );
    conveyor(&mut p, "메인 컨베이어 1 (에어)", -57.0, -26.0);
    conveyor(&mut p, "메인 컨베이어 2", -25.8, 4.8);
    // Two blow moulders, 15,400 bottles an hour each.
    for (k, y) in [(1, 3.4), (2, -3.4)] {
        p.place(
            "blow_molder",
            &format!("블로 성형기 {k}"),
            [-51.0, y, 0.0],
            0.0,
            json!({}),
            90000.0,
        );
        p.place(
            "flow_station",
            &format!("블로 성형 {k}"),
            [-51.0, y, 0.0],
            0.0,
            json!({"cycle": 5.6, "look": 12.0}),
            0.0,
        );
        across(
            &mut p,
            &format!("블로 성형기 {k} 배출"),
            -47.3,
            y - 1.75 * y.signum(),
        );
    }
    p.place(
        "flow_buffer",
        "에어 컨베이어 축적",
        [-40.0, 0.0, 0.5],
        0.0,
        json!({"capacity": 40.0}),
        0.0,
    );
    // Two fillers, 13,500 bottles an hour each: the line's heart, the
    // slowest of its machines by a little.
    for (k, y) in [(1, 3.4), (2, -3.4)] {
        p.place(
            "filler_capper",
            &format!("세척·충전·캡핑 모노블록 {k}"),
            [-31.0, y, 0.0],
            0.0,
            json!({}),
            180000.0,
        );
        p.place(
            "flow_station",
            &format!("세척·충전·캡핑 {k}"),
            [-31.0, y, 0.0],
            0.0,
            json!({"cycle": 6.4, "look": 13.0}),
            0.0,
        );
        across(
            &mut p,
            &format!("충전기 {k} 투입"),
            -35.5,
            y - 2.25 * y.signum(),
        );
        across(
            &mut p,
            &format!("충전기 {k} 배출"),
            -27.4,
            y - 2.25 * y.signum(),
        );
    }
    p.place(
        "bottle_inspector",
        "액면·캡 검사기",
        [-21.0, 0.0, 0.0],
        0.0,
        json!({}),
        20000.0,
    );
    p.place(
        "flow_station",
        "액면·캡 검사",
        [-21.0, 0.0, 0.2],
        0.0,
        json!({"cycle": 3.0, "look": 13.0}),
        0.0,
    );
    // Two labellers.
    for (k, y) in [(1, 2.6), (2, -2.6)] {
        p.place(
            "labeler",
            &format!("라벨러 {k}"),
            [-14.0, y, 0.0],
            0.0,
            json!({}),
            30000.0,
        );
        p.place(
            "flow_station",
            &format!("라벨링 {k}"),
            [-14.0, y, 0.0],
            0.0,
            json!({"cycle": 6.0, "look": 14.0}),
            0.0,
        );
        across(
            &mut p,
            &format!("라벨러 {k} 투입"),
            -16.8,
            y - 1.55 * y.signum(),
        );
        across(
            &mut p,
            &format!("라벨러 {k} 배출"),
            -12.2,
            y - 1.55 * y.signum(),
        );
    }
    p.place(
        "flow_buffer",
        "축적 테이블 축적",
        [-4.0, 0.0, 0.5],
        0.0,
        json!({"capacity": 80.0}),
        0.0,
    );
    p.place(
        "shrink_wrapper",
        "수축 포장기",
        [9.0, 0.0, 0.0],
        0.0,
        json!({"length": 8.0}),
        40000.0,
    );
    p.place(
        "flow_station",
        "수축 포장",
        [9.0, 0.0, 0.0],
        0.0,
        json!({"cycle": 3.0, "look": 15.0}),
        0.0,
    );
    conveyor(&mut p, "팩 컨베이어", 13.2, 18.2);
    p.place(
        "palletizer",
        "팔레타이저",
        [21.0, 0.0, 0.0],
        0.0,
        json!({}),
        45000.0,
    );
    p.place(
        "flow_station",
        "팔레타이징",
        [22.2, 0.0, 0.15],
        0.0,
        json!({"cycle": 60.0, "takes": 60.0, "look": 16.0, "base": 1.0}),
        0.0,
    );
    p.place(
        "stretch_wrapper",
        "팔레트 랩핑기",
        [27.5, 0.0, 0.0],
        0.0,
        json!({}),
        8000.0,
    );
    p.place(
        "flow_station",
        "팔레트 랩핑",
        [27.5, 0.0, 0.12],
        0.0,
        json!({"cycle": 60.0, "look": 16.0, "base": 1.0}),
        0.0,
    );

    // Shipping: AMRs take each wrapped pallet to the warehouse.
    p.place(
        "amr_stop",
        "랩핑기 정지 위치",
        [27.5, -3.5, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place(
        "amr_stop",
        "창고 정지 위치",
        [50.0, -3.5, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place(
        "amr_stop",
        "AMR 충전소",
        [40.0, -10.0, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place("amr", "AMR 1", [40.0, -10.0, 0.0], 0.0, json!({}), 7000.0);
    p.place("amr", "AMR 2", [37.0, -10.0, 0.0], 0.0, json!({}), 7000.0);
    p.place(
        "flow_sink",
        "완제품 창고",
        [50.0, 0.0, 0.0],
        0.0,
        json!({"base": 1.0}),
        0.0,
    );
    for (k, (x, y)) in [
        (46.0, 12.0),
        (50.0, 12.0),
        (54.0, 12.0),
        (58.0, 12.0),
        (54.0, -2.0),
        (58.0, -2.0),
    ]
    .into_iter()
    .enumerate()
    {
        p.place(
            "pallet_rack",
            &format!("팔레트 랙 {}", k + 1),
            [x, y, 0.0],
            0.0,
            json!({"bays": 1.0, "levels": 4.0, "load": 1.0}),
            0.0,
        );
    }
    p.place("forklift", "지게차", [56.0, 6.0, 0.0], half, json!({}), 0.0);
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
    let o = |name: &str| object(&roles, name);
    let role = |cell: &mut Cell, name: &str, kind: &str, props: Value| {
        ask(
            cell,
            &doc,
            json!({"op": "role", "object": o(name), "kind": kind, "props": props}),
        );
    };
    // Each machine's time a tray of 24 takes, seconds, and how many stand
    // side by side: the line balanced on 1,000 trays an hour (24,000
    // bottles), after each machine's breakdowns — the two fillers just
    // above it, the rest 10–15 % more, as a line is laid out (a V).
    role(
        &mut cell,
        "프리폼 공급",
        "source",
        json!({"feed": 2.8, "look": 11, "speed": 2.0, "rule": 1}),
    );
    for k in [1, 2] {
        role(
            &mut cell,
            &format!("블로 성형 {k}"),
            "station",
            json!({"cycle": 5.6, "look": 12, "capacity": 6, "speed": 2.0, "mtbf": 7200.0, "mttr": 360.0, "scrap": 0.003}),
        );
        role(
            &mut cell,
            &format!("세척·충전·캡핑 {k}"),
            "station",
            json!({"cycle": 6.4, "look": 13, "capacity": 6, "speed": 2.0, "mtbf": 5400.0, "mttr": 600.0}),
        );
        role(
            &mut cell,
            &format!("라벨링 {k}"),
            "station",
            json!({"cycle": 6.0, "look": 14, "capacity": 6, "speed": 2.0, "mtbf": 3600.0, "mttr": 300.0}),
        );
    }
    role(
        &mut cell,
        "에어 컨베이어 축적",
        "buffer",
        json!({"capacity": 40, "speed": 2.0, "rule": 1}),
    );
    role(
        &mut cell,
        "액면·캡 검사",
        "station",
        json!({"cycle": 3.0, "look": 13, "capacity": 6, "speed": 2.0, "scrap": 0.005, "rule": 1}),
    );
    role(
        &mut cell,
        "축적 테이블 축적",
        "buffer",
        json!({"capacity": 80, "speed": 2.0}),
    );
    role(
        &mut cell,
        "수축 포장",
        "station",
        json!({"cycle": 3.0, "look": 15, "capacity": 6, "speed": 2.0, "mtbf": 7200.0, "mttr": 300.0}),
    );
    // Sixty packs to a pallet, five layers of twelve: they wait for it in
    // its queue.
    role(
        &mut cell,
        "팔레타이징",
        "station",
        json!({"cycle": 60.0, "takes": 60, "look": 16, "capacity": 70, "speed": 1.0}),
    );
    role(
        &mut cell,
        "팔레트 랩핑",
        "station",
        json!({"cycle": 60.0, "look": 16, "capacity": 1, "carrier": 1}),
    );
    role(&mut cell, "AMR 충전소", "amr_stop", json!({"charger": 1}));
    // Each stage to the next: one to both of a pair (the one with fewer
    // waiting), both of a pair to the one after.
    let stages: [&[&str]; 11] = [
        &["프리폼 공급"],
        &["블로 성형 1", "블로 성형 2"],
        &["에어 컨베이어 축적"],
        &["세척·충전·캡핑 1", "세척·충전·캡핑 2"],
        &["액면·캡 검사"],
        &["라벨링 1", "라벨링 2"],
        &["축적 테이블 축적"],
        &["수축 포장"],
        &["팔레타이징"],
        &["팔레트 랩핑"],
        &["완제품 창고"],
    ];
    for w in stages.windows(2) {
        let next: Vec<String> = w[1].iter().map(|n| o(n)).collect();
        for from in w[0] {
            ask(
                &mut cell,
                &doc,
                json!({"op": "flow_next", "object": o(from), "next": next}),
            );
        }
    }
    // The AMRs' lanes: the wrapper, the warehouse and the charger, each to
    // each.
    let stops = ["랩핑기 정지 위치", "창고 정지 위치", "AMR 충전소"];
    for s in stops {
        let others: Vec<String> = stops.iter().filter(|t| **t != s).map(|t| o(t)).collect();
        ask(
            &mut cell,
            &doc,
            json!({"op": "amr_lanes", "object": o(s), "next": others}),
        );
    }
    // Half an hour's warm-up; a two-shift year, ten years' write-off; a
    // pallet — sixty packs of 24, 150 won a bottle — brings in 216,000 won.
    ask(
        &mut cell,
        &doc,
        json!({"op": "flow_settings", "work": 0, "rest": 0, "warmup": 1800, "hours": 6000, "years": 10, "margin": 216000}),
    );
    (cell.to_saved(), plant.library.to_saved())
}

#[test]
#[ignore]
fn make_beverage_plant() {
    let (cell, library) = build();
    let snap = |name: &str, cell: Vec<Value>| {
        crate::scenario::snapshot(name, Vec::new(), cell, library.clone())
    };
    let both = |name: &str, props: Value| -> Vec<(String, Value)> {
        vec![
            (format!("{name} 1"), props.clone()),
            (format!("{name} 2"), props),
        ]
    };
    let with = |changes: Vec<(String, Value)>| {
        let refs: Vec<(&str, Value)> = changes
            .iter()
            .map(|(n, v)| (n.as_str(), v.clone()))
            .collect();
        changed(&cell, &refs)
    };
    // The fillers' pair undone: all goes to the first.
    let mut one_filler = cell.clone();
    for r in one_filler.iter_mut() {
        if r["name"] == "에어 컨베이어 축적" {
            r["next"] = json!([r["next"][0].clone()]);
        }
    }
    let scenarios = vec![
        snap("1. 기준 (병렬 균형 배치, 24,000병/시)", cell.clone()),
        snap(
            "2. 라벨러 고장 감소 (MTBF 1시간→4시간)",
            with(both("라벨링", json!({"mtbf": 14400.0}))),
        ),
        snap(
            "3. 축적 테이블 확대 (80→200 트레이)",
            changed(&cell, &[("축적 테이블 축적", json!({"capacity": 200}))]),
        ),
        snap(
            "4. 충전기 속도 향상 (6.4초→5.8초)",
            with(both("세척·충전·캡핑", json!({"cycle": 5.8}))),
        ),
        snap(
            "5. 블로 성형기 고장 증가 (MTBF 2시간→30분)",
            with(both("블로 성형", json!({"mtbf": 1800.0}))),
        ),
        snap("6. 충전기 1대만 운전 (병렬 효과 확인)", one_filler),
    ];
    // How each does over two hours, to see the plant is as it is meant.
    for s in &scenarios {
        let r = crate::scenario::simulate_seeded(&Document::new(), s, 7200.0, 1);
        assert_eq!(r["ok"], true, "{r}");
        println!(
            "{}: {:.1} pallets/h ({:.0} bottles/h), bottleneck {}, per pallet {}",
            s["name"].as_str().unwrap(),
            r["per_hour"].as_f64().unwrap(),
            r["per_hour"].as_f64().unwrap() * 60.0 * 24.0,
            r["bottleneck"],
            r["per_part"]
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

/// The demo's scenarios compared as a user would: four hours, three runs
/// each, the mean and its interval.
#[test]
#[ignore]
fn compare_beverage_plant() {
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(FILE).unwrap()).unwrap();
    for s in saved["scenarios"].as_array().unwrap() {
        let r = crate::scenario::replicate(&Document::new(), s, 4.0 * 3600.0, 3);
        println!(
            "{}: {:.1} ± {:.1} pallets/h, bottleneck {}",
            s["name"].as_str().unwrap(),
            r["per_hour"].as_f64().unwrap(),
            r["per_hour_ci"].as_f64().unwrap(),
            r["bottleneck"]
        );
        // Each machine's share at work, to see the line balanced.
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
fn the_beverage_plant_has_no_clashes() {
    let found = clashes(&mut lay_out());
    assert!(
        found.is_empty(),
        "{} clashes:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// The check finds a clash where there is one: a tank stood on a filler.
#[test]
fn a_tank_on_a_filler_is_a_clash() {
    let mut plant = lay_out();
    plant.place(
        "process_tank",
        "잘못 놓은 탱크",
        [-31.0, 3.4, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    let found = clashes(&mut plant);
    assert_eq!(
        found,
        vec!["세척·충전·캡핑 모노블록 1 ↔ 잘못 놓은 탱크"],
        "{found:?}"
    );
}
