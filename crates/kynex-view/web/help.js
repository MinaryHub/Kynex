// A ? beside each setting — in the inspector, the equipment's sizes, a
// role's values, the PLC, the site's drawing, the flow's settings — that
// opens a short explanation of it: what it is, what it changes, a value to
// start from. Fields are found as they are drawn (the inspector is drawn
// anew often), and each is explained by what it sets — `data-prop`,
// `data-param`, its id —, by its role where the same name means something
// else for another, or else by its own title.

import { tFor, getLang } from './i18n.js';

const t = tFor('panels');

// By role and setting first (`station.speed`), then by setting alone.
const HELP = {
  // ---- placing --------------------------------------------------------------
  'at.0': ['X 위치', '장비 기준점의 X 좌표입니다. 그리드의 빨간 선 방향이 +X입니다.', '바닥에 놓을 때는 끌어서 놓고, 정확한 값이 필요할 때 숫자로 넣으세요.'],
  'at.1': ['Y 위치', '장비 기준점의 Y 좌표입니다. 그리드의 초록 선 방향이 +Y입니다.'],
  'at.2': ['Z 높이', '장비 바닥이 놓인 높이입니다. 0이면 바닥 위입니다.', '숫자로 넣은 높이는 그대로 지켜집니다. 끌어서 놓으면 바닥이나 받침대 위로 내려앉습니다.'],
  yaw: ['회전', '위에서 볼 때 장비가 돌아간 각도(Z축 기준)입니다. 반시계 방향이 +입니다.', '15° 단위로 바뀝니다. 90°씩 돌리면 줄 맞추기 쉽습니다.'],
  'tilt.roll': ['X축 기울기', '장비를 X축으로 기울입니다.', '벽에 단 로봇은 90°, 천장에 거꾸로 단 로봇은 180°입니다.'],
  'tilt.pitch': ['Y축 기울기', '장비를 Y축으로 기울입니다.'],
  price: ['투자비', '장비 값과 설치비(만원)입니다.', '통계·시나리오의 투자비, 감가상각, 개당 원가, 회수 기간 계산에 쓰입니다.'],
  align: ['정렬', '여러 장비를 골랐을 때 한쪽 끝이나 가운데로 줄을 맞춥니다.'],

  // ---- equipment sizes (library parameters) ---------------------------------
  'param.tool': ['툴', '로봇 손목에 다는 툴(그리퍼)입니다.', '바꾸면 툴의 길이만큼 TCP가 달라집니다. 프로그램의 지점은 다시 확인하세요.'],
  'param.length': ['길이', '장비의 긴 쪽 길이(mm)입니다.', '컨베이어·레일·주행 트랙은 길이만큼 이송 시간이 늘어납니다.'],
  'param.width': ['폭', '장비의 폭(mm)입니다.'],
  'param.depth': ['깊이', '장비의 앞뒤 깊이(mm)입니다.'],
  'param.height': ['높이', '장비의 높이(mm)입니다.', '컨베이어는 벨트 윗면, 받침대·테이블은 윗면 높이입니다. 로봇이 닿는지 높이를 맞춰 보세요.'],
  'param.size': ['크기', '장비 단면이나 테이블의 한 변(mm)입니다.'],
  'param.diameter': ['지름', '원통형 장비(탱크·탑)의 지름(mm)입니다.'],
  'param.spacing': ['간격', '반복되는 부분(기둥·칸·랙) 사이 간격(mm)입니다.'],
  'param.span': ['스팬', '크레인·구조물의 양쪽 다리 사이 거리(mm)입니다.'],
  'param.stroke': ['행정', '움직이는 부분이 갈 수 있는 거리(mm)입니다.'],
  'param.opening': ['문 폭', '출입문이 열리는 폭(mm)입니다.'],
  'param.line': ['선 폭', '바닥에 그리는 구역 테두리 선의 폭(mm)입니다.'],
  'param.range': ['빔 길이', '센서 빛이 닿는 거리(mm)입니다. 이 안에 부품이 오면 감지합니다.'],
  'param.plate_w': ['판 폭', '판의 폭(mm)입니다.'],
  'param.plate_d': ['판 깊이', '판의 깊이(mm)입니다.'],
  'param.fork_height': ['포크 높이', '지게차 포크가 올라간 높이(mm)입니다.'],
  'param.count': ['개수', '같은 부분이 몇 개 반복되는지입니다.'],
  'param.bays': ['칸 수', '랙·창고의 가로 칸 수입니다.'],
  'param.levels': ['단 수', '랙·창고의 세로 단 수입니다.'],
  'param.layers': ['단 수', '쌓는 층 수입니다.'],
  'param.chambers': ['챔버 수', '장비 안의 처리 챔버 수입니다.'],
  'param.angle': ['회전 각도', '움직이는 부분이 도는 각도입니다.'],
  'param.time': ['동작 시간', '한 번 움직이는 데 걸리는 시간(초)입니다.'],
  'param.cycle': ['처리 시간', '부품 하나를 처리하는 시간(초)입니다.', '라인에서 가장 긴 처리 시간이 생산량을 정합니다(병목).'],
  'param.feed': ['공급 간격', '소재가 하나씩 들어오는 간격(초)입니다.'],
  'param.capacity': ['용량', '한 번에 담을 수 있는 부품 수입니다.'],
  'param.takes': ['투입 수', '부품 몇 개가 모여야 한 번 처리하는지입니다.', '2 이상이면 조립(여러 개 → 하나)입니다. 예: 병 24개 → 박스 1개.'],
  'param.makes': ['배출 수', '한 번 처리하면 몇 개가 나오는지입니다.', '2 이상이면 분해(하나 → 여러 개)입니다.'],
  'param.speed': ['속도', '움직이는 속도입니다.'],

  // ---- roles: machines ------------------------------------------------------
  'cnc.cycle': ['가공 시간', '부품 하나를 가공하는 시간(초)입니다. 문이 닫히고 척이 물린 뒤부터 셉니다.', '실제 사이클 타임을 넣으세요. 로봇 대기 시간과 가동률이 여기에 따라 달라집니다.'],
  'cnc.door': ['문 개폐 시간', '자동문이 열리거나 닫히는 데 걸리는 시간(초)입니다.', '보통 2~4초입니다.'],
  'cnc.chuck': ['척 개폐 시간', '척이 부품을 물거나 푸는 데 걸리는 시간(초)입니다.', '보통 0.5~1.5초입니다.'],
  'molder.cycle': ['성형 시간', '한 번 사출 성형하는 시간(초)입니다.'],
  'molder.door': ['가드 개폐 시간', '안전 가드가 열리거나 닫히는 시간(초)입니다.'],
  'infeed.rows': ['행', '팔레트에 부품을 놓는 자리의 행 수입니다.'],
  'infeed.cols': ['열', '팔레트에 부품을 놓는 자리의 열 수입니다.'],
  'infeed.pitch': ['간격', '팔레트 자리 사이 간격(mm)입니다. 부품보다 조금 크게 잡으세요.'],
  'outfeed.rows': ['행', '배출 팔레트 자리의 행 수입니다.'],
  'outfeed.cols': ['열', '배출 팔레트 자리의 열 수입니다.'],
  'outfeed.pitch': ['간격', '배출 팔레트 자리 사이 간격(mm)입니다.'],
  'gripper.actuate': ['개폐 시간', '그리퍼가 열리거나 닫히는 시간(초)입니다.', '공압 그리퍼는 보통 0.2~0.5초입니다.'],
  'conveyor.speed': ['벨트 속도', '컨베이어 벨트가 부품을 나르는 속도(mm/s)입니다.', '길이 ÷ 속도 = 한 끝에서 다른 끝까지 걸리는 시간입니다.'],
  'conveyor.feed': ['소재 공급 간격', '컨베이어 입구에 소재가 하나씩 나타나는 간격(초)입니다. 0이면 저절로 나오지 않습니다.'],
  'conveyor.drain': ['끝에서 치움', '1이면 끝에 닿은 부품을 치웁니다(밖으로 내보냄). 0이면 끝에서 기다립니다.'],
  'amr.speed': ['주행 속도', 'AMR이 달리는 속도(mm/s)입니다.', '공장 안 AMR은 보통 1000~1500 mm/s입니다.'],
  'amr.capacity': ['적재량', 'AMR이 한 번에 나르는 부품 수입니다.'],
  'amr.battery': ['배터리 주행 시간', '완충으로 달릴 수 있는 시간(분)입니다. 다 쓰면 충전소로 갑니다. 0이면 배터리를 따지지 않습니다.'],
  'amr.charge': ['충전 시간', '빈 배터리를 완충하는 데 걸리는 시간(분)입니다.'],
  'amr_stop.charger': ['충전소', '이 정지 위치에서 AMR이 충전할 수 있는지 정합니다.'],
  'positioner.speed': ['회전 속도', '포지셔너가 도는 속도(°/s)입니다.'],
  'door.swing': ['동작', '미닫이는 직선으로 밀려 열리고, 여닫이는 경첩을 축으로 돌아 열립니다.'],
  'door.axis': ['축', '문이 움직이는 방향(미닫이) 또는 도는 축(여닫이)입니다. 부품 자신의 좌표축 기준입니다.'],
  'door.travel': ['거리', '미닫이 문이 열릴 때 움직이는 거리(mm)입니다. 음수면 반대쪽입니다.'],
  'door.angle': ['각도', '여닫이 문이 열리는 각도입니다. 음수면 반대로 돕니다.'],
  'sensor.axis': ['빔 방향', '포토센서 빛이 나가는 방향입니다. 부품 자신의 좌표축 기준입니다.'],
  'sensor.travel': ['빔 길이', '빛이 닿는 거리(mm)입니다. 이 안을 부품이 가리면 감지합니다.'],
  'actuator.swing': ['동작', '직선은 실린더·스토퍼·리프터처럼 밀고, 회전은 돌립니다.'],
  'actuator.axis': ['축', '움직이는 방향 또는 도는 축입니다. 부품 자신의 좌표축 기준입니다.'],
  'actuator.travel': ['행정', '직선으로 움직이는 거리(mm)입니다. 음수면 반대 방향입니다.'],
  'actuator.angle': ['각도', '회전하는 각도입니다.'],
  'actuator.door': ['동작 시간', '전진 끝에서 후진 끝까지(또는 반대로) 걸리는 시간(초)입니다.'],
  'connector.plug': ['종류', '이 점이 무엇과 이어지는지입니다.', '로봇 받침면: 로봇이 올라서는 자리 · 컨베이어 입구/출구: 컨베이어끼리 이어 붙는 끝 · 받침면에 올라섬: 장비 바닥.'],

  // ---- roles: the process flow ----------------------------------------------
  rule: ['다음 공정 고르기', '다음 공정이 여러 곳일 때 어디로 보낼지입니다.', '번갈아: 차례대로 · 대기 적은 곳: 덜 밀린 곳으로(병렬 설비에 좋음) · 무작위.'],
  'source.feed': ['공급 간격', '소재가 하나씩 들어오는 간격(초)입니다.', '3600 ÷ 공급 간격 = 시간당 투입 수입니다. 병목 처리 시간보다 짧으면 앞에 쌓입니다.'],
  kanban: ['칸반', '공정 안에 있을 수 있는 최대 부품 수입니다. 이만큼 차면 공급을 멈춥니다. 0이면 제한이 없습니다.', '재공품(WIP)을 줄이고 리드 타임을 볼 때 씁니다.'],
  dist: ['시간 분포', '시간이 매번 같은지, 흔들리는지입니다.', '고정: 늘 같음 · 균등 ±폭: 그 안에서 고르게 · 삼각 ±폭: 가운데가 많음 · 정규 σ: 종 모양 · 지수: 평균만 같고 들쭉날쭉(도착 간격에 흔함).'],
  spread: ['폭·σ', '시간 분포의 흔들림 크기(초)입니다. 균등·삼각이면 ±폭, 정규면 표준편차입니다.'],
  'source.capacity': ['내보낼 자리', '공급에서 나가기를 기다리는 부품을 몇 개까지 둘 수 있는지입니다.'],
  'source.speed': ['이송 속도', '다음 공정으로 직접 옮길 때의 속도(mm/s)입니다. 거리 ÷ 속도만큼 걸립니다.'],
  'station.cycle': ['처리 시간', '부품 하나(조립이면 한 묶음)를 처리하는 시간(초)입니다.', '라인에서 가장 긴 처리 시간 ÷ 대수가 생산량을 정합니다(병목은 빨간 원으로 표시).'],
  setup: ['품종 교체 시간', '앞 부품과 다른 제품이 오면 바꾸는 데 드는 시간(초)입니다.'],
  mtbf: ['고장 간격 평균 (MTBF)', '평균 몇 초 일하면 한 번 고장 나는지입니다. 0이면 고장 나지 않습니다.', '예: 8시간에 한 번이면 28800.'],
  pm_every: ['보전 간격 (계획 보전)', '몇 초 가동할 때마다 계획 보전(유지보수)으로 멈추는지입니다. 0이면 하지 않습니다.', '하던 사이클을 마친 뒤 멈춥니다. 계획 정지라서 OEE 가동률의 계획 시간에서 뺍니다. 예: 8시간 가동마다면 28800.'],
  pm_time: ['보전 시간', '계획 보전 한 번에 멈춰 있는 시간(초)입니다.'],
  mttr: ['수리 시간 평균 (MTTR)', '고장 나면 평균 몇 초 동안 서 있는지입니다.'],
  scrap: ['불량률', '처리한 것 중 불량으로 버려지는 비율(%)입니다. 불량은 다음 공정으로 가지 않습니다.'],
  crew: ['작업자', '작업자 필요로 하면 작업자(사람 역할)가 와야 처리를 시작합니다.'],
  holds: ['동시 투입 가능 수', '이 장비 안에 한 번에 들어가 있을 수 있는 부품 수입니다. 기다리는 것, 처리 중인 것, 다 되어 꺼내기를 기다리는 것을 모두 셉니다. 차면 더 넣지 않습니다.', '개수는 공정 흐름의 부품 하나를 1개로 셉니다. 부품 모양이 유리 카세트·FOUP·팔레트처럼 여러 장을 담은 것이면 그 한 묶음이 1개입니다. 지금 몇 개가 들어 있는지는 아래 상태 줄의 「장비 안 n/한도」로 보입니다. 0이면 자동입니다. 척·금형과 문이 있는 기계(CNC·사출기)는 1개, 그 밖의 장비는 대기 자리만큼 받습니다. 투입 수보다 작게 정하면 투입 수만큼은 받습니다.'],
  'station.capacity': ['대기 자리', '처리를 기다리며 앞에 쌓일 수 있는 부품 수입니다. 차면 앞 공정이 막힙니다.', 'CNC·사출기처럼 척·금형과 문이 있는 기계는 이 값 대신 동시 투입 가능 수를 따릅니다.'],
  takes: ['투입 수', '부품 몇 개가 모여야 한 번 처리하는지입니다.', '2 이상이면 조립(여러 개 → 하나)입니다. 예: 병 24개 → 박스 1개.'],
  makes: ['배출 수', '한 번 처리하면 몇 개가 나오는지입니다.', '2 이상이면 분해(하나 → 여러 개)입니다.'],
  'station.speed': ['이송 속도', '다음 공정으로 직접 옮길 때의 속도(mm/s)입니다.'],
  'buffer.capacity': ['용량', '버퍼에 쌓을 수 있는 최대 부품 수입니다. 차면 앞 공정이 막힙니다.', '병목 앞에 두면 앞 공정이 멈춰도 병목이 쉬지 않습니다.'],
  policy: ['꺼내는 순서', '먼저 들어온 것부터(FIFO) 또는 나중 들어온 것부터(LIFO) 꺼냅니다.'],
  'buffer.speed': ['이송 속도', '다음 공정으로 직접 옮길 때의 속도(mm/s)입니다.'],
  'operator.speed': ['걷는 속도', '작업자가 걷는 속도(m/s)입니다. 보통 1.0~1.4입니다.'],
  'operator.capacity': ['한 번에 드는 수', '작업자가 한 번에 나르는 부품 수입니다.'],
  wage: ['임금', '작업자 한 명의 시간당 임금(원)입니다. 개당 원가 계산에 쓰입니다.'],
  shift_from: ['근무 시작', '흐름을 시작한 때를 0시로 하여 근무가 시작되는 시각입니다.'],
  shift_to: ['근무 끝', '근무가 끝나는 시각입니다. 시작과 같으면 하루 종일 일합니다.'],

  // ---- the PLC and real machines -------------------------------------------
  '#plc-protocol': ['방식', 'PLC와 이야기하는 통신 방식입니다.', 'Modbus TCP: 가장 흔함 · OPC UA: 최신 PLC·서버 · Siemens S7 · LS ELECTRIC XGT.'],
  '#plc-host': ['주소', 'PLC의 IP 주소입니다. 같은 PC에서 시뮬레이터를 쓰면 127.0.0.1.'],
  '#plc-port': ['포트', '통신 포트입니다. Modbus 502, OPC UA 4840, S7 102, XGT 2004가 기본입니다.'],
  '#plc-unit': ['유닛', 'Modbus 장치 번호(Unit ID)입니다. 보통 1입니다.'],
  '#plc-rack': ['랙·슬롯', 'S7 CPU가 꽂힌 랙과 슬롯 번호입니다.', 'S7-1200/1500: 랙 0, 슬롯 1 · S7-300: 랙 0, 슬롯 2.'],
  '#plc-path': ['경로', 'OPC UA 서버 주소의 경로 부분입니다. 없으면 비워 두세요.'],
  '#plc-security': ['보안', 'OPC UA 통신 보안 방식입니다. 서버 설정과 같아야 합니다.'],
  '#plc-user': ['사용자', 'OPC UA 로그인 사용자입니다. 비우면 익명으로 접속합니다.'],
  '#plc-password': ['비밀번호', 'OPC UA 로그인 비밀번호입니다.'],
  io: ['I/O 번호', '로봇 컨트롤러의 디지털 입출력 번호입니다. 0–7 표준, 8–15 설정 가능, 16–17 툴입니다.', '→는 KYNEX가 내보내는 신호, ←는 받아 기다리는 신호입니다.'],
  plc: ['PLC 주소', '이 신호를 읽고 쓸 PLC 주소입니다. 예: Modbus 코일 00001, S7 DB1.DBX0.0.'],
  '#mirror-kind': ['방식', '실제 장비의 상태를 받아 오는 방식입니다.', 'MTConnect: 공작기계 · umati: OPC UA 공작기계 · MQTT(VDA 5050): AMR.'],
  '#mirror-agent': ['에이전트', 'MTConnect 에이전트 주소입니다. 예: http://10.0.0.5:5000'],
  '#mirror-device': ['장비', '에이전트 안의 장비 이름입니다. 비우면 첫 장비를 씁니다.'],
  '#mirror-url': ['서버', 'umati OPC UA 서버 주소입니다. 예: opc.tcp://10.0.0.5:4840'],
  '#mirror-node': ['장비 노드', '서버 안에서 장비를 가리키는 노드 ID입니다. 예: ns=3;s=Machine1'],
  '#mirror-broker': ['브로커', 'MQTT 브로커 주소입니다. 예: 10.0.0.5:1883'],
  '#mirror-topic': ['토픽', 'AMR이 상태를 보내는 MQTT 토픽입니다. 예: uagv/v2/제조사/일련번호/state'],
  '#cell-kind': ['셀 역할', '이 장비가 셀에서 무엇인지 정합니다(CNC, 팔레트, 공정 스테이션 등).', '역할을 주면 시뮬레이션에서 그 역할대로 움직이고, 그 역할의 설정이 아래에 나옵니다.'],

  // ---- the site's drawing and the flow's settings ---------------------------
  '#floor-width': ['실제 가로', '도면 이미지의 가로가 실제로 몇 m인지입니다. 이 값으로 도면 크기가 맞춰집니다.', '도면의 치수선 하나를 재서 넣으면 정확합니다.'],
  '#floor-x': ['가운데 X', '도면 가운데가 놓일 X 위치(m)입니다.'],
  '#floor-y': ['가운데 Y', '도면 가운데가 놓일 Y 위치(m)입니다.'],
  '#floor-yaw': ['회전', '도면을 돌리는 각도입니다.'],
  '#floor-opacity': ['진하기', '도면이 얼마나 진하게 보이는지(%)입니다. 낮추면 장비가 잘 보입니다.'],
  '#flow-work': ['근무·휴식', '근무와 휴식을 번갈아 둡니다(분). 휴식 중에는 흐름이 멈추고 OEE 계획 시간에서 빠집니다. 0이면 쉬지 않습니다.'],
  '#flow-warmup': ['워밍업', '처음 이만큼(분)은 통계에서 뺍니다. 라인이 비어 있다가 차는 동안을 빼고 정상 상태만 봅니다.'],
  '#cost-hours': ['연간 가동', '1년에 라인을 돌리는 시간입니다. 개당 원가와 회수 기간 계산의 기준입니다.'],
};

// What `label` sets, as HELP knows it, or null.
function keysOf(label) {
  const c = label.querySelector('input, select, textarea');
  const keys = [];
  if (!c) return keys;
  const role = document.getElementById('cell-kind')?.value;
  const d = c.dataset;
  if (d.prop) {
    if (role) keys.push(`${role}.${d.prop}`);
    keys.push(d.prop);
  }
  if (d.param) keys.push(`param.${d.param}`);
  if (d.at != null) keys.push(`at.${d.at}`);
  if (d.yaw != null) keys.push('yaw');
  if (d.tilt) keys.push(`tilt.${d.tilt}`);
  if (d.price != null) keys.push('price');
  if (d.align != null) keys.push('align');
  if (d.io) keys.push('io');
  if (d.plc) keys.push('plc');
  if (c.id) keys.push(`#${c.id}`);
  return keys;
}

// The words of `label` without what is in its fields: its name.
function nameOf(label) {
  const span = label.querySelector(':scope > .insp-label');
  if (span) return span.textContent.trim();
  let text = '';
  for (const n of label.childNodes) if (n.nodeType === Node.TEXT_NODE) text += n.textContent;
  return text.replace(/\s+/g, ' ').trim();
}

function explain(label) {
  // Shown in the language chosen; looked up by what it sets.
  for (const k of keysOf(label)) if (HELP[k]) return HELP[k].map((x) => t(x));
  const title = label.getAttribute('title') || label.querySelector('[title]')?.getAttribute('title');
  if (title) return [nameOf(label), title];
  return null;
}

// ---- the ? and its card ---------------------------------------------------

let card = null;

function closeCard() {
  card?.remove();
  card = null;
}

function openCard(button, label) {
  const was = card?.dataset.for === label.dataset.helpId;
  closeCard();
  if (was) return;
  const said = explain(label);
  const [title, text, tip] = said ?? [nameOf(label), t('이 값에 대한 설명이 아직 없습니다.')];
  card = document.createElement('div');
  card.className = 'help-card';
  card.setAttribute('role', 'dialog');
  card.dataset.for = label.dataset.helpId;
  const head = document.createElement('div');
  head.className = 'help-title';
  head.textContent = title || nameOf(label);
  const body = document.createElement('p');
  body.className = 'help-text';
  body.textContent = text;
  card.append(head, body);
  if (tip) {
    const more = document.createElement('p');
    more.className = 'help-tip';
    more.textContent = tip;
    card.append(more);
  }
  document.body.appendChild(card);
  // Below the ?, kept inside the window.
  const r = button.getBoundingClientRect();
  const w = card.offsetWidth;
  const h = card.offsetHeight;
  let left = Math.min(r.left - 12, window.innerWidth - w - 8);
  let top = r.bottom + 6;
  if (top + h > window.innerHeight - 8) top = Math.max(8, r.top - h - 6);
  card.style.left = `${Math.max(8, left)}px`;
  card.style.top = `${top}px`;
}

let ids = 0;
function decorate(label) {
  if (label.querySelector(':scope > .help-q')) return;
  if (!label.querySelector('input, select, textarea')) return;
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'help-q';
  button.textContent = '?';
  button.setAttribute('aria-label', t('설명'));
  button.tabIndex = -1;
  label.dataset.helpId ||= `h${++ids}`;
  // The name in a span of its own, so the row can be laid out as name, ?,
  // field, unit; a long one takes the whole row (see app.css).
  const name = nameOf(label);
  if (name && !label.querySelector(':scope > .insp-label')) {
    const span = document.createElement('span');
    span.className = 'insp-label';
    span.textContent = name;
    span.title = name;
    for (const n of [...label.childNodes]) if (n.nodeType === Node.TEXT_NODE) n.remove();
    label.prepend(span);
  }
  // English names are longer than Korean ones of the same weight.
  label.classList.toggle('help-long', name.length > (getLang() === 'en' ? 8 : 2));
  // Right after the name, before its fields.
  const field = [...label.children].find((n) => n.matches('input, select, textarea'));
  label.insertBefore(button, field ?? null);
}

const WANTED = 'label.insp-field, #floor-panel label, label.muted[title]';

function decorateAll(root = document) {
  for (const label of root.querySelectorAll(WANTED)) decorate(label);
}

export function mountHelp() {
  decorateAll();
  let queued = false;
  new MutationObserver(() => {
    if (queued) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      decorateAll();
    });
  }).observe(document.body, { childList: true, subtree: true });
  // A ? pressed: its card, and nothing else (not the field it is in).
  document.addEventListener(
    'click',
    (e) => {
      const button = e.target.closest?.('.help-q');
      if (!button) return;
      e.preventDefault();
      e.stopPropagation();
      openCard(button, button.closest('label'));
    },
    true,
  );
  document.addEventListener(
    'pointerdown',
    (e) => {
      if (card && !card.contains(e.target) && !e.target.closest?.('.help-q')) closeCard();
    },
    true,
  );
  window.addEventListener(
    'keydown',
    (e) => {
      if (e.key === 'Escape' && card) {
        closeCard();
        e.preventDefault();
        e.stopImmediatePropagation();
      }
    },
    true,
  );
  window.addEventListener('resize', closeCard);
}
