//! `kynex bench`: how far KYNEX goes before it slows, measured.
//!
//! ```text
//! kynex bench [--tier cell|line|factory|factory-xl|all] [--browser] [--port 8091]
//! ```
//!
//! Each tier is a synthetic factory: stations of parts laid out on a grid,
//! most parts sharing their mesh with others, as repeated equipment does.
//! Without `--browser` it measures what happens on this side: building the
//! model, encoding the viewer's scene and its updates, and the shared
//! session's snapshot and patches. With `--browser` it also opens the
//! viewer in the default browser — this machine's own GPU — and the page
//! reports its load time and frame rate while it orbits the scene.
//!
//! Results are printed and written to `bench-results/`.
//!
//! The collision check is measured too: the whole scene's contacts, as the
//! viewer works them out after every change, with no robot and with an arm
//! standing in the first station.

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kreate_math::{DQuat, DVec3};
use kreate_scene::{Changes, DatablockKind, Document, Object, ObjectData, ObjectId};
use kynex_model::Model;
use kynex_view::{Server, Status, ViewState};
use serde_json::{json, Value};

/// A synthetic factory's size.
#[derive(Clone, Copy, Debug)]
pub struct Tier {
    pub name: &'static str,
    pub objects: usize,
    /// Triangles each part has: the drawn total is `objects` times this.
    pub triangles: usize,
    /// The share of parts with a mesh of their own; the rest repeat one.
    pub unique: f64,
}

pub const TIERS: [Tier; 4] = [
    Tier {
        name: "cell",
        objects: 500,
        triangles: 400,
        unique: 0.5,
    },
    Tier {
        name: "line",
        objects: 5_000,
        triangles: 200,
        unique: 0.2,
    },
    Tier {
        name: "factory",
        objects: 20_000,
        triangles: 200,
        unique: 0.2,
    },
    Tier {
        name: "factory-xl",
        objects: 50_000,
        triangles: 200,
        unique: 0.2,
    },
];

pub fn run(args: Vec<String>) -> Result<(), String> {
    let mut tier = "all".to_string();
    let mut browser = false;
    let mut open = true;
    let mut port = 8091u16;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--tier" => tier = it.next().ok_or("--tier needs a name")?,
            "--browser" => browser = true,
            // Serve the benchmark page without opening it: for driving it
            // from another browser or a test harness.
            "--no-open" => open = false,
            "--port" => {
                port = it
                    .next()
                    .and_then(|p| p.parse().ok())
                    .ok_or("--port needs a number")?
            }
            a => return Err(format!("unknown option {a:?}")),
        }
    }
    let tiers: Vec<Tier> = if tier == "all" {
        TIERS.to_vec()
    } else {
        vec![*TIERS.iter().find(|t| t.name == tier).ok_or(format!(
            "no tier {tier:?}: cell, line, factory, factory-xl or all"
        ))?]
    };

    let mut results = Vec::new();
    println!("서버 쪽 측정 (각 항목 3회 중 중앙값)\n");
    println!(
        "{:<11} {:>7} {:>9} {:>8} {:>9} {:>9} {:>10} {:>10} {:>9} {:>10} {:>10}",
        "tier",
        "objects",
        "tris",
        "model",
        "view",
        "scene",
        "update×1",
        "update×100",
        "snapshot",
        "idle tick",
        "patch×1",
    );
    println!("{:<11} {:>12} {:>12}", "", "contacts", "+arm");
    let mut docs = Vec::new();
    for tier in &tiers {
        let started = Instant::now();
        let doc = factory(tier);
        let built = started.elapsed();
        let r = measure(tier, &doc);
        println!(
            "{:<11} {:>7} {:>8.2}M {:>6.0}ms {:>7.0}ms {:>7.1}MB {:>5.1}ms/{} {:>6.0}ms/{} {:>5.0}ms/{:.0}MB {:>8.1}ms {:>5.1}ms/{}",
            tier.name,
            tier.objects,
            r["triangles"].as_f64().unwrap() / 1e6,
            r["model_ms"].as_f64().unwrap(),
            r["view_reset_ms"].as_f64().unwrap(),
            r["scene_mb"].as_f64().unwrap(),
            r["update_1_ms"].as_f64().unwrap(),
            bytes(r["update_1_bytes"].as_u64().unwrap()),
            r["update_100_ms"].as_f64().unwrap(),
            bytes(r["update_100_bytes"].as_u64().unwrap()),
            r["snapshot_ms"].as_f64().unwrap(),
            r["snapshot_mb"].as_f64().unwrap(),
            r["idle_tick_ms"].as_f64().unwrap(),
            r["patch_1_ms"].as_f64().unwrap(),
            bytes(r["patch_1_bytes"].as_u64().unwrap()),
        );
        println!(
            "{:<11} {:>10.1}ms {:>10.1}ms",
            "",
            r["contacts_ms"].as_f64().unwrap(),
            r["contacts_arm_ms"].as_f64().unwrap(),
        );
        let mut r = r;
        r["generate_ms"] = json!(ms(built));
        results.push(r);
        docs.push((*tier, doc));
    }
    println!(
        "\n  model: 모델 재구성 · view: 뷰어 장면 구성 · scene: 첫 전송량 · update: 물체 1/100개 이동 시 뷰어 갱신 \
         (세션 패치가 알려 준 물체만)\n  snapshot: 세션 참가 스냅샷 · idle tick: 협업 세션의 50ms 주기 비교 비용 · \
         patch×1: 물체 1개 이동 패치\n  contacts: 장면 전체 충돌 검사(로봇 없음) · +arm: 첫 스테이션에 팔 로봇 하나"
    );

    let mut browser_results = Vec::new();
    if browser {
        browser_results = run_browser(&docs, port, open)?;
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = json!({
        "when_unix": stamp,
        "machine": machine(),
        "server": results,
        "browser": browser_results,
    });
    std::fs::create_dir_all("bench-results").map_err(|e| e.to_string())?;
    let path = format!("bench-results/bench-{stamp}.json");
    std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap())
        .map_err(|e| e.to_string())?;
    println!("\n결과: {path}");
    Ok(())
}

// --- the scene --------------------------------------------------------------

/// A factory of `tier`'s size: stations of 25 parts, 6 m apart, each part
/// placed, turned and sized as no two neighbours are alike.
pub fn factory(tier: &Tier) -> Document {
    let mut rng = Lcg(0x5EED_CAFE ^ tier.objects as u64);
    let mut doc = Document::new();
    let unique = ((tier.objects as f64 * tier.unique).ceil() as usize).max(1);
    let meshes: Vec<_> = (0..unique)
        .map(|i| doc.add_mesh(part_mesh(tier.triangles, i)))
        .collect();
    let per_station = 25;
    let stations = tier.objects.div_ceil(per_station);
    let side = (stations as f64).sqrt().ceil() as usize;
    let mut station: Option<ObjectId> = None;
    for i in 0..tier.objects {
        if i % per_station == 0 {
            let s = i / per_station;
            let mut empty = Object::new(format!("station_{s}"), ObjectData::Empty);
            empty.transform.translation =
                DVec3::new((s % side) as f64 * 6.0, (s / side) as f64 * 6.0, 0.0);
            station = Some(doc.add_object(empty));
        }
        let mesh = meshes[if i < unique {
            i
        } else {
            rng.below(unique as u64) as usize
        }];
        let mut part = Object::new(format!("part_{i}"), ObjectData::Mesh(mesh));
        let s = i / per_station;
        let scale = 0.15 + rng.unit() * 0.35;
        part.transform.translation = DVec3::new(
            (s % side) as f64 * 6.0 + rng.unit() * 5.0 - 2.5,
            (s / side) as f64 * 6.0 + rng.unit() * 5.0 - 2.5,
            scale + rng.unit() * 1.5,
        );
        part.transform.rotation = DQuat::from_rotation_z(rng.unit() * std::f64::consts::TAU);
        part.transform.scale = DVec3::splat(scale);
        let id = doc.add_object(part);
        let _ = doc.set_parent(id, station);
    }
    doc
}

/// A part with about `triangles` triangles: a sphere or a cylinder, so
/// neighbouring meshes differ.
fn part_mesh(triangles: usize, i: usize) -> kreate_mesh::MeshData {
    if i.is_multiple_of(2) {
        // 2·segments·(rings − 1) triangles.
        let rings = ((triangles as f64 / 4.0).sqrt().round() as u32 + 1).max(3);
        let segments = ((triangles as u32) / (2 * (rings - 1))).max(3);
        kreate_mesh::primitives::uv_sphere(1.0, segments, rings)
    } else {
        // 4·segments triangles: the side's two per segment and a cap's one, twice.
        let segments = ((triangles / 4) as u32).max(3);
        kreate_mesh::primitives::cylinder(0.8, 1.6, segments)
    }
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn unit(&mut self) -> f64 {
        self.next() as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// --- this side --------------------------------------------------------------

fn measure(tier: &Tier, doc: &Document) -> Value {
    let triangles: usize = doc
        .objects()
        .filter_map(|(_, o)| o.mesh())
        .filter_map(|m| doc.mesh(m))
        .map(|m| m.triangulate().indices.len() / 3)
        .sum();

    let model_ms = median(|| {
        let t = Instant::now();
        std::hint::black_box(Model::from_document(doc));
        ms(t.elapsed())
    });
    let mut scene_len = 0;
    let view_reset_ms = median(|| {
        let t = Instant::now();
        let mut view = ViewState::new();
        view.reset(doc);
        scene_len = view.scene_bin().len();
        ms(t.elapsed())
    });

    // An update as the live link makes one: from what the session patch
    // names, the parts it moved.
    let parts: Vec<ObjectId> = doc
        .objects()
        .filter(|(_, o)| o.mesh().is_some())
        .map(|(id, _)| id)
        .collect();
    let moved = |n: usize| {
        let mut d = doc.clone();
        for id in parts.iter().take(n) {
            d.object_mut(*id).unwrap().transform.translation.z += 0.1;
        }
        d
    };
    let update = |n: usize| -> (f64, u64) {
        let after = moved(n);
        let named = named(doc, parts.iter().take(n));
        let mut size = 0u64;
        let t = median(|| {
            let mut view = ViewState::new();
            view.reset(doc);
            let t = Instant::now();
            let out = view.update(&after, Some(&named)).unwrap_or_default();
            size = out.len() as u64;
            ms(t.elapsed())
        });
        (t, size)
    };
    let (update_1_ms, update_1_bytes) = update(1);
    let (update_100_ms, update_100_bytes) = update(100);

    // The shared session: what a joiner is sent, what a participant does
    // every 50 ms with nothing changing, and what moving one part costs.
    let mut snapshot_len = 0;
    let snapshot_ms = median(|| {
        let t = Instant::now();
        snapshot_len = kreate_io::sync::encode_all(doc)
            .map(|b| b.len())
            .unwrap_or(0);
        ms(t.elapsed())
    });
    let base = doc.clone();
    let idle_tick_ms = median(|| {
        let t = Instant::now();
        std::hint::black_box(doc.changes_since(&base));
        ms(t.elapsed())
    });
    let clone_ms = median(|| {
        let t = Instant::now();
        std::hint::black_box(doc.clone());
        ms(t.elapsed())
    });
    let after = moved(1);
    let mut patch_len = 0;
    let patch_1_ms = median(|| {
        // As `Replica::outgoing` does it: compare, encode, keep a copy.
        let t = Instant::now();
        let changes = after.changes_since(&base);
        patch_len = kreate_io::sync::encode(&after, &changes)
            .map(|b| b.len())
            .unwrap_or(0);
        std::hint::black_box(after.clone());
        ms(t.elapsed())
    });
    let patch = kreate_io::sync::encode(&after, &after.changes_since(&base)).unwrap();
    let link_apply_ms = median(|| {
        // As KYNEX takes one in.
        let mut mirror = base.clone();
        let t = Instant::now();
        let (p, c) = kreate_io::sync::decode(&patch).unwrap();
        mirror.apply_patch(&p, &c);
        ms(t.elapsed())
    });

    // The viewer's collision check, its shapes already made, as after the
    // first change: with no robot, and with an arm among the parts.
    let contacts = |doc: &Document| {
        let mut collider = crate::collide::Collider::default();
        collider.contacts(doc, &Default::default());
        median(|| {
            let t = Instant::now();
            std::hint::black_box(collider.contacts(doc, &Default::default()));
            ms(t.elapsed())
        })
    };
    let contacts_ms = contacts(doc);
    let contacts_arm_ms = contacts(&with_arm(doc));

    json!({
        "tier": tier.name,
        "objects": tier.objects,
        "contacts_ms": contacts_ms,
        "contacts_arm_ms": contacts_arm_ms,
        "unique_meshes": ((tier.objects as f64 * tier.unique).ceil() as usize).max(1),
        "triangles": triangles,
        "model_ms": model_ms,
        "view_reset_ms": view_reset_ms,
        "scene_mb": scene_len as f64 / 1e6,
        "update_1_ms": update_1_ms,
        "update_1_bytes": update_1_bytes,
        "update_100_ms": update_100_ms,
        "update_100_bytes": update_100_bytes,
        "snapshot_ms": snapshot_ms,
        "snapshot_mb": snapshot_len as f64 / 1e6,
        "document_clone_ms": clone_ms,
        "idle_tick_ms": idle_tick_ms,
        "patch_1_ms": patch_1_ms,
        "patch_1_bytes": patch_len,
        "link_apply_ms": link_apply_ms,
    })
}

/// An arm of two box links, standing among the first station's parts.
const ARM: &str = r#"<?xml version="1.0"?>
<robot name="arm">
  <link name="arm_base">
    <visual><origin xyz="0 0 0.25"/><geometry><box size="0.3 0.3 0.5"/></geometry></visual>
  </link>
  <joint name="arm_turn" type="revolute">
    <parent link="arm_base"/><child link="arm_link"/>
    <origin xyz="0 0 0.5"/><axis xyz="0 0 1"/>
    <limit lower="-3" upper="3" effort="10" velocity="1"/>
  </joint>
  <link name="arm_link">
    <visual><origin xyz="0.6 0 0"/><geometry><box size="1.2 0.12 0.12"/></geometry></visual>
  </link>
</robot>
"#;

fn with_arm(doc: &Document) -> Document {
    let mut doc = doc.clone();
    let none = |_: &std::path::Path| -> Result<kreate_io::ir::ImportScene, String> {
        Ok(kreate_io::ir::ImportScene::new(
            kreate_io::ir::SourceMetadata::new("none"),
        ))
    };
    let arm = kreate_io::urdf::parse_urdf(ARM, std::path::Path::new("."), &none)
        .expect("the bench arm reads");
    kreate_io::build::build_into(&arm, &mut doc);
    doc
}

/// What a session patch moving `parts` names.
fn named<'a>(doc: &Document, parts: impl Iterator<Item = &'a ObjectId>) -> Changes {
    let mut changes = Changes::default();
    for id in parts {
        if let Some(uid) = doc.object_uid(*id) {
            changes.changed.insert((DatablockKind::Object, uid));
        }
    }
    changes
}

// --- the browser ------------------------------------------------------------

fn run_browser(docs: &[(Tier, Document)], port: u16, open: bool) -> Result<Vec<Value>, String> {
    let server = Server::start(&format!("127.0.0.1:{port}"))
        .map_err(|e| format!("could not listen on port {port}: {e}"))?;
    let reports = server.bench_reports();
    server.status(&Status::File {
        path: "benchmark".into(),
    });
    let url = format!("http://127.0.0.1:{}/?bench", server.addr().port());
    println!("\n브라우저 측정: {url}");
    println!("  (기본 브라우저가 열립니다. 측정이 끝날 때까지 창을 앞에 두고 건드리지 마세요.)\n");

    let mut out = Vec::new();
    let mut opened = false;
    for (tier, doc) in docs {
        server.reset(doc);
        if !opened && open {
            crate::view::open_browser(&url);
            opened = true;
        }
        // Parts keep moving as a live session would move them: 1% of them,
        // 20 times a second.
        let mut live = doc.clone();
        let parts: Vec<ObjectId> = live
            .objects()
            .filter(|(_, o)| o.mesh().is_some())
            .map(|(id, _)| id)
            .collect();
        let moving = (parts.len() / 100).clamp(1, 500);
        let named = named(&live, parts.iter().take(moving));
        let started = Instant::now();
        let mut update_ms = Vec::new();
        let report = loop {
            if let Ok(report) = reports.try_recv() {
                break Some(report);
            }
            if started.elapsed() > Duration::from_secs(240) {
                break None;
            }
            if server.viewers() > 0 {
                let t = started.elapsed().as_secs_f64();
                for (k, id) in parts.iter().take(moving).enumerate() {
                    let o = live.object_mut(*id).unwrap();
                    o.transform.translation.z = 0.5 + 0.3 * (t * 2.0 + k as f64).sin();
                }
                let tick = Instant::now();
                server.update(&live, Some(&named));
                update_ms.push(ms(tick.elapsed()));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let Some(report) = report else {
            println!(
                "{:<11} 시간 초과: 브라우저가 결과를 보내지 않았습니다",
                tier.name
            );
            break;
        };
        let mut r: Value = serde_json::from_str(&report).map_err(|e| e.to_string())?;
        if r["error"] == "hidden" {
            println!(
                "{:<11} 측정 못 함: 브라우저 탭이 화면 뒤에 있습니다. 탭을 앞으로 가져오고 다시 실행하세요.",
                tier.name
            );
            break;
        }
        update_ms.sort_by(f64::total_cmp);
        r["tier"] = json!(tier.name);
        r["server_update_ms_median"] =
            json!(update_ms.get(update_ms.len() / 2).copied().unwrap_or(0.0));
        r["moving_objects"] = json!(moving);
        let f = |k: &str| r[k].as_f64().unwrap_or(0.0);
        println!(
            "{:<11} 전체 {:>5.1} fps (p95 {:>5.1} ms, {:.2}M △) · 가까이 {:>5.1} fps (p95 {:>5.1} ms, {:.2}M △) · 첫 화면 {:.0} ms · 드로우 콜 {} · 서버 갱신 {:.0} ms",
            tier.name,
            f("fps"),
            f("frame_ms_p95"),
            f("triangles_drawn") / 1e6,
            f("close_fps"),
            f("close_frame_ms_p95"),
            f("close_triangles_drawn") / 1e6,
            f("first_frame_ms"),
            r["draw_calls"],
            f("server_update_ms_median"),
        );
        out.push(r);
    }
    if let Some(gpu) = out.first().and_then(|r| r["gpu"].as_str()) {
        println!("  GPU: {gpu}");
    }
    Ok(out)
}

// --- small things ------------------------------------------------------------

fn median(mut f: impl FnMut() -> f64) -> f64 {
    let mut v: Vec<f64> = (0..3).map(|_| f()).collect();
    v.sort_by(f64::total_cmp);
    v[1]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn bytes(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}MB", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.0}KB", n as f64 / 1e3)
    } else {
        format!("{n}B")
    }
}

fn machine() -> Value {
    let mut m = BTreeMap::new();
    m.insert("os", std::env::consts::OS.to_string());
    m.insert("arch", std::env::consts::ARCH.to_string());
    m.insert(
        "threads",
        std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_default(),
    );
    m.insert(
        "build",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .to_string(),
    );
    json!(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tier_is_the_size_it_says() {
        let tier = Tier {
            name: "t",
            objects: 60,
            triangles: 200,
            unique: 0.2,
        };
        let doc = factory(&tier);
        let parts = doc.objects().filter(|(_, o)| o.mesh().is_some()).count();
        assert_eq!(parts, 60);
        assert_eq!(doc.mesh_count(), 12);
        // Three stations of 25, the last part-full.
        assert_eq!(doc.object_count(), 60 + 3);
        for (_, mesh) in doc.meshes() {
            let tris = mesh.triangulate().indices.len() / 3;
            assert!((100..=400).contains(&tris), "{tris} triangles");
        }
    }
}
