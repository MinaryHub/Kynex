//! Sample projects, kept in KYNEX itself (`samples/`): one for each thing
//! it does, from a beginner's line on a site plan to a robot's cell, to
//! open and learn from. Opening one opens a copy of it in the projects
//! folder: the sample stays as it is.

/// A sample: its file's name, what it is called, the level it is for
/// (1 beginner, 2 intermediate, 3 expert), what it shows, and the project.
pub struct Sample {
    pub key: &'static str,
    pub title: &'static str,
    pub level: u8,
    pub about: &'static str,
    pub text: &'static str,
}

macro_rules! sample {
    ($key:literal, $title:literal, $level:literal, $about:literal) => {
        Sample {
            key: $key,
            title: $title,
            level: $level,
            about: $about,
            text: include_str!(concat!("../../../samples/", $key, ".json")),
        }
    };
}

pub const SAMPLES: &[Sample] = &[
    sample!(
        "01-line-on-plan",
        "샘플 01 도면 위 기본 라인",
        1,
        "부지 도면 위에 공급 → 공정 → 버퍼 → 공정 → 배출. 처리 시간만 넣고 돌려 병목을 보고, 저장된 시나리오 둘을 비교합니다"
    ),
    sample!(
        "02-cnc-in-flow",
        "샘플 02 CNC를 공정으로",
        1,
        "라이브러리의 CNC 선반을 공정 스테이션으로 이어 처리 시간대로 돌립니다"
    ),
    sample!(
        "03-conveyor-rack",
        "샘플 03 컨베이어와 랙",
        2,
        "공급 → 컨베이어 → 공정 → 랙(버퍼) → 배출: 벨트 위로 부품이 흐릅니다"
    ),
    sample!(
        "04-variation",
        "샘플 04 변동과 반복 실행",
        2,
        "처리 시간 분포·고장·불량, 시나리오를 여러 번 돌려 95% 구간으로 비교"
    ),
    sample!(
        "05-products",
        "샘플 05 다품종",
        2,
        "여러 제품을 비율대로 공급하고, 제품마다 다른 공정으로 보냅니다"
    ),
    sample!(
        "06-orders",
        "샘플 06 주문표",
        2,
        "주문(시각·제품·수량·납기)대로 투입하고 납기 준수를 봅니다"
    ),
    sample!(
        "07-operator",
        "샘플 07 작업자",
        2,
        "작업자가 부품을 나르고, 사람이 있어야 도는 두 공정을 오가며 일합니다"
    ),
    sample!(
        "08-amr",
        "샘플 08 AMR 운반",
        2,
        "AMR이 정지 위치 사이 길을 따라 부품을 나릅니다"
    ),
    sample!(
        "09-robot-carry",
        "샘플 09 로봇이 나르는 공정",
        2,
        "받침대 위 로봇이 공급에서 공정으로 부품을 프로그램 없이 집어 옮깁니다"
    ),
    sample!(
        "10-robot-program",
        "샘플 10 로봇 프로그램",
        3,
        "UR5e와 CNC 옆에서 변수, 반복(while), 조건(만약/아니면), 대기를 쓰는 프로그램"
    ),
    sample!(
        "11-sensors-axes",
        "샘플 11 센서와 동작축",
        3,
        "컨베이어에 포토센서·스토퍼·리프터·직선/회전축: PLC로 움직일 장치들"
    ),
];

pub fn find(key: &str) -> Option<&'static Sample> {
    SAMPLES.iter().find(|s| s.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kreate_scene::Document;
    use serde_json::Value;

    /// Every sample reads, and one that runs by itself runs ten minutes of
    /// the simulation with nothing stopping it; a flow's parts come out.
    #[test]
    fn every_sample_opens_and_runs() {
        for s in SAMPLES {
            // Its title the name of the project it opens as.
            crate::view::project_name(s.title).unwrap_or_else(|e| panic!("{}: {e}", s.key));
            let v: Value =
                serde_json::from_str(s.text).unwrap_or_else(|e| panic!("{}: {e}", s.key));
            assert!(
                v["library"].as_array().is_some_and(|l| !l.is_empty()),
                "{}",
                s.key
            );
            // Equipment to drive by hand or from a PLC: nothing runs by itself.
            let runs = v["programs"].as_array().is_some_and(|p| !p.is_empty())
                || v["cell"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|c| c["kind"] == "source"));
            if !runs {
                continue;
            }
            let r = crate::scenario::simulate_seeded(&Document::new(), &v, 600.0, 1);
            assert_eq!(r["ok"], true, "{}: {r}", s.key);
            let flow = v["cell"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c["kind"] == "sink"));
            if flow {
                assert!(r["parts"].as_u64().unwrap_or(0) > 0, "{}: {r}", s.key);
            }
            println!("{}: {} parts", s.key, r["parts"]);
        }
    }
}
