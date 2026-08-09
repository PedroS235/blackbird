# Agentic tuning feedback

An "ask AI" button (`egui_phosphor::regular::OPEN_AI_LOGO`) on the Step
Response and PSD panels. Click it, the panel's already-computed metrics get
sent to an LLM with a preamble that reasons before recommending, and the
answer (Diagnosis → Recommended changes → Betaflight CLI block) renders
under the button.

## Question the prototype answered

Does sending computed metrics (not raw timeseries) plus a hand-written
preamble to a real LLM produce feedback worth showing a pilot, and does a
click → loading → response state model feel right?

**Primary source**: branch `prototype/agent-feedback`
(`.scratch/agent-feedback/PROTOTYPE-agent-feedback.html`, `src/bin/llm.rs`).
Not on `main` — the HTML mockup and the throwaway experiment stay there;
`main` keeps only the validated decision, built for real on
`feature/agent-feedback`.

**Answer**: yes on both counts. Ported as-is: the preamble (`ai::PREAMBLE`),
the per-panel metrics-to-message shape (`ai::prompt::{step_response_message,
psd_message}`), and the state model (`ai::Feedback::{Idle, Loading, Done}`).

## Decisions made along the way

- **Backend: OpenAI via `rig`, not Anthropic/Ollama.** CLAUDE.md's original
  `LlmBackend` trait spec'd Anthropic + Ollama over raw `reqwest`. The
  prototype tested OpenAI (matching an existing `rig` experiment and the
  chosen icon), and that's what shipped — no trait, since there's one
  backend. See CLAUDE.md's "AI integration" section for the fuller
  rationale and what the original design still leaves open.
- **No settings panel yet.** API key comes from `OPENAI_API_KEY`, read by
  `rig`'s `openai::Client::from_env()`. A settings panel remains open
  (CLAUDE.md's "Open questions").
- **`tokyo` dependency removed.** A one-letter-off typosquat of `tokio` that
  had been added to `Cargo.toml` by mistake (not a `rig` dependency, nothing
  in `src/` used it) — pulled before this branch existed.

## Scope not covered

- Only Step Response and PSD carry the button — the two panels the pilot
  named as making sense for this. Filter Analysis' other sub-tabs
  (Frequency, Vs Reference, Spectrogram) and PID Analysis' gyro-vs-setpoint
  don't have one.
- No streaming — one blocking-ish call per click, rendered whole once it
  lands.
- No dedicated `ai_panel.rs` / copyable CLI block widget — the response
  renders as plain text inside an `elegance::Card`.

## Comments

None yet.
