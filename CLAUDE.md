# AI Usage Widget — Claude Instructions

Windows 데스크톱에 떠 있는 작은 Electron 위젯. Claude / Codex 5시간·주간 사용률을 나란히 보여준다.

## 기술 스택

- **Electron 37** (frameless, transparent, always-on-top BrowserWindow)
- **Node 24+**
- **Vanilla JS / CSS** — 프레임워크 없음. `renderer/` 안에 `index.html`, `styles.css`, `renderer.js`
- **electron-packager** — `npm run dist`로 portable Windows zip 빌드 (코드 서명 없음)
- **node:test** — `lib/widget-core.js` 순수 함수 단위 테스트 (`test/widget-core.test.js`)
- **@fontsource/silkscreen** — 픽셀 폰트
- 의존성 최소 — production deps 없음. devDeps만.

## 아키텍처

- `main.js` — Electron main 프로세스. tray, window, settings, fetch loop, native notifications
- `lib/widget-core.js` — pure 헬퍼 (display percent 계산, threshold cross 판정, 모드 라벨). main에서 require, 테스트 대상
- `preload.js` — contextBridge로 renderer ↔ main IPC 노출
- `renderer/` — UI. main에서 `usage:update` IPC 푸시 받음
- `scripts/capture-preview.js` — README용 스크린샷 캡처 헬퍼

## 데이터 소스

- **Codex**: `~/.codex/auth.json` Bearer + `https://chatgpt.com/backend-api/wham/usage`
- **Claude**: claude.ai 세션 쿠키 → org UUID 동적 조회 → `https://claude.ai/api/organizations/{org}/usage`
- 토큰은 절대 디스크에 다시 쓰지 않는다. 메모리·요청 헤더에만.

## 컨벤션

- **OS 경로는 코드/문서 어디서도 하드코딩 금지** — `app.getPath('userData')`, `process.env.USERPROFILE`, README는 `%APPDATA%` / `%USERPROFILE%` 표기
- **BrowserWindow 보안**: `nodeIntegration: false`, `contextIsolation: true`, preload 경유. 최근 커밋(`794ae51`)에서 명시적 hardening 완료 — 후속 창 추가 시 동일 패턴 유지
- **commit message**: 영어 짧은 prefix (`fix:`, `chore:`, `security:`, `feat:`)
- **테스트는 pure 함수 위주** — Electron API 모킹하지 않는다. UI 검증은 `npm start`로 직접
- **외부 공유 레포**라 사적 메모(`BUGLOG.md` 등)에 사용자 경로·org id 같은 식별정보 새지 않게 주의

## 작업 시 참고

- 새 작업 전 `CLAUDE.local.md`(gitignored)에 이어서 할 일이 있는지 확인
- 큰 변경은 `ROADMAP.md`의 마일스톤과 정렬
- UI 변경은 빌드 후 실제 위젯에서 시각 확인 — type check만으로 끝내지 말 것
