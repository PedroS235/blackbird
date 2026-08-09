//! Agentic tuning feedback. Validated against the prototype at
//! `.scratch/agent-feedback/` (primary source: branch `prototype/agent-feedback`) —
//! this module ports its preamble, its per-panel message shape, and its
//! click → loading → response state model, backed by a real call instead of
//! a mock.
//!
//! Knows nothing about the parser or egui: callers hand it plain metrics
//! (via [`prompt`]) and read plain `Result<String, String>` back.

mod prompt;

use std::sync::mpsc::{Receiver, channel};

use rig::client::{AgentClientExt, ProviderClient};
use rig::completion::Prompt as _;
use rig::providers::openai;

pub use prompt::{PidGains, psd_message, step_response_message};

const MODEL: &str = "gpt-4o-mini";

/// System prompt: reason about the metrics before recommending anything, and
/// keep step response (PID) and PSD (filters) separate — one button asks
/// about one domain, so a change is never suggested in the wrong one.
const PREAMBLE: &str = "You are a highly qualified drone tuner of Betaflight drones. A pilot \
gives you their flight logs and your job is to analyze them and give the pilot feedback what \
values to tweak in order to have the drone fly better and locked in to their inputs.

Reason about the metrics first, out loud, before you give any recommendation. Step response \
metrics are driven by PID; PSD / noise metrics are driven by filters — do not suggest a PID \
change to fix a filtering problem or vice versa.

Structure your answer as:
Diagnosis: what the metrics say, reasoned out loud.
Recommended changes: what to tweak and why.
A copy-pasteable Betaflight CLI block with the exact commands.";

/// One tuning-feedback call, fired on a background thread so a slow or
/// stalled API call never blocks a frame. `poll` is the only way back in.
pub struct Request {
    rx: Receiver<Result<String, String>>,
}

impl Request {
    pub fn spawn(user_message: String) -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(ask(&user_message));
        });
        Self { rx }
    }

    /// Never blocks — `None` while still in flight.
    pub fn poll(&self) -> Option<Result<String, String>> {
        self.rx.try_recv().ok()
    }
}

fn ask(user_message: &str) -> Result<String, String> {
    tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(async {
            let client = openai::Client::from_env().map_err(|e| e.to_string())?;
            let agent = client.agent(MODEL).preamble(PREAMBLE).build();
            agent.prompt(user_message).await.map_err(|e| e.to_string())
        })
}

/// Where an "ask AI" button's click sits, from idle through to a shown
/// answer. One field on the owning panel, so it cannot show a spinner and a
/// stale response at once.
#[derive(Default)]
pub enum Feedback {
    #[default]
    Idle,
    Loading(Request),
    Done(Result<String, String>),
}

impl Feedback {
    pub fn ask(&mut self, user_message: String) {
        *self = Self::Loading(Request::spawn(user_message));
    }

    /// Call once per frame before drawing. Moves `Loading` to `Done` the
    /// frame a reply lands; a no-op otherwise.
    pub fn poll(&mut self) {
        if let Self::Loading(req) = self
            && let Some(result) = req.poll()
        {
            *self = Self::Done(result);
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading(_))
    }
}
