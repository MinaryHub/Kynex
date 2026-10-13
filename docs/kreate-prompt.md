이 저장소(Kreate)에 KYNEX 연동용 수정 6건(K1–K6)을 구현해 줘. KYNEX는 C:\Users\SMIC\KYNEX에 있는 로봇·공장 시뮬레이터야. Kreate의 공동 편집 세션에 참가자로 붙어서, `kreate-scene`, `kreate-io`, `kreate-collab` 크레이트를 git rev `151137a`로 고정해 쓰고 있어. 항목별 상세 근거, 측정값, 완료 기준은 C:\Users\SMIC\KYNEX\docs\kreate-change-requests.md에 있으니 먼저 읽어 줘. 아래는 그 요약과 작업 규칙이야.

## 작업 규칙

- 지금 main 작업 트리에 커밋하지 않은 변경이 있어(apps/kreate/src/main.rs, shell.rs, snap.rs, locales 등). **이 변경은 건드리지도, 커밋하지도 마.** `git worktree add ../Kreate-kynex -b kynex-integration origin/main`으로 별도 작업 트리를 만들어서 그 안에서만 작업해 줘.
- 항목 하나당 커밋 하나. 순서는 **K3 → K6 → K1 → K2 → K4 → K5**.
- 커밋하기 전에 CI와 같은 검사를 통과시켜: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (Windows에서는 `--exclude kreate` 후 `cargo test -p kreate -- --test-threads=1`).
- 코드와 주석은 이 저장소의 기존 문체를 따라. 주석은 영어 산문으로, 무엇보다 "왜"를 설명해.
- 세션 프로토콜은 `PROTOCOL = 1`을 유지해. 새 필드는 `#[serde(default)]`로 추가해서, 구버전 Kreate나 KYNEX와 섞여도 동작해야 해.
- `.kreate` 파일 형식 변경은 이전 파일 읽기와 왕복(load → save → load)이 모두 깨지지 않아야 해.
- push, PR 생성, 릴리스는 하지 마. 끝나면 커밋 목록과 측정 결과를 보고해 줘.

## K3. 원격 관절 변경이 로봇 리그에 덮어써지는 버그 (먼저)

- 현상: `apps/kreate/src/main.rs`의 `run_drivers()`가 문서가 바뀔 때마다 `doc.sync_robot_rigs()`를 부르는데, 이 함수는 "뼈 → 관절" 방향으로만 동작해. 그래서 리그가 있는 로봇에서 **다른 참가자가 관절 오브젝트의 transform을 바꾼 패치**를 받으면, 다음 프레임에 관절이 뼈 위치로 되돌아가. 그 되돌림이 다시 세션에 퍼져서 두 참가자가 서로 덮어쓰게 돼.
- 수정 방향:
  1. `kreate_collab::Session::tick`이 원격 패치로 바뀐 데이터블록을 알려 주게 해. 예를 들어 `SessionEvent::Changed`에 `Changes`를 싣고, 필요하면 `Replica::incoming`이 `Option<Changes>`를 반환하게 바꿔.
  2. `apps/kreate/src/collab.rs`의 `collab_tick()`에서, 원격으로 바뀐 `RobotPart::Joint` 오브젝트마다 `doc.hold_joint_in_rig(joint, joint.position_of(&transform))`를 `run_drivers()`보다 **먼저** 호출해.
  3. 원격 패치가 리그 포즈(armature)를 바꾼 경우에는 지금처럼 `sync_robot_rigs()`가 관절을 맞추게 둬.
- 테스트: 리그가 있는 2축 로봇에서 참가자 B가 관절을 0.7 rad로 바꾸는 패치를 보내. A에서 `tick`과 `run_drivers`에 해당하는 처리를 10회 돌린 뒤에도 관절과 뼈가 모두 0.7이어야 하고, A가 되돌리는 패치를 보내지 않아야 해(`waiting() == 0`).

## K6. 협업 세션의 O(n²) 성능 문제 (공장 규모에서 Kreate가 멈춤)

- KYNEX에서 측정한 값이야(release, 합성 공장, 부품당 삼각형 약 200개, 메시 공유율 80%).

  | 오브젝트 | `changes_since` (50ms마다 호출) | 1개 이동 패치 | `encode_all` 스냅샷 |
  |---|---|---|---|
  | 5,000 | 35 ms | 40 ms | 59 ms |
  | 20,000 | 483 ms | 530 ms | 628 ms |
  | 50,000 | 3,855 ms | 3,902 ms | 4,104 ms |

- 원인:
  1. `crates/kreate-scene/src/document/sync.rs`의 `changes_since`가 삭제를 `before.keys()` × `self.objects.values().any(...)`로 감지해서 **O(n²)**이야.
  2. 같은 함수가 오브젝트를 `format!("{b:?}") == format!("{object:?}")`로 비교해.
  3. `Document::object_by_uid`와 `mesh_by_uid`(`document.rs`)가 선형 탐색인데, `apply_patch`와 `patch()`가 이걸 오브젝트마다 불러.
- 수정 방향: 삭제 감지는 uid `HashSet`으로, 오브젝트 비교는 `PartialEq` 또는 오브젝트별 수정 세대 번호로 바꿔. uid → id 색인은 `Document`가 직접 유지하게 해. 오브젝트를 추가·삭제·복원·adopt하는 모든 경로에서 색인이 맞게 유지돼야 해.
- `kreate-collab`에 `#[ignore]` 벤치 테스트를 추가해서(1만·5만 오브젝트, 공유 메시 80%) 수정 전후 수치를 커밋 메시지에 남겨 줘.
- 목표: 5만 개에서 유휴 `changes_since` < 1 ms, 1개 이동 패치 < 5 ms, 스냅샷이 선형으로 늘어날 것.
- 추가 확인: 라이브러리 링크(`Linkage::Linked`) 오브젝트를 세션 스냅샷과 패치에 실을 때, 라이브러리 파일이 없는 참가자도 형상 데이터를 받는지 확인해 줘. 못 받는다면 실제 데이터를 포함하도록 고쳐 줘(읽기 전용 표시는 유지).

## K1. 협업 프로토콜: 관찰자(observer) 역할

- `wire.rs`의 `Msg::Hello`와 `Peer`에 `role: "editor" | "observer"`를 추가해(`#[serde(default)]`, 기본값은 editor).
- `relay.rs` 규칙:
  - 관찰자가 보낸 `Patch`는 버려.
  - 아직 장면이 없는 방(`seeded == false`)에 관찰자가 들어오면 연결을 유지하고, 편집자의 첫 패치로 장면이 생기는 즉시 기다리던 관찰자에게 `Snapshot`을 보내.
  - 관찰자 수는 `max_peers`와 따로 세(예: 최대 16명).
- Kreate의 세션 창에 관찰자를 "관찰자" 배지로 표시해(ko·en 로케일 모두).
- 테스트: 관찰자가 먼저 대기하다가 편집자가 들어오면 그 장면을 받고 편집자의 장면은 바뀌지 않는다. 관찰자의 Patch는 다른 참가자에게 전달되지 않는다. role이 없는 Hello는 편집자로 처리된다.

## K2. 세션 자동화

- `apps/kreate/src/connector.rs`에 브리지 메서드 `collab`을 추가해. action은 `host`(port, key, name), `join`(address, key, name), `leave`, `status`이고, `status`는 `{state, hosting, address, peers:[{name,site,role}]}`를 돌려줘. 기존 `App::collab_host`, `collab_join`, `collab_leave`를 재사용해.
- `apps/kreate-mcp`에 이 메서드를 노출하는 MCP 도구 `session`을 추가하고, README의 도구 표에도 넣어.
- 세션을 열면 `bridge.json`과 같은 설정 폴더에 `session.json`(`{address, lan_address, key, pid}`)을 쓰고, 세션을 나가거나 프로그램이 끝나면 지워. 비정상 종료로 남은 파일은 `pid`로 판별할 수 있게 해.

## K4. 오브젝트 사용자 속성

- `kreate-scene`에 `PropValue { Bool, Int(i64), Float(f64), Text(String), Vec3([f64;3]) }`와 `Object.props: BTreeMap<String, PropValue>`를 추가해. `changes_since`가 props 차이를 감지해야 세션에 실려.
- `container.rs`의 `ObjectRecord`에 `props`를 추가해(비어 있으면 생략). 이전 파일과의 호환은 `migrate.rs` 규칙을 따라.
- 오퍼레이터 `object.set_property(name, value)`, `object.remove_property(name)`를 추가해. 실행 취소가 되어야 하고 MCP `run_script`로 부를 수 있어야 해.
- 속성 패널에 "사용자 속성" 섹션을 추가해(보기, 편집, 추가, 삭제).
- URDF 임포트 시 루트 링크에 `urdf.robot_name` 속성으로 `<robot name>`을 보존하고, `write_urdf`는 이 값을 우선 사용해.
- KYNEX는 `kynex.` 접두어 키(`kynex.robot_model`, `kynex.kind`, `kynex.cell` 등)를 쓸 거야. Kreate는 의미를 해석하지 말고 저장하고 전달하기만 해.

## K5. UR 로봇 URDF 임포트 검증

- xacro는 구현하지 마. KYNEX가 미리 펼친 UR URDF를 제공할 거야.
- 펼친 UR5e URDF와 메시(시각용 `.dae`, 충돌용 `.stl`, `package://ur_description/...` 경로)를 테스트 픽스처로 추가해. 라이선스가 문제면 형상을 단순화한 합성 메시로 대체해.
- 확인할 것:
  - `base_link` … `wrist_3_link`, `flange`, `tool0`이 모두 생기고, 고정 관절은 `JointKind::Fixed`로 들어온다.
  - 관절 한계 ±2π와 effort·velocity가 보존된다.
  - DAE 재질 색이 반영된다.
  - 충돌 메시가 `RobotPart::Collision`이면서 숨김 상태다.
- 순기구학 테스트: 관절 값 q = [0.3, −1.2, 1.5, −0.8, 1.1, 0.4]에서 `tool0`의 월드 자세가 UR5e DH 순기구학과 1e-6 안에서 일치해야 해. DH 파라미터는 d1 = 0.1625, a2 = −0.425, a3 = −0.3922, d4 = 0.1333, d5 = 0.0997, d6 = 0.0996, α = [π/2, 0, 0, π/2, −π/2, 0].

## 끝나면 보고할 것

- 항목별 커밋 해시와 한 줄 요약
- K6 수정 전후 벤치 수치 표
- 구현하지 못했거나 범위를 바꾼 부분이 있으면 그 이유
- KYNEX 쪽에서 맞춰야 할 API 변경(예: `SessionEvent::Changed`의 시그니처, `Hello.role`)
