# AI Usage Widget — Claude Instructions

Windows 데스크톱에 떠 있는 작은 Tauri 위젯. Claude / Codex 5시간·주간 사용률을 나란히 보여준다.

## 기술 스택

- **Tauri 2** (frameless, transparent, always-on-top WebviewWindow) + **Rust 1.95+**
- **WebView2** (Windows 시스템 라이브러리) — Electron의 Chromium 통째 X
- **Vanilla JS / CSS** 렌더러 — 프레임워크 없음. `tauri-poc/src/`
- **`reqwest 0.12`** (rustls + Windows 네이티브 신뢰 저장소) — HTTP fetch
- **`tauri-plugin-{notification,autostart,opener}`** — 네이티브 토스트 / 로그온 자동시작 / URL 오픈
- 산출물: `tauri-poc/src-tauri/target/release/` — exe ~13MB, NSIS ~2.9MB, MSI ~4.3MB

## 아키텍처 (`tauri-poc/src-tauri/src/`)

- `lib.rs` — Tauri builder, `#[tauri::command]` 핸들러, 윈도우 / 트레이 / autostart 동기화, refresh 루프, 임계값 알림 dispatch
- `codex.rs` — `~/.codex/auth.json` Bearer + `chatgpt.com/backend-api/wham/usage`. Retry (401/403 즉시 expired, 429/5xx 백오프). Status check가 JSON 파싱보다 먼저
- `claude.rs` — `~/.claude/.credentials.json` Bearer + 쿠키 fallback. `AuthHeader::{Bearer,Cookie}` enum으로 retry 로직 공유. Org UUID는 settings에 캐시 (cookie path도 즉석 resolve 가능)
- `session.rs` — `~/.codex/sessions/`에서 최신 `rollout-*.jsonl` 256KB tail에서 `payload.type=="user_message"` 파싱, TTL/mtime 캐시
- `tauri-poc/src/` — 렌더러 (preload shim이 `window.codexWidget`로 invoke/listen 매핑)

## 데이터 소스

- **Codex**: `~/.codex/auth.json` Bearer + `https://chatgpt.com/backend-api/wham/usage`
- **Claude**: `~/.claude/.credentials.json` OAuth (`claudeAiOauth.accessToken`) → 실패 시 webview 쿠키 (`sessionKey`) 폴백 → `https://claude.ai/api/organizations/{uuid}/usage`
- 토큰은 메모리·요청 헤더에만. 디스크에 다시 쓰지 않음 (단, sessionKey는 사용자 동의 하에 settings에 저장 — 매 refresh마다 재로그인 안 하기 위해)

## 컨벤션

- **OS 경로 하드코딩 금지** — `app.path().app_config_dir()`, `std::env::var("USERPROFILE")`. 문서는 `%APPDATA%` / `%USERPROFILE%` 표기
- **WebviewWindow 보안**: contextIsolation 기본. `claude_login` 같은 외부 URL 창 추가 시 동일 격리 패턴
- **commit message**: 영어 짧은 prefix (`fix:`, `chore:`, `security:`, `feat:`, `poc:`)
- **테스트는 pure 함수 위주** — Tauri API 모킹하지 않는다. UI 검증은 `npm run dev`로 직접
- **외부 공유 레포**라 사적 메모(`BUGLOG.md`, `CLAUDE.local.md` 등)에 사용자 경로·org id·token이 새지 않게 주의

## 빌드 / 개발

```bash
# 루트에서:
npm run dev       # tauri-poc 서브프로젝트로 위임 → tauri dev
npm run build     # release 빌드 (exe + MSI + NSIS)
```

`tauri-poc/src-tauri/Cargo.toml`이 실제 의존성. 루트 `package.json`은 thin wrapper.

## Legacy

기존 Electron 소스는 `archive/electron/` (gitignored 로컬 보관)에 있음. Tauri 이전은 마일스톤 4 / Phase B 완료, Phase C에서 본 정리 진행됨 (2026-05-05).

## 작업 시 참고

- 새 작업 전 `CLAUDE.local.md`(gitignored)에 이어서 할 일이 있는지 확인
- 큰 변경은 `ROADMAP.md`의 마일스톤과 정렬
- UI 변경은 빌드 후 실제 위젯에서 시각 확인 — `cargo check`만으로 끝내지 말 것
