# VC 레이아웃 옮겨 오기 조사 (로드맵 6.1)

작성일: 2026-10-16

Visual Components(VC)로 만든 레이아웃(`.vcmx`)을 KYNEX로 옮기는 방법을 조사했습니다. 결론은 다음과 같습니다.

- `.vcmx` 파일을 직접 풀어 읽는 방식은 택하지 않습니다.
- 당장은 VC의 형상 내보내기(FBX·STEP)를 쓰고, KYNEX가 **배치 위치를 그대로** 가져옵니다. 이 기능은 이번에 만들었습니다.
- 다음 단계(6.2)는 VC 안에서 공식 Python API로 도는 내보내기 스크립트입니다. 장비 이름, 위치, 종류를 열린 형식으로 내보내고, KYNEX가 이를 읽어 셀 역할까지 붙입니다.

## 1. `.vcmx` 파일 구조 (공개 자료로 확인한 것)

| 항목 | 내용 | 출처 |
|---|---|---|
| 컨테이너 | ZIP 압축 폴더. 확장자를 .zip으로 바꾸면 열림 | VC 포럼 |
| 필수 파일 | `layout.rsc`(레이아웃을 만드는 데이터), `component.dat`(메타데이터), `layout_preview_icon.tga`, `layout_icon.tga` | VC 4.8 도움말 |
| 컴포넌트 | 기본 `.vcmx`는 컴포넌트 구조 전체를 **복사본**으로 담음. 파일에서 읽은 형상만 참조 | VC 포럼(XML 내보내기 샘플 글) |
| 컴포넌트 파일 | 컴포넌트마다 `component.dat`(VCID 포함), `component.rsc`, 아이콘 | VC 4.10 도움말 |
| `layout.rsc` 형식 | 공개 문서 없음. 텍스트로 편집 가능하다는 사용자 도구는 있으나 VC가 정의한 형식 | 확인 못 함 |
| VC의 입장 | "파일 형식은 독점(proprietary)이며, 데이터를 꺼내는 지원 방법은 VC 애플리케이션의 API뿐" | VC 직원 답변(포럼) |

## 2. 방법별 비교

| 방법 | 얻는 것 | 문제 | 판단 |
|---|---|---|---|
| A. `.vcmx` 직접 해석 | 배치, 컴포넌트 구조 | 형식이 문서화되지 않았고 버전마다 바뀔 수 있음. VC는 API만 지원한다고 밝힘. 라이선스(EULA)의 역공학 조항 확인이 필요함. 컴포넌트의 동작은 VC Python 스크립트라 어차피 옮길 수 없음 | **하지 않음** |
| B. VC 형상 내보내기 → KYNEX 가져오기 | 레이아웃 전체 또는 컴포넌트별 형상과 위치 | 셀 역할·신호·로봇 프로그램은 오지 않음. STEP은 테셀레이션(메시)만 나옴 | **지금 가능**(이번에 보완) |
| C. VC 안의 Python 스크립트(공식 API) → 열린 JSON + 형상 | 장비 이름, VCID, 위치 행렬, 속성, 연결, 컴포넌트별 형상 | VC가 설치된 PC에서 실행해야 함. 스크립트 시험에 VC 라이선스가 필요함 | **다음 단계(6.2)로 추천** |

VC의 형상 내보내기(4.8·5.0 도움말): FBX(형상·재질·텍스처·관절 구조), STEP AP242(테셀레이션만), OBJ, 3MF, 3DS, DWG/DXF, JT, VRML 등. glTF 내보내기는 확인하지 못했습니다. 레이아웃 전체를 한 파일로 내보내거나, 고른 컴포넌트만 내보낼 수 있습니다.

## 3. 이번에 만든 것 (방법 B)

- **FBX 가져오기**: VC가 주로 내보내는 FBX(바이너리·ASCII)를 KYNEX의 "모델 가져오기"로 읽습니다. FBX 안의 단위와 위쪽 축을 따릅니다. FBX 옆의 텍스처 파일은 읽지 않습니다.
- **원래 위치**: 라이브러리의 "모델 가져오기" 옆 **원래 위치**에 체크하면 모델이 원점이 아니라 파일 안의 위치에 놓입니다. 레이아웃 전체를 내보낸 파일이 배치 그대로 들어옵니다.

VC에서 하는 일:

1. 레이아웃을 열고 형상 내보내기에서 형식을 FBX(또는 STEP)로, 대상을 모든 컴포넌트로 고릅니다.
2. KYNEX에서 **원래 위치**에 체크하고 그 파일을 가져옵니다.
3. 가져온 장비에 셀 역할(CNC, 컨베이어, 그리퍼 등)을 붙이고, 로봇은 KYNEX 라이브러리의 같은 기종으로 바꿔 놓습니다.

한계: 레이아웃 전체가 장비 **하나**로 들어옵니다. 컴포넌트마다 따로 옮기려면 VC에서 컴포넌트별로 내보내야 합니다. 방법 C가 이를 자동으로 합니다.

## 4. 다음 단계 제안 (방법 C, 6.2)

VC 안에서 실행하는 Python 스크립트가 다음을 내보냅니다.

- `layout.json`: 컴포넌트마다 이름, VCID, 위치 행렬(N·O·A·P), 주요 속성(크기, 사이클 시간 등), 부모-자식 연결
- 컴포넌트마다 형상 파일(FBX 또는 OBJ)

KYNEX는 이 묶음(zip)을 읽어 다음을 합니다.

- 컴포넌트마다 장비 하나로, 제자리에 놓습니다.
- 로봇은 이름·VCID로 KYNEX 라이브러리의 같은 기종(UR, ABB, 두산, FANUC)에 맞춥니다.
- 이름과 VC 종류로 셀 역할을 추정합니다(예: Conveyor → 컨베이어, Machine/CNC → CNC 장비).

이 방법은 VC가 지원하는 API만 쓰므로 파일 형식 변화나 라이선스 문제를 피할 수 있습니다. 다만 스크립트는 VC가 설치된 PC에서 시험해야 하므로, **VC 라이선스가 있는 PC에서 한 번 확인할 수 있는지** 결정이 필요합니다.

## 출처

- [Layouts — VC 4.8 Professional 도움말](https://help.visualcomponents.com/4.8/Professional/en/English/Layout%20Configuration/Layouts.htm)
- [Components — VC 4.10 Professional 도움말](https://help.visualcomponents.com/4.10/Professional/en/English/Component%20Modeling/Components.htm)
- [VCMX File manipulator — VC 포럼](https://forum.visualcomponents.com/t/vcmx-file-manipulator/586)
- [XML Import/Export Tool — VC 포럼](https://forum.visualcomponents.com/t/xml-import-export-tool-save-layouts-as-pure-references-to-the-components/118)
- [Supported CAD Files — VC 5.0 도움말](https://help.visualcomponents.com/5.0/Premium/en/English/Import/Supported_CAD_files.htm)
- [Exporting Geometry of Layouts — VC 4.8 도움말](https://help.visualcomponents.com/4.8/Premium/en/English/Export/Exporting_geometry_of_layouts.htm)
- [FBX Exporter — VC 4.8 도움말](https://help.visualcomponents.com/4.8/Premium/en/English/Export/FBX_Exporter.htm)
- [Export geometry to STEP AP214 — VC 포럼](https://forum.visualcomponents.com/t/export-geometry-to-step-ap214/2234)
