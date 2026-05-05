# Roadmap

외부 공유용 polish 중심. 우선순위 = 배포 신뢰도에 미치는 영향 순.

## Milestone 1 — 배포 전 막아야 할 것 ✅

- [x] README 하드코딩 경로 제거 (`C:\Users\1\AppData\...` → `%APPDATA%` / `%USERPROFILE%`)
- [x] 루트 stray 파일 정리 (`Error`, `HTTP`)
- [x] `package.json` 메타데이터 보강 (`license`, `author`, `repository`, `homepage`, `bugs`)
- [x] 첫 실행 동의 다이얼로그 (consentAccepted 1회 저장)
- [x] 첫 실행 패널 선택 다이얼로그 (Claude+Codex / Claude only / Codex only)
- [x] Windows SmartScreen 안내 README 섹션
- [x] 트레이 아이콘 제거 (Windows에서 투명 렌더링 이슈 + 사용 빈도 낮음). X 버튼이 종료 역할
- [x] 패널 토글 시 창 너비 자동 조정 (단일 410 / 양쪽 780)
- [x] CSS specificity 버그 수정 (`.col.col--hidden`로 우선순위 올림)
- [x] 설정 패널 absolute overlay + 자체 스크롤로 잘림 방지

## Milestone 2 — 가치 큰 폴리싱

- [ ] GitHub Actions release 워크플로 — 태그 푸시 시 산출물 자동 첨부
- [ ] 첫 실행 UX 개선 — Claude 세션 없을 때 "Click to sign in" + 가이드
- [ ] 에러 가시성 — 네트워크 실패/auth 만료 시 명시적 상태 표시
- [ ] macOS 지원 의도 명시 — README

## Milestone 3 — 리포 위생 / 문서

- [ ] `BUGLOG.md` 위치 검토 (공개 OK인지 확인 또는 `docs/`로 이동)
- [ ] README 첫인상 개선 — 정적 PNG → 3초 GIF
- [ ] "왜 만들었는가" 한 단락
- [ ] i18n 검토 — 라벨 영어 고정 vs 한국어 토글

## Milestone 4 — Tauri 마이그레이션

목표: Electron(~150MB, ~150MB RAM) → Tauri(~10MB, ~30MB RAM). 외부 공유 시 사용자 설치 마찰 감소.

### Phase A — PoC (1일, `tauri-migration-poc` 브랜치)
- [ ] Rust toolchain 설치 (`rustup`) + Tauri CLI
- [ ] Claude 로그인 쿠키 추출 PoC — Tauri의 WebviewWindow + cookie store API로 sessionKey 획득 가능한지 확인. **이게 안 되면 마이그레이션 폐기**
- [ ] Codex auth.json 읽기 + Bearer fetch PoC (단순 — `reqwest`)
- [ ] PoC 결과 평가: Go/No-Go 결정

### Phase B — 본격 이식 (Go일 경우, 1~2일)
- [ ] Tauri 프로젝트 스캐폴드, 창 설정 이식 (frameless, transparent, alwaysOnTop)
- [ ] `renderer/`를 `src/`로 복사, preload → `invoke()` 패턴 치환
- [ ] `main.js` 백엔드 로직을 Rust로 포팅 (settings, fetch, login, alerts, refresh timer)
- [ ] `lib/widget-core.js` 순수 함수 Rust 포팅 + 테스트 이식
- [ ] `npm run tauri build` → MSI/portable exe 산출, 크기·동작 검증
- [ ] README 업데이트 (Quick Start, Build 섹션을 Tauri 기준으로)

### Phase C — 정리 (반나절)
- [ ] Electron 의존성 제거 (`electron`, `@electron/packager`, `@fontsource/silkscreen` 빌드 경로 검토)
- [ ] 기존 Electron 코드는 `archive/electron/`로 이동 또는 삭제
- [ ] CI 릴리스 워크플로 Tauri 빌드로 전환

### 위험 / 폐기 조건
- Phase A에서 Claude sessionKey 추출 불가 시 폐기 (Electron 유지)
- Rust 빌드 환경 안정성 문제 (Windows MSVC 의존성) — 이슈 있으면 Phase B 이전에 차단

## 완료 이력

- 2026-05-05 — **Milestone 1 완료** (외부 공유 준비). 동의 + 패널 선택 첫 실행 흐름, 트레이 제거, 패널 토글 + 자동 리사이즈, CSS specificity 버그, README 정비
- 2026-04-24 (`b21985d`) — main widget window의 `sandbox: true` 제거
- 2026-04-24 (`794ae51`) — 모든 BrowserWindow에 명시적 webPreferences hardening
- 2026-04-24 (`4bb8157`) — 프로젝트 이름 "Codex Pixel Widget" → "AI Usage Widget"
- 2026-04-24 (`fd70bfd`) — Windows에서 투명 배경 복구
- 2026-04-17 (`31f1a6d`) — Claude 사용률 ≤1% 구간 100% 표시 버그 수정
