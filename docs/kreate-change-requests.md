# Kreate 수정 요청 (KYNEX 연동용)

작성일: 2026-10-13 · 대상: Kreate 저장소 (`C:\Users\SMIC\Kreate`, 기준 커밋 `151137a`)

KYNEX는 Kreate의 공동 편집 세션에 참가자로 붙어 장면을 실시간으로 받습니다
(`kreate-collab`, `kreate-scene`, `kreate-io`를 경로 의존성으로 사용). 지금은 Kreate를 수정하지 않고
동작하지만, 다음 단계(양방향 연동, 공장 규모 장면, UR 로봇 OLP)로 가려면 아래 항목을 Kreate 쪽에서
고쳐야 합니다.

**작업 전 주의:** Kreate 작업 트리에 커밋하지 않은 변경이 있습니다
(`apps/kreate/src/main.rs`, `shell.rs`, `snap.rs` 등). 각 항목은 별도 브랜치에서 진행하고,
기존 작업과 섞지 마세요.

## 우선순위 요약

| ID | 항목 | 필요한 KYNEX 단계 | 필수 여부 | 규모 |
|---|---|---|---|---|
| K1 | 협업 프로토콜: 관찰자(observer) 역할 | P0 | 필수 | 중 |
| K2 | 세션 자동화 (브리지·MCP 도구, 세션 정보 파일) | P0 | 필수 | 소 |
| K3 | 원격 관절 변경이 로봇 리그에 덮어써지는 문제 | P2 | **필수 (버그)** | 소 |
| K4 | 오브젝트 사용자 속성 (custom properties) | P1–P3 | 필수 | 중 |
| K5 | UR 로봇 URDF 임포트 검증 | P3 | 필수 | 소 |
| K6 | 공장 규모에서 협업 세션 성능 (측정 결과 O(n²) 확인) | P1 | **필수 (2만 개부터 Kreate가 멈춤)** | 중 |

권장 순서: **K3 → K6 → K1 → K2 → K4 → K5.** K3과 K6은 실제로 확인된 결함이라 먼저 고칩니다.
K6의 원인과 수치는 해당 절에 있습니다.

---

## K1. 협업 프로토콜: 관찰자 역할

### 현재 동작
- `crates/kreate-collab/src/wire.rs`: `Msg::Hello { protocol, name, room, key }`에는 역할 구분이 없습니다.
- `relay.rs`의 `serve()`: 아직 장면이 없는 방(`seeded == false`)에 처음 들어온 참가자는 `Welcome { seeded: false }`를
  받고, 그 참가자의 첫 `Patch`가 세션의 장면이 됩니다.
- 그래서 KYNEX는 `seeded == false`를 받으면 연결을 끊고 1초마다 다시 접속합니다. 빈 장면을 시드로 보내면 나중에
  들어오는 Kreate의 장면을 지워 버리기 때문입니다. 결과적으로 Kreate의 참가자 목록에 KYNEX가 들어왔다
  나갔다 하며 깜박이고, 세션이 열려도 최대 1초 늦게 연결됩니다.

### 요청
1. `Msg::Hello`에 `role` 필드를 추가합니다. 값은 `"editor"`(기본값) 또는 `"observer"`이고,
   `#[serde(default)]`를 붙여 **`PROTOCOL = 1`을 유지**합니다. 이렇게 하면 기존 Kreate 빌드와 섞여도 동작합니다.
2. 릴레이 규칙:
   - 관찰자가 보낸 `Patch`는 적용하지 않고 버립니다. 응답은 `Refused`가 아니라 무시입니다.
   - 아직 장면이 없는 방에 관찰자가 들어오면 `Welcome { seeded: false }`를 보내고 **연결을 유지**합니다.
     편집자의 첫 패치로 방에 장면이 생기는 순간, 기다리던 관찰자들에게 `Snapshot`을 보냅니다.
   - 관찰자는 `max_peers` 계산에서 따로 셉니다(예: 관찰자 최대 16명).
3. `Peer`에 `role`을 추가하고, Kreate의 세션 창에서 관찰자를 "관찰자" 배지로 구분해 표시합니다
   (`apps/kreate/src/collab.rs`, `kreate-ui` 세션 창). 관찰자가 `Look`을 보내지 않으면 시야 표시도 그리지 않습니다.

### 완료 기준 (kreate-collab 테스트)
- 관찰자가 먼저 들어와 기다리는 동안, 편집자가 장면을 가지고 들어오면 관찰자가 그 장면을 받는다.
  편집자의 장면은 바뀌지 않는다.
- 관찰자가 보낸 `Patch`는 다른 참가자에게 전달되지 않는다.
- `role` 필드가 없는 `Hello`(구버전)는 편집자로 처리된다.

KYNEX 쪽 후속 작업: `kynex-link`가 `role: "observer"`로 접속하고, 다시 접속하는 반복 루프를 제거합니다.

---

## K2. 세션 자동화

### 현재 동작
- 세션 열기는 메뉴(`file.collab`, `Command::OpenCollab`, `shell.rs:3838`)와 대화상자로만 할 수 있습니다.
- MCP 브리지(`apps/kreate/src/connector.rs`의 `match request.method`)에는 세션 관련 메서드가 없습니다.
  그래서 자동 시연이나 테스트를 할 때 사람이 직접 메뉴를 눌러야 합니다.

### 요청
1. **브리지 메서드 `collab`** (`connector.rs`)
   - `{"action": "host", "port": 7878, "key": "...", "name": "..."}` → `App::collab_host`
   - `{"action": "join", "address": "...", "key": "...", "name": "..."}` → `App::collab_join`
   - `{"action": "leave"}` → `App::collab_leave`
   - `{"action": "status"}` → `{ "state": "none|connecting|live|closed", "hosting": bool, "address": "...", "peers": [{name, site, role}] }`
2. **MCP 도구 `session`** (`apps/kreate-mcp/src/lib.rs`): 위 메서드를 그대로 노출하고 README의 도구 표에 추가합니다.
3. **세션 정보 파일**: 세션을 열면(host) 설정 폴더(`bridge.json`과 같은 위치, Windows는 `%APPDATA%\Kreate`)에
   `session.json`을 씁니다. 내용은 `{ "address": "127.0.0.1:7878", "lan_address": "...", "key": "...", "pid": 1234 }`이고,
   세션을 나가거나 프로그램이 끝나면 지웁니다.
   - 보안 모델은 `bridge.json`과 같습니다. 이 사용자만 읽을 수 있는 폴더에 둡니다.
   - KYNEX는 이 파일을 읽어 `kynex view --auto`로 주소와 키를 입력하지 않고 바로 붙습니다.

### 완료 기준
- MCP로 `session(action="host")`를 호출하면 메뉴로 연 것과 같은 상태가 되고, `status`에 참가자 목록이 나온다.
- 세션을 열면 `session.json`이 생기고, 닫으면 지워진다. 비정상 종료 후 남은 파일은 `pid`로 오래된 것인지 판별할 수 있다.

---

## K3. 원격 관절 변경이 로봇 리그에 덮어써지는 문제 (버그)

### 현재 동작
- `apps/kreate/src/main.rs`의 `run_drivers()`는 문서가 바뀔 때마다 `self.doc.sync_robot_rigs()`를 호출합니다.
  이 함수는 "리그의 뼈가 서 있는 자리로 관절을 놓는다"(`crates/kreate-scene/src/robot.rs`)는 방향으로만 동작합니다.
- 리그가 있는 로봇에서 **다른 참가자가 관절 오브젝트의 transform을 바꾼 패치**가 들어오면, 다음 프레임에
  `sync_robot_rigs()`가 관절을 뼈 위치로 되돌립니다. 그 되돌림은 다시 이 참가자의 편집으로 세션에 퍼집니다.
  결과적으로 두 참가자가 관절을 두고 서로 덮어쓰는 진동이 생깁니다.
- KYNEX의 P2 단계(뷰어에서 관절을 조작해 Kreate에 반영)는 바로 이 경로를 씁니다.
  Kreate 사용자 두 명이 공동 편집할 때도 같은 문제가 생깁니다.

### 요청
1. `kreate_collab::Session::tick`이 원격 패치로 바뀐 데이터블록을 알려 주게 합니다.
   예: `SessionEvent::Changed`에 `Changes`를 담거나, `Session::last_remote_changes()`를 추가합니다.
   필요하면 `Replica::incoming`의 반환값도 `bool`에서 `Option<Changes>`로 바꿉니다.
2. `collab_tick()`(`apps/kreate/src/collab.rs`)에서, 원격으로 바뀐 오브젝트 중 `RobotPart::Joint`인 것마다
   `doc.hold_joint_in_rig(joint, joint.position_of(&transform))`를 **`run_drivers()`보다 먼저** 호출합니다.
   이렇게 하면 뼈가 원격 관절 값을 따라가므로 `sync_robot_rigs()`가 관절을 되돌리지 않습니다.
3. 반대로 원격 패치가 **리그(armature) 포즈**를 바꾼 경우에는 지금처럼 `sync_robot_rigs()`가 관절을 맞춥니다.

### 완료 기준
- 테스트: 리그가 있는 2축 로봇에서 참가자 B가 관절 값을 0.7 rad로 바꾸는 패치를 보낸다. 참가자 A에서
  `tick`과 `run_drivers`를 10회 실행한 뒤에도 관절 값이 0.7이고, 뼈도 0.7 위치에 있다.
- 같은 상황에서 A가 되돌리는 패치를 보내지 않는다(`waiting() == 0`, 릴레이 `seq`가 늘지 않음).

---

## K4. 오브젝트 사용자 속성 (custom properties)

### 현재 동작
- `Object`(`crates/kreate-scene/src/document.rs`)에는 임의의 키-값을 붙일 필드가 없습니다.
  `ObjectRecord.extra`(`kreate-io/src/container.rs`)는 상위 버전 필드를 보존하는 용도라서, `Document`로는 올라오지 않습니다.
- URDF를 가져오면 `<robot name="...">`이 사라집니다. 로봇 이름이 루트 링크 이름(예: `base_link`)으로 바뀝니다.

### KYNEX가 필요로 하는 이유
OLP와 공장 레이아웃을 하려면 Kreate에서 만든 오브젝트에 KYNEX용 의미를 붙여야 합니다. 예를 들면
로봇 모델 식별자(`ur5e`), TCP 프레임, 작업 목표점(target), 스테이션·셀 소속, 사이클 포인트 같은 것입니다.
이 의미마다 Kreate 타입을 따로 추가하는 대신, 범용 속성 하나로 해결하려는 것입니다.

### 요청
1. `kreate-scene`에 다음을 추가합니다.
   ```rust
   pub enum PropValue { Bool(bool), Int(i64), Float(f64), Text(String), Vec3([f64; 3]) }
   pub struct Object { /* ... */ pub props: BTreeMap<String, PropValue> }
   ```
   `Object::new`의 기본값은 빈 맵입니다. `Document::changes_since`가 `props`의 차이도 변경으로 감지해야 합니다.
   그래야 세션 패치에 실립니다.
2. 컨테이너 형식(`container.rs`): `ObjectRecord`에 `props` 필드를 추가합니다(`skip_serializing_if = "BTreeMap::is_empty"`).
   이전 파일 읽기와, 이전 버전에서 저장한 파일의 왕복이 모두 깨지지 않아야 합니다(`migrate.rs`의 규칙을 따릅니다).
3. 오퍼레이터: `object.set_property(name, value)`, `object.remove_property(name)`. 둘 다 실행 취소가 가능해야 하고
   MCP `run_script`로 부를 수 있어야 합니다.
4. UI: 속성 패널에 "사용자 속성" 섹션을 추가합니다(보기, 편집, 추가, 삭제).
5. URDF 임포트(`kreate-io/src/urdf.rs`): 루트 링크에 `urdf.robot_name` 속성으로 `<robot name>`을 보존합니다.
   `write_urdf`는 이 값이 있으면 그 이름을 씁니다.

### 키 규칙 (KYNEX가 사용할 예정)
`kynex.` 접두어를 씁니다. 예: `kynex.robot_model = "ur5e"`, `kynex.kind = "target" | "tcp" | "station" | "conveyor"`,
`kynex.cell = "A-03"`. Kreate는 이 키들의 의미를 해석하지 않고 저장하고 전달하기만 합니다.

### 완료 기준
- 속성을 설정한 문서를 저장했다가 다시 열어도 그대로 남는다.
- 세션에서 다른 참가자에게 속성 변경이 전달된다.
- 실행 취소와 다시 실행이 된다.
- `props`가 없는 이전 `.kreate` 파일이 그대로 열린다.

---

## K5. UR 로봇 URDF 임포트 검증

### 배경
- UR 공식 설명 파일(`Universal_Robots_ROS2_Description`)은 xacro 형식입니다. **Kreate에 xacro를 구현하지 않습니다.**
  KYNEX가 미리 펼친 URDF를 메시와 함께 카탈로그로 제공합니다(UR3e·UR5e·UR10e·UR16e·UR20·UR30).
  시각 메시는 `.dae`, 충돌 메시는 `.stl`이고, `package://ur_description/...` 경로를 씁니다.
- Kreate에는 이미 URDF 읽기(`urdf.rs`), `package://` 경로 해석(`resolve`), DAE 읽기(`dae.rs`)가 있습니다.

### 요청
1. 펼친 UR5e URDF와 메시를 테스트 픽스처로 추가합니다. 라이선스를 확인하고, 문제가 되면 형상만 대체한 합성
   메시를 씁니다. 그런 다음 아래를 확인합니다.
   - `base_link`, `base_link_inertia`, `shoulder_link` … `wrist_3_link`, `flange`, `tool0`이 모두 오브젝트로 생기고,
     고정 관절(`base_link-base_link_inertia`, `wrist_3-flange`, `flange-tool0`)이 `JointKind::Fixed`로 들어온다.
   - 관절 한계 ±2π(revolute)와 effort·velocity 값이 보존된다.
   - DAE 재질 색이 반영되고, 충돌 메시가 `RobotPart::Collision`이면서 숨김 상태로 들어온다.
2. **순기구학 정확도 테스트:** 관절 값 `q = [0.3, -1.2, 1.5, -0.8, 1.1, 0.4]`에서 `tool0`의 월드 자세가 UR5e의 DH
   순기구학과 1e-6 m, 1e-6 rad 안에서 일치하는지 확인합니다.
   DH 파라미터: d1 = 0.1625, a2 = −0.425, a3 = −0.3922, d4 = 0.1333, d5 = 0.0997, d6 = 0.0996, α = [π/2, 0, 0, π/2, −π/2, 0].
3. 임포트할 때 루트에 `kynex.robot_model` 같은 속성을 자동으로 붙이지는 않습니다. 이 일은 KYNEX 카탈로그가 맡습니다.
   Kreate는 K4의 `urdf.robot_name`만 보존합니다.

### 완료 기준
위 두 테스트가 `cargo test -p kreate-io`에서 통과한다.

---

## K6. 공장 규모에서 협업 세션 성능

### 배경
KYNEX의 목표 장면 규모는 **공장 전체**입니다(오브젝트 1만~5만 개, 삼각형 수백만~수천만 개, 반복 부품 다수).
현재 구조에서 걱정되는 지점은 다음과 같습니다(코드 기준, 측정은 아직 하지 않았습니다).
- `replica.rs`: 패치를 보내고 받을 때마다 `self.base = doc.clone()`을 하고, 50 ms마다 `doc.changes_since(&self.base)`로
  문서 전체를 비교합니다.
- `relay.rs`: 모든 방이 하나의 `Mutex`를 씁니다. 패치마다 방 문서에 `apply`하고, 참가자가 들어올 때마다
  `encode_all`로 문서 전체를 zip으로 만듭니다.
- 라이브러리 링크(`Linkage::Linked`) 오브젝트를 컨테이너에 쓸 때 참조 `[library, name]`이 기록됩니다
  (`container.rs`). 라이브러리 파일이 없는 참가자(예: 다른 PC의 KYNEX)가 형상 데이터까지 받는지는 확인하지 못했습니다.

### KYNEX에서 측정한 결과 (2026-10-13, `kynex bench`, release 빌드, Kreate `151137a`)

합성 공장 장면입니다. 스테이션당 부품 25개, 부품당 삼각형 약 200개, 메시 공유율 80%입니다.

| 오브젝트 수 | `changes_since` (50 ms마다 호출) | 1개 이동 패치 (비교 + 인코딩 + 복제) | 참가 스냅샷 `encode_all` |
|---|---|---|---|
| 500 | 2.8 ms | 3.6 ms / 1 KB | 9 ms / 4 MB |
| 5,000 | 35 ms | 40 ms / 1 KB | 59 ms / 8 MB |
| 20,000 | **483 ms** | **530 ms** / 1 KB | 628 ms / 33 MB |
| 50,000 | **3,855 ms** | **3,902 ms** / 1 KB | **4,104 ms** / 83 MB |

**판정:** 오브젝트 수에 대해 제곱으로 늘어납니다. 2만 개짜리 장면으로 세션을 열면 Kreate의 매 프레임 `collab_tick`이
0.5초 이상 걸리고, 5만 개에서는 4초가 걸립니다. 즉 **공장 규모에서는 세션을 여는 순간 Kreate가 멈춥니다.**
그래서 K6을 필수 + 높은 우선순위로 올립니다.

**원인 (코드에서 확인):**
1. `crates/kreate-scene/src/document/sync.rs`의 `changes_since`가 삭제를 감지할 때
   `for uid in before.keys() { if !self.objects.values().any(|o| o.uid == *uid) ... }`로 처리합니다. 이것이 **O(n²)**입니다.
2. 같은 함수가 오브젝트를 `format!("{b:?}") == format!("{object:?}")`로 비교합니다. 매번 모든 오브젝트를
   Debug 문자열로 만들기 때문에 할당과 서식화 비용이 큽니다.
3. `Document::object_by_uid`와 `mesh_by_uid`가 선형 탐색입니다(`document.rs:875`, `888`).
   `apply_patch`(`sync.rs:355`)와 `patch()`(`sync.rs:213`, `218`)가 이것을 오브젝트마다 부르므로,
   스냅샷 생성·적용과 큰 패치가 O(n²)이 됩니다.

**바로 고칠 수 있는 것:**
- 삭제 감지: 현재 문서의 uid를 `HashSet`으로 한 번 만든 뒤 조회합니다.
- 오브젝트 비교: `Object`에 `PartialEq`를 구현합니다. 더 좋은 방법은 오브젝트별 수정 세대 번호(generation)를
  두는 것입니다. 편집은 `object_mut`을 거치므로 거기서 번호를 올리면 됩니다.
- uid 조회: `Document`에 uid → id 색인을 유지합니다. `object_by_uid` 주석에 "callers가 적어서 맵을 두지 않았다"고
  적혀 있는데, 이제 협업 세션이 그 호출자가 되었습니다.
- 이 세 가지를 고친 뒤 아래 벤치마크로 다시 측정합니다. KYNEX 쪽에서는 `kynex bench`로 같은 표를 바로 다시 만들 수 있습니다.

### 요청
1. **측정을 Kreate 안에도 둡니다.** `kreate-collab`에 벤치마크(criterion 또는 `#[ignore]` 테스트)를 추가합니다.
   - 장면: 오브젝트 1만 개와 5만 개, 공유 메시(같은 메시를 쓰는 복제본) 비율 80%, 삼각형 500만 개
   - 측정 항목: 유휴 상태 `tick` 비용, 오브젝트 1개를 드래그할 때의 패치 크기와 처리 시간, 새 참가자 접속 시
     스냅샷 크기와 생성 시간, 릴레이 메모리
2. 목표치(측정 결과를 보고 조정합니다).

   | 항목 | 목표 |
   |---|---|
   | 유휴 `tick` | < 1 ms |
   | 오브젝트 1개 이동 패치 | < 10 KB, 생성·적용 < 5 ms |
   | 5만 개 장면 참가 (LAN) | < 10 s |

3. 목표를 넘으면 다음 순서로 개선을 검토합니다.
   - 데이터블록별 세대 번호(generation)로 바뀐 것만 추적해서, 문서 전체 비교와 복제를 없앱니다.
   - `Document` 복제 비용을 줄입니다(오브젝트 저장소를 `Arc`로 공유하거나 copy-on-write).
   - 스냅샷 전송을 스트리밍하고 압축합니다. 같은 메시는 한 번만 보냅니다.
   - 릴레이의 잠금을 방 단위로 나눕니다.
4. 라이브러리 링크 오브젝트를 세션에 실을 때의 동작을 정하고 테스트합니다.
   권장안은 "세션 스냅샷과 패치에는 링크된 데이터블록의 실제 데이터를 포함한다"입니다.
   읽기 전용 표시(`Linkage`)는 유지합니다.

### 완료 기준
- 벤치마크 결과표가 PR에 첨부된다.
- 목표 미달 항목에 대해 개선 PR이 나오거나, 목표 조정 근거가 제시된다.

---

## Kreate에서 바꾸지 않는 것 (KYNEX가 직접 처리)

- 3D 웹 뷰어, 모디파이어 적용 결과 계산(`apply_stack`을 KYNEX에서 호출), IK, 경로 계획, 충돌 검사, 사이클타임,
  URScript 포스트프로세서. 모두 KYNEX 저장소에서 처리합니다.
- xacro 처리. KYNEX 카탈로그가 미리 펼친 URDF를 제공합니다.

## 검증 방법 (KYNEX 쪽)

Kreate 변경을 반영한 뒤 KYNEX 저장소에서 다음을 실행합니다.

```
cargo test --workspace                      # 연동 테스트 (실제 릴레이와 Kreate Session 사용)
cargo run -p kynex-link --example demo_session -- --key demo
cargo run -p kynex -- view 127.0.0.1:7878 --key demo --open
```

K1과 K3은 KYNEX의 `crates/kynex-link/tests/live.rs`에 대응 테스트를 추가해서 Kreate·KYNEX 양쪽에서 확인합니다.
