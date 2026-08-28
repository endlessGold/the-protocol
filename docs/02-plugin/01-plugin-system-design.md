# 플러그인 시스템 전체 설계

> The Protocol 플러그인 시스템 아키텍처 및 생명주기 관리

## 1. 개요

The Protocol의 플러그인 시스템은 WASM 기반 격리 실행 환경에서 서버 기능을 확장하는 구조입니다. 모든 플러그인은 공통된 생명주기를 따르며, 런타임에 의해 관리됩니다.

## 2. 전체 아키텍처

```
┌─────────────────────────────────────────────────────┐
│                   Plugin Manager                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐  │
│  │ Discovery │  │ Registry │  │ Dependency Graph  │  │
│  └──────────┘  └──────────┘  └──────────────────┘  │
├─────────────────────────────────────────────────────┤
│                WASM Runtime Layer                    │
│  ┌─────────────────────────────────────────────┐   │
│  │              Wasmtime Engine                 │   │
│  │  ┌────────┐ ┌────────┐ ┌────────┐          │   │
│  │  │Store A │ │Store B │ │Store C │ ...       │   │
│  │  │Plugin 1│ │Plugin 2│ │Plugin 3│          │   │
│  │  └────────┘ └────────┘ └────────┘          │   │
│  └─────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────┤
│              Host Function Interface                 │
│  ┌──────┐ ┌─────────┐ ┌───────┐ ┌──────────────┐  │
│  │Log   │ │Storage  │ │Event  │ │Player/Combat  │  │
│  └──────┘ └─────────┘ └───────┘ └──────────────┘  │
└─────────────────────────────────────────────────────┘
```

## 3. 플러그인 생명주기

```
Discover → Validate → Resolve Dependencies → Load → Initialize → Enable
                                                                   │
                                                           Disable ←┘
                                                             │
                                                           Unload
```

### 3.1 Discover (발견)

- `plugins/` 디렉토리를 순회하며 `plugin.toml` 파일 탐색
- 디렉토리 구조: `plugins/{plugin_name}/plugin.toml`
- 존재하지 않는 디렉토리 또는 잘못된 TOML은 로그 후 건너뜀
- 현재 구현: `DefaultPluginRuntime::discover()` (`core/plugin/src/lib.rs:93`)

### 3.2 Validate (검증)

- 매니페스트 필수 필드 존재 여부 확인
- `api_version` 호환성 검사 (MAJOR 버전 일치 필요)
- `permissions`에 선언된 권한이 지원되는지 확인
- `dependencies`가 유효한 플러그인 이름인지 확인
- 파일 이름 충돌 검사

**검증 규칙:**

| 검증 항목 | 규칙 | 실패 시 |
|-----------|------|---------|
| name | 비어있지 않은 소문자+하이픈 | Error |
| version | semver 형식 | Error |
| api_version | MAJOR 버전 일치 | IncompatibleApiVersion |
| permissions | 지원되는 권한만 선언 | PermissionDenied |
| dependencies | 존재하는 플러그인만 참조 | NotFound |

### 3.3 Resolve Dependencies (의존성 해석)

- 의존성 그래프 구축 (DAG - Directed Acyclic Graph)
- 위상 정렬(Topological Sort)로 로딩 순서 결정
- 순환 의존성 감지: DFS 기반 DFS 기반 cycle detection
- 의존성 없는 플러그인부터 순차 로딩

**의존성 그래프 해석 알고리즘:**

```
1. 모든 플러그인의 dependencies를 기반으로 그래프 생성
2. 각 노드의 진입 차수(in-degree) 계산
3. 진입 차수 0인 노드를 큐에 추가
4. 큐에서 노드를 꺼내 처리 후, 관련 엣지 제거
5. 엣지 제거 후 진입 차수가 0이 된 노드를 큐에 추가
6. 큐가 빌 때까지 반복
7. 처리된 노드 수 ≠ 전체 노드 수 → 순환 의존성 존재
```

### 3.4 Load (로딩)

- WASM 모듈 파일(`plugin.wasm`) 로드
- Wasmtime Engine에 컴파일
- Store 생성 및 WASI 컨텍스트 설정
- Host Function 바인딩
- Linear Memory 할당 (기본 16MB, 최대 256MB)

### 3.5 Initialize (초기화)

- `plugin_init` export 함수 호출
- 플러그인 자체 초기화 수행
- 초기화 실패 시 해당 플러그인 비활성화
- 의존성 플러그인의 초기화 완료 보장

### 3.6 Enable (활성화)

- `plugin_enable` export 함수 호출
- 명령어 핸들러 등록
- 이벤트 핸들러 등록
- 타이머 콜백 등록
- 플러그인을 활성 상태로 전환

### 3.7 Disable (비활성화)

- `plugin_disable` export 함수 호출
- 등록된 핸들러 제거
- 실행 중인 타이머 취소
- 플러그인을 비활성 상태로 전환
- 의존성 플러그인에 영향 없는 독립 비활성화

### 3.8 Unload (언로드)

- `plugin_unload` export 함수 호출
- WASM Store 해제
- Linear Memory 해제
- 플러그인 데이터 정리
- 레지스트리에서 제거

## 4. 플러그인 매니페스트 (plugin.toml) 명세

```toml
# 필수: 플러그인 고유 식별자
name = "combat-system"

# 필수: semver 형식 버전
version = "1.0.0"

# 필수: 플러그인 설명
description = "전투 시스템 플러그인"

# 필수: 호환 가능한 API 버전 (semver)
api_version = "1.0"

# 선택: 플러그인 저자
authors = ["Developer Name <email@example.com>"]

# 선택: 라이선스
license = "MIT"

# 선택: 플러그인 홈페이지
homepage = "https://example.com"

[permissions]
# 필수: 필요한 권한 목록
required = [
    "player.read",
    "player.write",
    "combat.start",
    "combat.action",
    "inventory.read",
    "inventory.write",
    "storage.read",
    "storage.write",
    "event.emit",
    "event.subscribe",
]

# 선택: 선택적 권한 목록
optional = [
    "admin.config",
    "logging.debug",
]

[resources]
# 선택: 최대 메모리 (기본 16MB)
memory_limit = "32MB"

# 선택: 실행당 최대 Fuel (기본 1,000,000)
execution_limit = 2_000_000

# 선택: 최대 동시 타이머 수
max_timers = 10

[dependencies]
# 선택: 의존성 플러그인 (이름 = 버전 요구사항)
"inventory-system" = ">=1.0.0"
"event-bus" = "^2.0"

[metadata]
# 선택: 커스텀 메타데이터
category = "gameplay"
tags = ["combat", "pvp", "pve"]
min_server_version = "0.5.0"
```

### 4.1 필수 필드

| 필드 | 타입 | 설명 |
|------|------|------|
| `name` | String | 플러그인 고유 이름 (소문자, 하이픈 허용) |
| `version` | String | semver 형식 버전 |
| `description` | String | 플러그인 설명 |
| `api_version` | String | 호환 API 버전 |

### 4.2 선택 필드

| 필드 | 타입 | 기본값 | 설명 |
|------|------|--------|------|
| `authors` | Vec<String> | `[]` | 저자 목록 |
| `license` | String | `""` | 라이선스 |
| `homepage` | String | `""` | 홈페이지 |
| `permissions` | Table | `{}` | 권한 선언 |
| `resources` | Table | `{}` | 리소스 제한 |
| `dependencies` | Table | `{}` | 의존성 |
| `metadata` | Table | `{}` | 커스텀 메타데이터 |

## 5. 의존성 그래프 해석

### 5.1 그래프 구조

```
combat-system
├── inventory-system (>=1.0.0)
│   └── event-bus (^2.0)
└── event-bus (^2.0)
```

### 5.2 위상 정렬 결과

```
Level 0: event-bus
Level 1: inventory-system
Level 2: combat-system
```

### 5.3 버전 호환성 검사

- `>=1.0.0`: 1.0.0 이상의 모든 버전
- `^2.0`: 2.0.0 이상 3.0.0 미만
- `~1.2`: 1.2.0 이상 1.3.0 미만
- exact: 정확한 버전 일치

## 6. 순환 의존성 감지

### 6.1 감지 알고리즘

```rust
fn detect_cycle(graph: &HashMap<String, Vec<String>>) -> Option<Vec<String>> {
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut path = Vec::new();

    for node in graph.keys() {
        if !visited.contains(node) {
            if dfs_cycle_detect(graph, node, &mut visited, &mut rec_stack, &mut path) {
                return Some(path);
            }
        }
    }
    None
}

fn dfs_cycle_detect(
    graph: &HashMap<String, Vec<String>>,
    node: &str,
    visited: &mut HashSet<String>,
    rec_stack: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> bool {
    visited.insert(node.to_string());
    rec_stack.insert(node.to_string());
    path.push(node.to_string());

    if let Some(deps) = graph.get(node) {
        for dep in deps {
            if !visited.contains(dep) {
                if dfs_cycle_detect(graph, dep, visited, rec_stack, path) {
                    return true;
                }
            } else if rec_stack.contains(dep) {
                path.push(dep.to_string());
                return true;
            }
        }
    }

    rec_stack.remove(node);
    path.pop();
    false
}
```

### 6.2 순환 의존성 처리

- 순환 감지 시 관련 플러그인 그룹을 식별
- 에러 메시지에 순환 경로 포함
- 해당 플러그인 그룹을 로딩에서 제외
- 관리자에게 알림 발송

## 7. 플러그인 격리

### 7.1 메모리 격리

- 각 플러그인은 독립된 Wasmtime Store 보유
- Store 간 직접 메모리 접근 불가
- Linear Memory는 플러그인당 최대 256MB
- Host Function을 통해서만 데이터 교환

### 7.2 실행 격리

- 각 플러그인 함수 호출은 Fuel 제한 적용
- 기본 1,000,000 Fuel, 매니페스트에서 증가 가능
- Fuel 소진 시 함수 실행 자동 중단
- 플러그인 간 실행 스케줄링은 런타임이 담당

### 7.3 격리 수준

```
┌─────────────────────────────────────────────┐
│            Server Process (Host)            │
│  ┌───────────────────────────────────────┐  │
│  │         WASM Runtime (Wasmtime)       │  │
│  │  ┌─────────┐  ┌─────────┐            │  │
│  │  │ Store 1 │  │ Store 2 │  ...       │  │
│  │  │Plugin A │  │Plugin B │            │  │
│  │  │ Memory  │  │ Memory  │            │  │
│  │  │ (isolated)│ │ (isolated)│          │  │
│  │  └─────────┘  └─────────┘            │  │
│  │         ↕ Host Functions ↕            │  │
│  └───────────────────────────────────────┘  │
│         Managed State (shared)              │
└─────────────────────────────────────────────┘
```

## 8. 현재 구현 vs 미구현 비교

| 기능 | 상태 | 위치 |
|------|------|------|
| 플러그인 매니페스트 구조체 | ✅ 구현 | `core/plugin/src/lib.rs:31` |
| 플러그인 상태 머신 | ✅ 구현 | `core/plugin/src/lib.rs:54` |
| 플러그인 디스커버리 | ✅ 구현 | `core/plugin/src/lib.rs:93` |
| 플러그인 로딩/활성화/비활성화 | ✅ 구현 | `core/plugin/src/lib.rs:131` |
| WASM 런타임 통합 | ❌ 미구현 | - |
| 의존성 그래프 해석 | ❌ 미구현 | - |
| 순환 의존성 감지 | ❌ 미구현 | - |
| 플러그인 격리 (WASM Store) | ❌ 미구현 | - |
| Host Function 인터페이스 | ❌ 미구현 | - |
| API 버전 검증 | ❌ 미구현 | - |
| 권한 검증 | ❌ 미구현 | - |
| 리소스 제한 (Fuel/Memory) | ❌ 미구현 | - |
| 플러그인 이벤트 시스템 | ❌ 미구현 | - |
| 플러그인 명령어 시스템 | ❌ 미구현 | - |
| TypeScript SDK | ❌ 미구현 | `sdk/typescript/` |
| C# SDK | ❌ 미구현 | `sdk/csharp/` |

## 9. 구현 우선순위

### Phase 1: WASM 런타임 기본 (핵심)

1. **Wasmtime 통합** - Engine, Store, Module, Instance 관리
2. **Host Function 인터페이스** - logging, storage, events
3. **플러그인 로딩 파이프라인** - WASM 파일 로드 → 컴파일 → 인스턴스화
4. **기본 생명주기** - Load → Init → Enable → Disable → Unload

### Phase 2: 의존성 관리

5. **의존성 그래프 해석** - 위상 정렬
6. **순환 의존성 감지** - DFS 기반
7. **버전 호환성 검사** - semver 비교
8. **점진적 로딩** - 의존성 순서 보장

### Phase 3: 격리 및 보안

9. **Fuel Metering** - 실행 제한
10. **Memory Limit** - 메모리 제한
11. **권한 검증** - API 호출 시 권한 체크
12. **격리 강화** - Store 격리, 리소스 격리

### Phase 4: SDK 및 도구

13. **TypeScript SDK** - AssemblyScript/wasm-bindgen 기반
14. **C# SDK** - .NET WASM 기반
15. **플러그인 테스트 프레임워크**
16. **플러그인 레지스트리**

### Phase 5: 고급 기능

17. **핫 리로딩** - 개발 중 실시간 리로드
18. **플러그인 간 통신** - 이벤트 기반
19. **플러그인 UI** - 클라이언트 측 렌더링
20. **플러그인 프로파일링** - 성능 측정 도구

## 10. 에러 처리 전략

### 10.1 에러 분류

```rust
pub enum PluginError {
    NotFound(String),                    // 플러그인 미발견
    IncompatibleApiVersion { ... },      // API 버전 불일치
    PermissionDenied { ... },            // 권한 부족
    InitFailed(String),                  // 초기화 실패
    Wasm(String),                        // WASM 런타임 에러
    DependencyCycle(Vec<String>),        // 순환 의존성
    DependencyMissing(String, String),   // 의존성 미충족
    ResourceExceeded(String),            // 리소스 초과
    ExecutionTimeout(String),            // 실행 시간 초과
}
```

### 10.2 복구 전략

- **InitFailed**: 해당 플러그인만 비활성화, 나머지 정상 동작
- **DependencyMissing**: 의존성 플러그인 먼저 로드 시도
- **ExecutionTimeout**: 플러그인 재시작 또는 비활성화
- **ResourceExceeded**: 리소스 제한 증가 또는 플러그인 교체 요청

## 11. 모니터링 및 로깅

- 각 플러그인의 상태 변경 시 `tracing`으로 로깅
- 플러그인별 Fuel 사용량 추적
- 플러그인별 메모리 사용량 추적
- 에러 발생 시 상세 정보 로깅
- 플러그인 시작/종료 시간 기록
