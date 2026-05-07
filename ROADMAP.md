# Roadmap

> 마지막 업데이트: 2026-05-07 (v0.1.0 cutting + first-run UX + 에러 가시성 + README polish)

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

- [x] GitHub Actions release 워크플로 — 태그 푸시 시 산출물 자동 첨부 (`.github/workflows/release.yml`, NSIS+MSI on `v*` tag)
- [x] 첫 실행 UX 개선 — Claude 세션 없을 때 "Click to sign in" + 가이드 (NotConfigured에도 LOGIN 노출)
- [x] 에러 가시성 — Codex쪽도 typed error enum (`NotConfigured`/`SessionExpired`/`Other`)로 분리, OFF/expired/stale 상태 패널 표시
- [x] macOS 지원 의도 명시 — README

## Milestone 3 — 리포 위생 / 문서

- [x] `BUGLOG.md` 위치 검토 — `docs/BUGLOG.md`로 이동 (공개 OK 콘텐츠지만 루트 정돈)
- [ ] README 첫인상 개선 — 정적 PNG → 3초 GIF
- [x] "왜 만들었는가" 한 단락
- [x] i18n 토글 (en/ko) — 첫 실행 모달 + 설정 패널 + 상태 라벨. 5-HOUR/WEEKLY/OFF/LOGIN 등 브랜드성 라벨은 영문 유지

## Milestone 4 — Tauri 마이그레이션

목표: Electron(~150MB, ~150MB RAM) → Tauri(~10MB, ~30MB RAM). 외부 공유 시 사용자 설치 마찰 감소.

### Phase A — PoC ✅
- [x] Rust toolchain 설치 (`rustup` + MSVC Build Tools 2022 + Win11 SDK)
- [x] Smart App Control off (Rust unsigned 빌드 산출물 차단 회피)
- [x] Claude 로그인 쿠키 추출 PoC — `WebviewWindow::cookies_for_url`로 sessionKey 획득 검증됨
- [x] PoC 결과 평가: **Go**

### Phase B — 본격 이식 ✅ (남은 polish 1건)
- [x] Tauri 프로젝트 스캐폴드 (`tauri-poc/`), 창 설정 이식 (frameless, transparent, alwaysOnTop, skipTaskbar, shadow off, 780/410×320)
- [x] `renderer/`를 `tauri-poc/src/`로 복사. preload → `window.codexWidget` shim으로 `invoke()`/`listen()` 매핑
- [x] `data-tauri-drag-region` 도입
- [x] Silkscreen 폰트 — Google Fonts CDN
- [x] Settings 디스크 영속화 — `%APPDATA%/com.lumatic2.ai-usage-widget/settings.json` + sanitize/clamp
- [x] 위치 자동 저장 + 시작 시 복원
- [x] 패널 토글 시 창 자동 리사이즈
- [x] **Codex 백엔드 포팅** (`codex.rs`) — `auth.json` Bearer + retry (`fetch_retries` 횟수, 401/403 즉시 expired, 429/5xx 백오프, status check가 JSON 파싱보다 먼저)
- [x] **Claude 백엔드 포팅** (`claude.rs`) — Bearer + 쿠키 fallback 둘 다 retry. Org UUID는 settings에 캐시. `poll_claude_session_cookie`가 sessionKey + org UUID 같이 잡음
- [x] **Refresh timer** — `tauri::async_runtime::spawn` + `tokio::time::sleep` 루프, `usage:update` emit
- [x] **세션 라벨** (`session.rs`) — `~/.codex/sessions/`에서 최신 rollout-*.jsonl 256KB tail 파싱, TTL/mtime 캐시
- [x] **Last-good 캐시** — Codex/Claude 각각 5분 TTL. fetch 실패 시 stale 값 + error 메시지로 패널 유지 (Claude는 5h window rollover 시 자동 무효화)
- [x] **임계값 알림** — `tauri-plugin-notification` (Windows toast). per-window `WindowAlertSlot` (last_value + notified set, 10% drop 시 reset 감지)
- [x] **TLS 호환성** — `rustls-tls-native-roots` (Windows SChannel + 사내 CA)
- [x] **자동 시작** — `tauri-plugin-autostart` (`openOnStartup` 토글 동기화)
- [x] **Production build 검증** — `tauri-poc.exe` **13MB**, NSIS **2.9MB**, MSI **4.3MB**, cold start **664ms** (warm WV2), Rust 본체 RSS **47MB** + WebView2 자식 6개 합 ~352MB. ROADMAP 가설 "30MB RAM"은 틀렸으나 디스크 11.5× 절감은 외부 공유 마찰의 핵심
- [x] `lib/widget-core.js` 순수 함수 + 테스트 Rust 포팅 — `widget_core.rs` 8개 단위 테스트 통과 (2026-05-07)
- [x] 첫 실행 UX (consent + panel selection) Tauri 측에 포팅 — Milestone 1 동등 수준 (2026-05-07)

### Phase C — 정리 ✅
- [x] `tauri-migration-poc` → `main` merge (`eed000b` `--no-ff`). Electron 흔적은 `archive/electron/`로 shell mv 후 `git rm` (gitignored, 로컬 보존)
- [x] 루트 `package.json` thin wrapper (`npm run dev/build/tauri` → tauri-poc)
- [x] `.gitignore` — `target/`, `archive/`, `CLAUDE.local.md` 추가
- [x] README + CLAUDE.md Tauri 기준 재작성. NSIS/MSI installer 안내 + 새 dev/build 명령
- [x] CI 릴리스 워크플로 — `.github/workflows/release.yml` (`v*` 태그 → NSIS+MSI 자동 첨부 draft release)
- [x] 로컬 정리: tauri 워크트리 제거 + 머지된 브랜치 삭제 + 옛 Electron `node_modules/` 345MB 회수 + 6개 백그라운드 Electron 프로세스 종료
- [x] GitHub repo 메타데이터 — description "Tauri 2 + Rust" 갱신, PUBLIC 유지 확인
- [x] 보안 재확인 — 워킹트리·전체 commit 히스토리 token 패턴 0건. `archive/`/`target/`/`CLAUDE.local.md` gitignore 적용 확인

### 위험 / 폐기 조건
- Phase A에서 Claude sessionKey 추출 불가 시 폐기 (Electron 유지)
- Rust 빌드 환경 안정성 문제 (Windows MSVC 의존성) — 이슈 있으면 Phase B 이전에 차단

## 완료 이력

- 2026-05-07 — **v0.1.0 첫 Tauri 릴리스 컷** (`b8dd016` + `ccd6ed0` + `06d4316`). 첫 실행 consent + 패널 선택 다이얼로그 Tauri 측 포팅, refresh emit 채널 버그 수정 (`usage:update` → `widget-state`), Codex 에러 타이핑 + OFF/expired/stale 패널 상태, README "Why" + macOS 의도, BUGLOG → `docs/`. 옛 Electron 시절 `v0.1.0` 태그 제거 후 재생성 (origin/main HEAD)
- 2026-05-06 — **Tauri 마이그레이션 종료** (`eed000b` merge to main + `b2cba86` GH Actions). Phase C: Electron 파일 archive 이동, README/CLAUDE.md/package.json Tauri 재구성, release.yml workflow, GitHub repo description 갱신, 보안 재확인 (토큰 0건). production exe 12.2MB / NSIS 2.9MB / MSI 4.3MB. Milestone 4 close
- 2026-05-05 — **Tauri Phase B 본체 완료** (`2161069` + 후속 commits, 브랜치 `tauri-migration-poc`). Codex/Claude fetch + retry + 쿠키 fallback + 세션 라벨 + last-good 캐시 + autostart + 임계값 알림. Production build 산출 검증 (exe 13MB, NSIS 2.9MB, cold start 664ms). 남은 polish: widget-core 테스트 포팅, 첫 실행 UX
- 2026-05-05 — **Tauri Phase A 완료 + Phase B step 1** (`15a3754`). 스캐폴드·창 설정·렌더러 이식·shim·settings 디스크 영속화·위치 자동저장
- 2026-05-05 — **Milestone 1 완료** (외부 공유 준비). 동의 + 패널 선택 첫 실행 흐름, 트레이 제거, 패널 토글 + 자동 리사이즈, CSS specificity 버그, README 정비
- 2026-04-24 (`b21985d`) — main widget window의 `sandbox: true` 제거
- 2026-04-24 (`794ae51`) — 모든 BrowserWindow에 명시적 webPreferences hardening
- 2026-04-24 (`4bb8157`) — 프로젝트 이름 "Codex Pixel Widget" → "AI Usage Widget"
- 2026-04-24 (`fd70bfd`) — Windows에서 투명 배경 복구
- 2026-04-17 (`31f1a6d`) — Claude 사용률 ≤1% 구간 100% 표시 버그 수정
