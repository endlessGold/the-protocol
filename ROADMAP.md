# The Protocol — 로드맵

> 최종 갱신: 2026-08-28
> 이 프로젝트는 서로 독립적인 두 개의 GitHub 저장소로 구성되어 있습니다.
> 체크리스트는 각 저장소의 **Issues + Milestones**로 관리합니다 — 이 파일은
> 그 둘을 찾아가기 위한 진입점입니다. 개별 항목의 상태(완료/진행중)를 여기에
> 중복 기록하지 않습니다 — GitHub이 유일한 출처(source of truth)입니다.

## 저장소

| 저장소 | 내용 | 로드맵 |
|---|---|---|
| [endlessGold/the-protocol](https://github.com/endlessGold/the-protocol) | Rust MUD 게임 런타임 (이 디렉토리) | [Milestones](https://github.com/endlessGold/the-protocol/milestones) · [Issues](https://github.com/endlessGold/the-protocol/issues) |
| [endlessGold/entity-naming](https://github.com/endlessGold/entity-naming) | MMO 엔티티 네이밍 엔진 (`entity-naming/`) | [Issues](https://github.com/endlessGold/entity-naming/issues) |

## the-protocol 마일스톤 구조

Critical → High → Medium → Low 순서의 4단계 Phase로 구성. 각 Phase가
끝나면 다음 단계로 넘어갑니다.

1. **Phase 1 — Critical**: 멀티플레이어가 실제로 동작하는 데 필요한 최소 조건
   (네트워크↔라우팅 연결, 캐릭터 ID 동적 할당, 코덱 버그).
2. **Phase 2 — High**: 세션/클라이언트 정합성 (session_id 하드코딩 제거,
   TCP/UDP 계층 분리).
3. **Phase 3 — Medium**: 품질/기능 완성도 (전투 시스템, 보안, 플러그인 뼈대).
4. **Phase 4 — Low**: 확장 (Gateway, API, SDK, 테스트, 코드 정리).

세부 근거는 감사 보고서에 남아있습니다: [`docs/00-status/implementation-status.md`](docs/00-status/implementation-status.md),
[`docs/00-status/known-issues.md`](docs/00-status/known-issues.md).

## entity-naming

Phase 구조 없이 남은 작업이 이슈로 등록되어 있습니다 (대부분 `needs-real-env`
라벨 — 이 개발 환경에 Python/API 키 등이 없어 검증하지 못한 항목). 세션 기록은
[`entity-naming/docs/status/2026-08-28-session.md`](entity-naming/docs/status/2026-08-28-session.md)
참고.

## 새 세션을 시작할 때

1. 이 파일 → 관련 저장소의 열려있는 Milestone/Issues 확인
   (`gh issue list --milestone "Phase 1 — ..."` 또는 웹에서).
2. 작업을 끝내면 해당 Issue를 닫고, 커밋 메시지에 `Closes #N`을 남깁니다.
3. 새로 발견한 문제는 적절한 저장소에 Issue로 등록합니다 (라벨:
   `priority:critical/high/medium/low` — the-protocol, `needs-real-env` 등 —
   entity-naming).
