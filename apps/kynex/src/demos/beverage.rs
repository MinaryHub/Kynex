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

use crate::cell::Cell;
use crate::library::Library;

/// Where the demo is written.
const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/demos/beverage_plant.json");

/// The plant being laid out: the library, and each item placed by its name.
struct Plant {
    library: Library,
    items: Vec<(String, u64)>,
}

impl Plant {
    /// Places `template` named `name` at `at`, turned `yaw`, with `params`,
    /// costing `price` (만원).
    fn place(
        &mut self,
        template: &str,
        name: &str,
        at: [f64; 3],
        yaw: f64,
        params: Value,
        price: f64,
    ) {
        let (reply, _) = self.library.handle(
            &json!({"op": "place", "template": template, "at": at, "yaw": yaw, "params": params})
                .to_string(),
        );
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["ok"], true, "{template}: {reply}");
        let id = reply["item"].as_u64().unwrap();
        let (reply, _) = self
            .library
            .handle(&json!({"op": "rename", "item": id, "name": name}).to_string());
        assert!(reply.contains("\"ok\":true"), "{name}: {reply}");
        if price > 0.0 {
            self.library
                .handle(&json!({"op": "price", "item": id, "price": price}).to_string());
        }
        self.items.push((name.to_string(), id));
    }
}

fn ask(cell: &mut Cell, doc: &Document, req: Value) {
    let a: Value = serde_json::from_str(&cell.handle(doc, &req.to_string())).unwrap();
    assert_eq!(a["ok"], true, "{req}: {a}");
}

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

    // The line, down +X along y = 0, conveyors' tops 1.1 m up.
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
    p.place(
        "pallet_load",
        "프리폼 옥타빈",
        [-59.0, 2.5, 0.0],
        0.0,
        json!({}),
        0.0,
    );
    p.place(
        "flow_source",
        "프리폼 공급",
        [-55.0, 0.0, 0.0],
        0.0,
        json!({"feed": 3.0, "look": 11.0}),
        0.0,
    );
    p.place(
        "blow_molder",
        "블로 성형기",
        [-48.0, 0.0, 0.0],
        0.0,
        json!({}),
        150000.0,
    );
    conveyor(&mut p, "에어 컨베이어", -44.5, -30.2);
    p.place(
        "flow_station",
        "블로 성형",
        [-43.8, 0.0, 0.2],
        0.0,
        json!({"cycle": 3.2, "look": 12.0}),
        0.0,
    );
    p.place(
        "flow_buffer",
        "에어 컨베이어 축적",
        [-37.0, 0.0, 0.5],
        0.0,
        json!({"capacity": 40.0}),
        0.0,
    );
    p.place(
        "filler_capper",
        "세척·충전·캡핑 모노블록",
        [-27.0, 0.0, 0.0],
        0.0,
        json!({}),
        300000.0,
    );
    p.place(
        "flow_station",
        "세척·충전·캡핑",
        [-27.0, 0.0, 0.0],
        0.0,
        json!({"cycle": 3.6, "look": 13.0}),
        0.0,
    );
    p.place(
        "control_panel",
        "충전기 제어반",
        [-31.0, -3.2, 0.0],
        0.0,
        json!({"width": 1.2}),
        0.0,
    );
    conveyor(&mut p, "충전 후 컨베이어", -24.0, -12.5);
    p.place(
        "bottle_inspector",
        "액면·캡 검사기",
        [-20.0, 0.0, 0.0],
        0.0,
        json!({}),
        20000.0,
    );
    p.place(
        "flow_station",
        "액면·캡 검사",
        [-20.0, 0.0, 0.2],
        0.0,
        json!({"cycle": 3.0, "look": 13.0}),
        0.0,
    );
    p.place(
        "labeler",
        "라벨러",
        [-11.0, 0.0, 0.0],
        0.0,
        json!({}),
        50000.0,
    );
    p.place(
        "flow_station",
        "라벨링",
        [-11.0, 0.0, 0.0],
        0.0,
        json!({"cycle": 3.4, "look": 14.0}),
        0.0,
    );
    conveyor(&mut p, "축적 테이블", -9.5, 7.0);
    p.place(
        "flow_buffer",
        "축적 테이블 축적",
        [-1.0, 0.0, 0.5],
        0.0,
        json!({"capacity": 80.0}),
        0.0,
    );
    p.place(
        "shrink_wrapper",
        "수축 포장기",
        [11.0, 0.0, 0.0],
        0.0,
        json!({"length": 8.0}),
        40000.0,
    );
    p.place(
        "flow_station",
        "수축 포장",
        [11.0, 0.0, 0.0],
        0.0,
        json!({"cycle": 3.3, "look": 15.0}),
        0.0,
    );
    conveyor(&mut p, "팩 컨베이어", 15.0, 20.6);
    p.place(
        "palletizer",
        "팔레타이저",
        [23.0, 0.0, 0.0],
        0.0,
        json!({}),
        45000.0,
    );
    p.place(
        "flow_station",
        "팔레타이징",
        [24.2, 0.0, 0.15],
        0.0,
        json!({"cycle": 60.0, "takes": 60.0, "look": 16.0, "base": 1.0}),
        0.0,
    );
    p.place(
        "stretch_wrapper",
        "팔레트 랩핑기",
        [31.0, 0.0, 0.0],
        0.0,
        json!({}),
        8000.0,
    );
    p.place(
        "flow_station",
        "팔레트 랩핑",
        [31.0, 0.0, 0.12],
        0.0,
        json!({"cycle": 60.0, "look": 16.0, "base": 1.0}),
        0.0,
    );

    // Shipping: AMRs take each wrapped pallet to the warehouse.
    p.place(
        "amr_stop",
        "랩핑기 정지 위치",
        [31.0, -3.5, 0.0],
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

/// A role's object, by its item's name.
fn object(roles: &[(u64, String, crate::cell::Kind)], name: &str) -> String {
    roles
        .iter()
        .find(|r| r.1 == name)
        .unwrap_or_else(|| panic!("no role {name}"))
        .0
        .to_string()
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
    // Each machine's time a tray of 24 takes, seconds: the filler the
    // slowest, at 3.6 s — 24,000 bottles an hour —, the rest faster, as a
    // line is laid out (a V); its breakdowns and its scrap.
    role(
        &mut cell,
        "프리폼 공급",
        "source",
        json!({"feed": 3.0, "look": 11, "speed": 2.0}),
    );
    role(
        &mut cell,
        "블로 성형",
        "station",
        json!({"cycle": 3.2, "look": 12, "capacity": 6, "speed": 2.0, "mtbf": 7200.0, "mttr": 360.0, "scrap": 0.003}),
    );
    role(
        &mut cell,
        "에어 컨베이어 축적",
        "buffer",
        json!({"capacity": 40, "speed": 2.0}),
    );
    role(
        &mut cell,
        "세척·충전·캡핑",
        "station",
        json!({"cycle": 3.6, "look": 13, "capacity": 6, "speed": 2.0, "mtbf": 5400.0, "mttr": 600.0}),
    );
    role(
        &mut cell,
        "액면·캡 검사",
        "station",
        json!({"cycle": 3.0, "look": 13, "capacity": 6, "speed": 2.0, "scrap": 0.005}),
    );
    role(
        &mut cell,
        "라벨링",
        "station",
        json!({"cycle": 3.4, "look": 14, "capacity": 6, "speed": 2.0, "mtbf": 3600.0, "mttr": 300.0}),
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
        json!({"cycle": 3.3, "look": 15, "capacity": 6, "speed": 2.0, "mtbf": 7200.0, "mttr": 300.0}),
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
    let chain = [
        "프리폼 공급",
        "블로 성형",
        "에어 컨베이어 축적",
        "세척·충전·캡핑",
        "액면·캡 검사",
        "라벨링",
        "축적 테이블 축적",
        "수축 포장",
        "팔레타이징",
        "팔레트 랩핑",
        "완제품 창고",
    ];
    for w in chain.windows(2) {
        ask(
            &mut cell,
            &doc,
            json!({"op": "flow_next", "object": o(w[0]), "next": [o(w[1])]}),
        );
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

/// The cell `cell` with the props of role `name` changed by `props`.
fn changed(cell: &[Value], changes: &[(&str, Value)]) -> Vec<Value> {
    let mut cell = cell.to_vec();
    for (name, props) in changes {
        let r = cell
            .iter_mut()
            .find(|r| r["name"] == *name)
            .unwrap_or_else(|| panic!("no role {name}"));
        for (k, v) in props.as_object().unwrap() {
            r["props"][k] = v.clone();
        }
    }
    cell
}

#[test]
#[ignore]
fn make_beverage_plant() {
    let (cell, library) = build();
    let snap = |name: &str, cell: Vec<Value>| {
        crate::scenario::snapshot(name, Vec::new(), cell, library.clone())
    };
    let scenarios = vec![
        snap("1. 기준 (설계 24,000병/시)", cell.clone()),
        snap(
            "2. 충전기 속도 향상 (3.6초→3.2초)",
            changed(&cell, &[("세척·충전·캡핑", json!({"cycle": 3.2}))]),
        ),
        snap(
            "3. 라벨러 고장 감소 (MTBF 1시간→4시간)",
            changed(&cell, &[("라벨링", json!({"mtbf": 14400.0}))]),
        ),
        snap(
            "4. 축적 테이블 확대 (80→200 트레이)",
            changed(&cell, &[("축적 테이블 축적", json!({"capacity": 200}))]),
        ),
        snap(
            "5. 충전기 + 라벨러 개선",
            changed(
                &cell,
                &[
                    ("세척·충전·캡핑", json!({"cycle": 3.2})),
                    ("라벨링", json!({"mtbf": 14400.0})),
                ],
            ),
        ),
        snap(
            "6. 블로 성형기 고장 증가 (MTBF 2시간→30분)",
            changed(&cell, &[("블로 성형", json!({"mtbf": 1800.0}))]),
        ),
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
    }
}
