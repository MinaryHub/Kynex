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
        name: "반도체 공장",
        about: "FOUP와 천장 반송(OHT), 6개 층 전공정, EDS와 후공정 패키징",
        file: include_str!("../demos/semiconductor_fab.json"),
    },
];

/// The demo called `name`.
pub fn demo(name: &str) -> Option<&'static Demo> {
    DEMOS.iter().find(|d| d.name == name)
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
