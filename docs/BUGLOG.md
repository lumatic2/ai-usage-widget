# Bug Log

## 2026-04-17 — Claude 5시간/주간 사용량이 간헐적으로 100%로 표시

### 증상
위젯 Claude 패널에서 5-hour, weekly 사용률이 실제와 달리 100%로 간헐적으로 찍힘.
새로고침 주기마다 100% → 정상값 → 100% 식으로 깜빡임.

### 근본 원인
`main.js` `normalizeClaudeUtilization` 함수의 스케일 자동감지 휴리스틱:

```js
const pct = numeric > 1 ? numeric : numeric * 100;
```

커밋 `f2100b2`에서 "API가 분수(0.0–1.0)로도, 퍼센트(0–100)로도 올 수 있다"는
가정으로 추가된 로직. 그러나 라이브 API(`https://claude.ai/api/organizations/{org}/usage`)를
직접 호출해 확인한 결과, 응답은 **항상 0–100 퍼센트 스케일**이었다.

```json
{ "five_hour": { "utilization": 8.0, ... },
  "seven_day": { "utilization": 2.0, ... } }
```

따라서 사용률이 `≤ 1` 구간(예: `1.0`, `0.5`)을 지나갈 때 휴리스틱이
"분수로 해석"해 `×100`을 적용 → 1.0% → 100%, 0.5% → 50% 로 뻥튀기.

### 간헐적으로 보이는 이유
- 5시간 창이 막 리셋된 직후, 실제 사용률이 0 → 1 → 2 % 로 올라가는 짧은 구간에만
  문제 값이 나옴. 2%를 넘기면 `numeric > 1` 분기를 타서 정상 복원.
- 주간(7일) 창은 사용률이 낮은 요일에 상시 1% 이하 → 반복적으로 100% 표시.
- 60초 refresh + 5분 in-memory cache 때문에 타이밍에 따라 "한 번씩 찍히고 정상"으로 보임.

### 수정
`main.js` (`normalizeClaudeUtilization`):
- 휴리스틱 제거. 값은 항상 0–100 퍼센트로 간주하고 `clamp(0, 100, round(value))` 만 적용.
- 관련 진단 `console.log` / `console.warn` 도 정리 (더 이상 scale 문제가 없으므로 불필요).

### 검증 방법
- 라이브 API 호출: `curl -H "Cookie: sessionKey=$SESS" https://claude.ai/api/organizations/$ORG/usage`
  → `utilization` 필드가 정수/소수 모두 0–100 범위인지 확인.
- 위젯 재시작 후 1% 언저리 사용률에서도 100%로 찍히지 않는지 확인.

### 관련 파일
- `main.js:333` `normalizeClaudeUtilization`
- `main.js:346` `parseClaudeWindow` (진단 로그 정리)
- 관련 커밋: `f2100b2` (원인 도입), 본 수정 커밋 (해결)
