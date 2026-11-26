//! Process-wide PostHog client for the desktop application.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use gpui::{App, AppContext as _};
use posthog_rs::{Client, ClientOptionsBuilder, ErrorTrackingOptionsBuilder, Event};

static CLIENT: OnceLock<Arc<Client>> = OnceLock::new();
static ANONYMOUS_SESSION_ID: OnceLock<String> = OnceLock::new();
static AI_TRACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Starts the one PostHog client used by the application.
pub(crate) fn init(cx: &mut App) {
    let token = std::env::var("POSTHOG_PROJECT_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let host = std::env::var("POSTHOG_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let (Some(token), Some(host)) = (token, host) else {
        if cfg!(debug_assertions) {
            if std::env::var("POSTHOG_PROJECT_TOKEN")
                .ok()
                .is_none_or(|value| value.trim().is_empty())
            {
                eprintln!(
                    "POSTHOG_PROJECT_TOKEN variable required by PostHog is missing or un-configured, this causes events to be silently missed. This error stops appearing once POSTHOG_PROJECT_TOKEN is configured"
                );
            }
            if std::env::var("POSTHOG_HOST")
                .ok()
                .is_none_or(|value| value.trim().is_empty())
            {
                eprintln!(
                    "POSTHOG_HOST variable required by PostHog is missing or un-configured, this causes events to be silently missed. This error stops appearing once POSTHOG_HOST is configured"
                );
            }
        }
        return;
    };

    cx.background_spawn(async move {
        let error_tracking = match ErrorTrackingOptionsBuilder::default()
            .capture_panics(true)
            .build()
        {
            Ok(options) => options,
            Err(error) => {
                eprintln!("PostHog panic capture could not be configured: {error}");
                return;
            }
        };
        let options = match ClientOptionsBuilder::default()
            .api_key(token)
            .host(host)
            .error_tracking(error_tracking)
            .build()
        {
            Ok(options) => options,
            Err(error) => {
                eprintln!("PostHog client could not be configured: {error}");
                return;
            }
        };

        // `init_global` installs the SDK's process-wide panic hook, preserving
        // the previously installed hook while delivering uncaught panics as
        // PostHog exceptions through this one shared client.
        match posthog_rs::init_global(options).await {
            Ok(client) => {
                let _ = CLIENT.set(Arc::new(client));
            }
            Err(error) => eprintln!("PostHog panic capture could not start: {error}"),
        }
    })
    .detach();
}

/// Returns the initialized shared client for event call sites.
pub(crate) fn client() -> Option<Arc<Client>> {
    CLIENT.get().cloned()
}

/// Captures an anonymous product event without creating a person profile.
pub(crate) fn capture(
    event_name: &'static str,
    properties: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) {
    let Some(client) = client() else {
        return;
    };
    let session_id = anonymous_session_id();
    let mut event = Event::new(event_name, session_id);
    let _ = event.insert_prop("$process_person_profile", false);
    for (key, value) in properties {
        let _ = event.insert_prop(key, value);
    }
    client.capture(event);
}

/// Captures one agent CLI turn or one-shot request as an AI generation. The CLIs do not expose their underlying
/// provider requests, so this intentionally records the app-visible request and completed response boundary.
pub(crate) fn capture_ai_generation(
    ai_session_id: &str,
    span_name: &'static str,
    provider: &'static str,
    model: Option<&str>,
    inputs: &[String],
    output: Option<&str>,
    latency_ms: u64,
    is_error: bool,
    total_cost_usd: Option<f64>,
) {
    let Some(client) = client() else {
        return;
    };
    let trace_sequence = AI_TRACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let process_session_id = anonymous_session_id();
    let mut event = Event::new("$ai_generation", process_session_id);
    let _ = event.insert_prop("$process_person_profile", false);
    let _ = event.insert_prop(
        "$ai_trace_id",
        format!("{process_session_id}-ai-{trace_sequence}"),
    );
    let _ = event.insert_prop("$ai_session_id", ai_session_id);
    let _ = event.insert_prop("$ai_span_name", span_name);
    let _ = event.insert_prop("$ai_provider", provider);
    if let Some(model) = model.filter(|model| !model.is_empty()) {
        let _ = event.insert_prop("$ai_model", model);
    }
    let _ = event.insert_prop(
        "$ai_input",
        serde_json::Value::Array(
            inputs
                .iter()
                .map(|input| serde_json::json!({ "role": "user", "content": input }))
                .collect(),
        ),
    );
    if let Some(output) = output.filter(|output| !output.is_empty()) {
        let _ = event.insert_prop(
            "$ai_output_choices",
            serde_json::json!([{ "role": "assistant", "content": output }]),
        );
    }
    let _ = event.insert_prop("$ai_latency", latency_ms as f64 / 1_000.0);
    let _ = event.insert_prop("$ai_is_error", is_error);
    if let Some(cost) = total_cost_usd {
        let _ = event.insert_prop("$ai_total_cost_usd", cost);
    }
    client.capture(event);
}

/// Maps SkyeLab's configured CLI products to the provider label the app can observe without inspecting their traffic.
pub(crate) fn ai_provider(provider: skyelab_core::Provider) -> &'static str {
    match provider {
        skyelab_core::Provider::Claude => "anthropic",
        skyelab_core::Provider::Codex => "openai",
        skyelab_core::Provider::Cursor => "cursor",
        skyelab_core::Provider::Antigravity => "google",
    }
}

fn anonymous_session_id() -> &'static str {
    ANONYMOUS_SESSION_ID.get_or_init(|| {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        format!("desktop-session-{timestamp}")
    })
}

/// Flushes queued analytics and stops the background worker while the app exits.
pub(crate) fn shutdown(cx: &mut App) {
    cx.on_app_quit(|_| async {
        if let Some(client) = client() {
            client.flush().await;
            client.shutdown().await;
        }
    })
    .detach();
}
