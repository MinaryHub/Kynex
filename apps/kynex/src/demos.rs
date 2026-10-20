//! The demo projects that come with KYNEX, built into it: wherever it is
//! installed, however it is packaged, they are there. Each is opened as a
//! copy in the user's projects folder (`paths::projects_dir`), the one
//! built in left as it is: the demo can always be had again as it came.

use std::path::{Path, PathBuf};

/// A demo project: its name (the copy's, too), what it shows, and the
/// project file as saved (`apps/kynex/demos`).
pub struct Demo {
    pub name: &'static str,
    pub about: &'static str,
    pub file: &'static str,
}

/// The demos, in the order they are listed.
pub const DEMOS: &[Demo] = &[
    Demo {
        name: "성능 데모 공장",
        about: "두 라인과 AMR 물류: 병목, 가동률, 시나리오 비교",
        file: include_str!("../demos/performance_demo.json"),
    },
    Demo {
        name: "공장 내부 전시",
        about: "공장 내부와 산업 장비 라이브러리, AMR과 작업자",
        file: include_str!("../demos/factory_interior.json"),
    },
    Demo {
        name: "대규모 물류 창고",
        about: "입고·보관·피킹·출고, AMR 군집과 일방통행 차선",
        file: include_str!("../demos/logistics_warehouse.json"),
    },
    Demo {
        name: "자동차 공장",
        about: "프레스·차체·도장·의장·검사, 공정을 따라가는 차",
        file: include_str!("../demos/car_plant.json"),
    },
    Demo {
        name: "LCD 공장",
        about: "TFT·CF 패턴 반복, 스토커 크레인과 유리 이송 로봇, 셀·모듈",
        file: include_str!("../demos/lcd_fab.json"),
    },
    Demo {
        name: "음료 공장",
        about: "PET 병 라인 24,000병/시: 블로 성형·충전·검사·라벨·수축 포장·팔레타이징, 시럽실, AMR 출하",
        file: include_str!("../demos/beverage_plant.json"),
    },
    Demo {
        name: "반도체 공장",
        about: "FOUP와 천장 반송(OHT), 6개 층 전공정, EDS와 후공정 패키징",
        file: include_str!("../demos/semiconductor_fab.json"),
    },
];

/// The samples, one for each thing KYNEX does, small enough to learn
/// from, each with the level it is for (1 beginner, 2 intermediate, 3
/// expert): opened as the demos are.
pub const SAMPLES: &[(u8, Demo)] = &[
    (
        1,
        Demo {
            name: "샘플 01 도면 위 기본 라인",
            about: "부지 도면 위에 공급 → 공정 → 버퍼 → 공정 → 배출. 처리 시간만 넣고 돌려 병목을 보고, 저장된 시나리오 둘을 비교합니다",
            file: include_str!("../demos/samples/01-line-on-plan.json"),
        },
    ),
    (
        1,
        Demo {
            name: "샘플 02 CNC를 공정으로",
            about: "라이브러리의 CNC 선반을 공정 스테이션으로 이어 처리 시간대로 돌립니다",
            file: include_str!("../demos/samples/02-cnc-in-flow.json"),
        },
    ),
    (
        2,
        Demo {
            name: "샘플 03 컨베이어와 랙",
            about: "공급 → 컨베이어 → 공정 → 랙(버퍼) → 배출: 벨트 위로 부품이 흐릅니다",
            file: include_str!("../demos/samples/03-conveyor-rack.json"),
        },
    ),
    (
        2,
        Demo {
            name: "샘플 04 변동과 반복 실행",
            about: "처리 시간 분포·고장·불량, 시나리오를 여러 번 돌려 95% 구간으로 비교",
            file: include_str!("../demos/samples/04-variation.json"),
        },
    ),
    (
        2,
        Demo {
            name: "샘플 05 다품종",
            about: "여러 제품을 비율대로 공급하고, 제품마다 다른 공정으로 보냅니다",
            file: include_str!("../demos/samples/05-products.json"),
        },
    ),
    (
        2,
        Demo {
            name: "샘플 06 주문표",
            about: "주문(시각·제품·수량·납기)대로 투입하고 납기 준수를 봅니다",
            file: include_str!("../demos/samples/06-orders.json"),
        },
    ),
    (
        2,
        Demo {
            name: "샘플 07 작업자",
            about: "작업자가 부품을 나르고, 사람이 있어야 도는 두 공정을 오가며 일합니다",
            file: include_str!("../demos/samples/07-operator.json"),
        },
    ),
    (
        2,
        Demo {
            name: "샘플 08 AMR 운반",
            about: "AMR이 정지 위치 사이 길을 따라 부품을 나릅니다",
            file: include_str!("../demos/samples/08-amr.json"),
        },
    ),
    (
        2,
        Demo {
            name: "샘플 09 로봇이 나르는 공정",
            about: "받침대 위 로봇이 공급에서 공정으로 부품을 프로그램 없이 집어 옮깁니다",
            file: include_str!("../demos/samples/09-robot-carry.json"),
        },
    ),
    (
        3,
        Demo {
            name: "샘플 10 로봇 프로그램",
            about: "UR5e와 CNC 옆에서 변수, 반복(while), 조건(만약/아니면), 대기를 쓰는 프로그램",
            file: include_str!("../demos/samples/10-robot-program.json"),
        },
    ),
    (
        3,
        Demo {
            name: "샘플 11 센서와 동작축",
            about: "컨베이어에 포토센서·스토퍼·리프터·직선/회전축: 손으로 또는 PLC로 움직일 장치들",
            file: include_str!("../demos/samples/11-sensors-axes.json"),
        },
    ),
];

/// The demo or sample called `name`.
pub fn demo(name: &str) -> Option<&'static Demo> {
    DEMOS
        .iter()
        .chain(SAMPLES.iter().map(|(_, d)| d))
        .find(|d| d.name == name)
}

/// Demo `demo` copied into `dir` for opening: as `<name>.json`, or, that
/// taken — the user's own work, or a copy opened before — `<name> 2.json`
/// and on. Returns the copy's file.
pub fn copy(demo: &Demo, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = (1..=100)
        .map(|n| match n {
            1 => dir.join(format!("{}.json", demo.name)),
            n => dir.join(format!("{} {n}.json", demo.name)),
        })
        .find(|f| !f.exists())
        .ok_or_else(|| format!("{}: 사본이 너무 많습니다", demo.name))?;
    std::fs::write(&file, demo.file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

/// The beverage plant demo's maker (see `demos/beverage.rs`).
#[cfg(test)]
mod beverage;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every demo is a project KYNEX reads as it is now: saved by an older
    /// KYNEX, it must still be read, its scenarios there, and the first of
    /// them run — its equipment placed, its flow started.
    #[test]
    fn every_demo_reads_and_runs() {
        for demo in DEMOS {
            let saved: serde_json::Value = serde_json::from_str(demo.file).unwrap();
            let scenarios = saved["scenarios"].as_array().unwrap();
            assert!(!scenarios.is_empty(), "{}: its scenarios", demo.name);
            let report = crate::scenario::simulate_seeded(
                &kreate_scene::Document::new(),
                &scenarios[0],
                300.0,
                1,
            );
            assert_eq!(report["ok"], true, "{}: {report}", demo.name);
            assert!(report["machines"].as_u64().unwrap() > 0, "{}", demo.name);
            assert!(report["alarm"].is_null(), "{}: {report}", demo.name);
        }
    }

    /// Every sample reads; one that runs by itself — a program, or a flow
    /// fed by a source — runs ten minutes with nothing stopping it, and a
    /// flow's parts come out. (A sample's names are names of projects.)
    #[test]
    fn every_sample_reads_and_runs() {
        for (level, s) in SAMPLES {
            assert!((1..=3).contains(level), "{}", s.name);
            assert!(
                !s.name.contains(|c: char| r#"/\:*?"<>|"#.contains(c)),
                "{}",
                s.name
            );
            let v: serde_json::Value = serde_json::from_str(s.file).unwrap();
            let has = |kind: &str| {
                v["cell"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|c| c["kind"] == kind))
            };
            let runs = v["programs"].as_array().is_some_and(|p| !p.is_empty()) || has("source");
            if !runs {
                continue;
            }
            let r = crate::scenario::simulate_seeded(&kreate_scene::Document::new(), &v, 600.0, 1);
            assert_eq!(r["ok"], true, "{}: {r}", s.name);
            if has("sink") {
                assert!(r["parts"].as_u64().unwrap_or(0) > 0, "{}: {r}", s.name);
            }
        }
    }

    #[test]
    fn a_demo_opened_again_is_a_new_copy_not_over_the_first() {
        let dir = std::env::temp_dir().join(format!("kynex-demo-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let demo = demo("자동차 공장").unwrap();
        let first = copy(demo, &dir).unwrap();
        std::fs::write(&first, "mine").unwrap();
        let second = copy(demo, &dir).unwrap();
        assert_eq!(second, dir.join("자동차 공장 2.json"));
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), demo.file);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
