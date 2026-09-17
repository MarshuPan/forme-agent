use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use headless_chrome::browser::tab::RequestPausedDecision;
use headless_chrome::protocol::cdp::types::Event;
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
use headless_chrome::protocol::cdp::{Fetch, Network, Page};
use headless_chrome::Browser;
use url::Url;

use crate::support::{
    cancelled, complete_external, emit_started, fail_unless_terminal, failed, outcome_unknown,
    ActiveCancellations, BoundedOutput,
};
use crate::{
    plan_with, resolve_external_input, validate_plan_for, ActionBackend, ActionResult,
    ArtifactStore, BackendKind, CancelToken, ContentResolver, DriverReceipt, EventSink,
    ExecutionPlan, OutputBudget, SecretResolver,
};

pub trait BrowserDriver: Send + Sync {
    fn perform(
        &self,
        action: &p::BrowserActionSpec,
        secrets: &dyn SecretResolver,
    ) -> p::Result<DriverReceipt>;
}

pub struct HeadlessChromeDriver {
    executable: PathBuf,
    contents: Arc<dyn ContentResolver>,
    artifacts: Arc<dyn ArtifactStore>,
    timeout: p::DurationMs,
}

impl HeadlessChromeDriver {
    pub fn new(
        executable: impl Into<PathBuf>,
        contents: Arc<dyn ContentResolver>,
        artifacts: Arc<dyn ArtifactStore>,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        let executable = executable.into();
        if !executable.is_file() || timeout.0 == 0 {
            return Err(p::Error("browser executable is unavailable".into()));
        }
        Ok(Self {
            executable,
            contents,
            artifacts,
            timeout,
        })
    }
}

impl BrowserDriver for HeadlessChromeDriver {
    fn perform(
        &self,
        action: &p::BrowserActionSpec,
        secrets: &dyn SecretResolver,
    ) -> p::Result<DriverReceipt> {
        let browser = IsolatedChrome::launch(&self.executable, self.timeout)?;
        let tab = browser
            .browser
            .new_tab()
            .map_err(|_| p::Error("headless browser tab could not be opened".into()))?;
        tab.set_default_timeout(Duration::from_millis(self.timeout.0));
        install_origin_guard(&tab, &action.allowed_origins)?;
        tab.navigate_to(&action.target_url)
            .and_then(|tab| tab.wait_until_navigated())
            .map_err(|_| p::Error("headless browser navigation failed".into()))?;
        ensure_allowed_url(&tab.get_url(), &action.allowed_origins)?;

        let (summary, record, effect) = match &action.operation {
            p::BrowserOperation::Navigate => (
                "browser navigation completed".to_owned(),
                None,
                p::EffectStatus::Observed,
            ),
            p::BrowserOperation::ReadText { selector } => {
                let text = match selector {
                    Some(selector) => tab
                        .wait_for_element(selector)
                        .and_then(|element| element.get_inner_text()),
                    None => tab
                        .wait_for_element("body")
                        .and_then(|element| element.get_inner_text()),
                }
                .map_err(|_| p::Error("browser text read failed".into()))?;
                let record =
                    self.artifacts
                        .write(&action.artifact_scope, "txt", text.as_bytes())?;
                (text, Some(record), p::EffectStatus::Observed)
            }
            p::BrowserOperation::Click { selector } => {
                let before = tab.get_url();
                let element = tab
                    .wait_for_element(selector)
                    .map_err(|_| p::Error("browser click failed".into()))?;
                let main_frame = tab
                    .call_method(Page::GetFrameTree(None))
                    .map_err(|_| p::Error("browser main frame could not be inspected".into()))?
                    .frame_tree
                    .frame
                    .id;
                let observation = Arc::new(ClickNavigationObservation::new(main_frame));
                let lifecycle_observation = Arc::clone(&observation);
                tab.add_event_listener(Arc::new(move |event: &Event| {
                    lifecycle_observation.observe(event);
                }))
                .map_err(|_| {
                    p::Error("browser click lifecycle observer could not be installed".into())
                })?;
                let response_observation = Arc::clone(&observation);
                tab.register_response_handling(
                    "forme-click-document-response",
                    Box::new(move |response, _fetch_body| {
                        if response.Type == Network::ResourceType::Document
                            && response_observation.is_main_frame(response.frame_id.as_ref())
                        {
                            response_observation.mark_finished();
                        }
                    }),
                )
                .map_err(|_| {
                    p::Error("browser click completion observer could not be installed".into())
                })?;
                element
                    .click()
                    .map_err(|_| p::Error("browser click failed".into()))?;
                let settlement = wait_for_click_settle(&tab, &before, &observation, self.timeout);
                (
                    match settlement {
                        ClickSettlement::TimedOut => {
                            "browser click outcome requires verification".to_owned()
                        }
                        _ => "browser click completed".to_owned(),
                    },
                    None,
                    match settlement {
                        ClickSettlement::TimedOut => p::EffectStatus::Unknown,
                        _ => p::EffectStatus::Committed,
                    },
                )
            }
            p::BrowserOperation::Type { selector, input } => {
                let input = resolve_external_input(input, secrets, self.contents.as_ref())?;
                let element = tab
                    .wait_for_element(selector)
                    .map_err(|_| p::Error("browser type action failed".into()))?;
                element
                    .type_into(&input)
                    .map_err(|_| p::Error("browser type action failed".into()))?;
                (
                    "browser type action completed".to_owned(),
                    None,
                    p::EffectStatus::Committed,
                )
            }
            p::BrowserOperation::Screenshot { full_page } => {
                let bytes = tab
                    .capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, *full_page)
                    .map_err(|_| p::Error("browser screenshot failed".into()))?;
                let record = self
                    .artifacts
                    .write(&action.artifact_scope, "png", &bytes)?;
                (
                    "browser screenshot captured".to_owned(),
                    Some(record),
                    p::EffectStatus::Observed,
                )
            }
        };
        Ok(DriverReceipt {
            schema_version: p::SchemaVersion(1),
            summary,
            content_ref: record.as_ref().map(|record| record.content_ref.clone()),
            digest: record.map(|record| record.digest),
            effect,
        })
    }
}

fn install_origin_guard(tab: &headless_chrome::Tab, origins: &[String]) -> p::Result<()> {
    let allowed = normalized_origins(origins)?;
    tab.enable_request_interception(Arc::new(
        move |_transport, _session, event: Fetch::events::RequestPausedEvent| {
            if request_url_is_allowed(&event.params.request.url, &allowed) {
                RequestPausedDecision::Continue(None)
            } else {
                RequestPausedDecision::Fail(Fetch::FailRequest {
                    request_id: event.params.request_id,
                    error_reason: Network::ErrorReason::BlockedByClient,
                })
            }
        },
    ))
    .map_err(|_| p::Error("browser origin guard could not be installed".into()))?;
    tab.enable_fetch(None, Some(false))
        .map_err(|_| p::Error("browser origin guard could not be enabled".into()))?;
    Ok(())
}

fn normalized_origins(origins: &[String]) -> p::Result<BTreeSet<String>> {
    if origins.is_empty() {
        return Err(p::Error("browser origin allowlist is empty".into()));
    }
    origins
        .iter()
        .map(|origin| {
            let url = Url::parse(origin)
                .map_err(|_| p::Error("browser allowlist origin cannot be parsed".into()))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(p::Error("browser allowlist entry is not an origin".into()));
            }
            Ok(url.origin().ascii_serialization())
        })
        .collect()
}

fn ensure_allowed_url(url: &str, origins: &[String]) -> p::Result<()> {
    let allowed = normalized_origins(origins)?;
    if request_url_is_allowed(url, &allowed) {
        Ok(())
    } else {
        Err(p::Error(
            "browser navigation left the approved origin set".into(),
        ))
    }
}

fn request_url_is_allowed(url: &str, origins: &BTreeSet<String>) -> bool {
    Url::parse(url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| origins.contains(&url.origin().ascii_serialization()))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClickSettlement {
    Waiting,
    Settled,
    NoNavigation,
    TimedOut,
}

struct ClickNavigationObservation {
    main_frame: Page::FrameId,
    started: AtomicBool,
    finished: AtomicBool,
}

impl ClickNavigationObservation {
    fn new(main_frame: Page::FrameId) -> Self {
        Self {
            main_frame,
            started: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }
    }

    fn observe(&self, event: &Event) {
        let Event::PageLifecycleEvent(event) = event else {
            return;
        };
        if event.params.frame_id != self.main_frame {
            return;
        }
        match event.params.name.as_ref() {
            "init" => {
                self.finished.store(false, Ordering::SeqCst);
                self.started.store(true, Ordering::SeqCst);
            }
            "load" | "networkAlmostIdle" if self.started.load(Ordering::SeqCst) => {
                self.mark_finished();
            }
            _ => {}
        }
    }

    fn mark_finished(&self) {
        self.finished.store(true, Ordering::SeqCst);
    }

    fn is_main_frame(&self, frame: Option<&Page::FrameId>) -> bool {
        frame == Some(&self.main_frame)
    }
}

fn classify_click_settlement(
    navigation_observed: bool,
    document_finished: bool,
    elapsed: Duration,
    quiet_limit: Duration,
    timeout: Duration,
) -> ClickSettlement {
    if navigation_observed && document_finished {
        ClickSettlement::Settled
    } else if navigation_observed && elapsed >= timeout {
        ClickSettlement::TimedOut
    } else if !navigation_observed && elapsed >= quiet_limit {
        ClickSettlement::NoNavigation
    } else {
        ClickSettlement::Waiting
    }
}

fn wait_for_click_settle(
    tab: &headless_chrome::Tab,
    before: &str,
    observation: &ClickNavigationObservation,
    timeout: p::DurationMs,
) -> ClickSettlement {
    let timeout = Duration::from_millis(timeout.0);
    let quiet_limit = timeout.min(Duration::from_millis(2_000));
    let started = Instant::now();
    loop {
        match classify_click_settlement(
            observation.started.load(Ordering::SeqCst) || tab.get_url() != before,
            observation.finished.load(Ordering::SeqCst),
            started.elapsed(),
            quiet_limit,
            timeout,
        ) {
            ClickSettlement::Settled => return ClickSettlement::Settled,
            ClickSettlement::NoNavigation => return ClickSettlement::NoNavigation,
            ClickSettlement::TimedOut => return ClickSettlement::TimedOut,
            ClickSettlement::Waiting => {}
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct IsolatedChrome {
    browser: Browser,
    child: Child,
    profile: PathBuf,
}

impl IsolatedChrome {
    fn launch(executable: &Path, timeout: p::DurationMs) -> p::Result<Self> {
        let profile = isolated_profile_directory()?;
        let mut command = Command::new(executable);
        command
            .env_clear()
            .envs(minimal_browser_environment())
            .arg("--headless=new")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let child = command
            .spawn()
            .map_err(|_| p::Error("headless browser could not be launched".into()))?;
        let endpoint =
            wait_for_devtools_endpoint(&profile, Duration::from_millis(timeout.0.min(30_000)));
        let endpoint = match endpoint {
            Ok(endpoint) => endpoint,
            Err(error) => {
                terminate_browser_process(child, &profile);
                return Err(error);
            }
        };
        let browser =
            match Browser::connect_with_timeout(endpoint, Duration::from_millis(timeout.0)) {
                Ok(browser) => browser,
                Err(_) => {
                    terminate_browser_process(child, &profile);
                    return Err(p::Error("headless browser connection failed".into()));
                }
            };
        Ok(Self {
            browser,
            child,
            profile,
        })
    }
}

impl Drop for IsolatedChrome {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

fn minimal_browser_environment() -> Vec<(String, String)> {
    #[cfg(windows)]
    const ALLOWED: &[&str] = &["SystemRoot", "TEMP", "TMP"];
    #[cfg(not(windows))]
    const ALLOWED: &[&str] = &["HOME", "LANG", "TMPDIR", "XDG_RUNTIME_DIR"];
    ALLOWED
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| ((*name).to_owned(), value))
        })
        .collect()
}

fn isolated_profile_directory() -> p::Result<PathBuf> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let profile = std::env::temp_dir().join(format!(
        "forme-browser-{}-{timestamp}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&profile)
        .map_err(|_| p::Error("browser profile directory cannot be created".into()))?;
    Ok(profile)
}

fn wait_for_devtools_endpoint(profile: &Path, timeout: Duration) -> p::Result<String> {
    let started = Instant::now();
    let marker = profile.join("DevToolsActivePort");
    while started.elapsed() < timeout {
        if let Ok(contents) = std::fs::read_to_string(&marker) {
            let mut lines = contents.lines();
            let port = lines.next().and_then(|value| value.parse::<u16>().ok());
            let path = lines
                .next()
                .filter(|value| value.starts_with("/devtools/browser/"));
            if let (Some(port), Some(path)) = (port, path) {
                return Ok(format!("ws://127.0.0.1:{port}{path}"));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(p::Error(
        "headless browser did not expose a debugging endpoint".into(),
    ))
}

fn terminate_browser_process(mut child: Child, profile: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(profile);
}

pub struct BrowserBackend {
    driver_id: p::ProviderId,
    driver: Arc<dyn BrowserDriver>,
    secrets: Arc<dyn SecretResolver>,
    budget: OutputBudget,
    timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl BrowserBackend {
    pub fn new(
        driver_id: p::ProviderId,
        driver: Arc<dyn BrowserDriver>,
        secrets: Arc<dyn SecretResolver>,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        if driver_id.0.trim().is_empty() || budget.max_bytes == 0 || timeout.0 == 0 {
            return Err(p::Error(
                "browser backend configuration is incomplete".into(),
            ));
        }
        Ok(Self {
            driver_id,
            driver,
            secrets,
            budget,
            timeout,
            active: ActiveCancellations::default(),
        })
    }
}

impl ActionBackend for BrowserBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Browser
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        let p::ActionParameters::Browser(spec) = &intent.parameters else {
            return Err(p::Error(
                "browser backend requires browser parameters".into(),
            ));
        };
        if spec.driver != self.driver_id || spec.schema_version.0 == 0 {
            return Err(p::Error("browser driver profile is not configured".into()));
        }
        plan_with(intent, self.budget.clone(), self.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        cancel: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, self.kind())?;
        let p::ActionParameters::Browser(spec) = &plan.intent.parameters else {
            return Err(p::Error("browser plan has incompatible parameters".into()));
        };
        if spec.driver != self.driver_id {
            return Err(p::Error("browser plan targets another driver".into()));
        }
        self.active
            .register(plan.intent.intent_id.clone(), cancel.clone())?;
        let result = (|| {
            emit_started(&plan, sink)?;
            let mut output = BoundedOutput::new(plan.budget.clone());
            if cancel.is_cancelled() {
                return cancelled(&plan, sink, &output);
            }
            let receipt = self.driver.perform(spec, self.secrets.as_ref());
            let receipt = match receipt {
                Ok(receipt) => receipt,
                Err(error) => return Err(failed(&plan, sink, error.to_string())),
            };
            if receipt.effect == p::EffectStatus::Unknown {
                receipt.validate_common()?;
            } else {
                receipt.validate()?;
            }
            output.push_with_ref(
                receipt.summary.as_bytes(),
                &plan,
                sink,
                receipt.content_ref.clone(),
            )?;
            if receipt.effect == p::EffectStatus::Unknown {
                return outcome_unknown(
                    &plan,
                    sink,
                    &output,
                    p::ProbeHintRef("inspect-browser-target-state".into()),
                );
            }
            complete_external(
                &plan,
                sink,
                &output,
                p::ExternalActionReceipt {
                    schema_version: p::SchemaVersion(1),
                    action: plan.intent.intent_id.clone(),
                    content_ref: receipt.content_ref,
                    content_digest: receipt.digest,
                    trust: p::TrustTier::Untrusted,
                    effect: receipt.effect,
                    probe_hint: None,
                },
            )
        })()
        .map_err(|error| fail_unless_terminal(&plan, sink, error));
        self.active.remove(&plan.intent.intent_id);
        result
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        self.active.cancel(&action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_settlement_never_treats_url_change_as_response_completion() {
        let quiet = Duration::from_secs(2);
        let timeout = Duration::from_secs(20);
        assert_eq!(
            classify_click_settlement(true, false, Duration::from_secs(2), quiet, timeout),
            ClickSettlement::Waiting
        );
        assert_eq!(
            classify_click_settlement(true, true, Duration::from_secs(2), quiet, timeout),
            ClickSettlement::Settled
        );
        assert_eq!(
            classify_click_settlement(false, true, Duration::from_secs(1), quiet, timeout),
            ClickSettlement::Waiting
        );
        assert_eq!(
            classify_click_settlement(true, false, timeout, quiet, timeout),
            ClickSettlement::TimedOut
        );
        assert_eq!(
            classify_click_settlement(false, false, quiet, quiet, timeout),
            ClickSettlement::NoNavigation
        );
    }

    #[test]
    fn origin_guard_rejects_credentials_paths_and_cross_origin_requests() {
        assert!(normalized_origins(&[]).is_err());
        assert!(normalized_origins(&["https://user@example.test".into()]).is_err());
        assert!(normalized_origins(&["https://example.test/path".into()]).is_err());

        let allowed = normalized_origins(&[
            "https://example.test".into(),
            "http://127.0.0.1:34001".into(),
        ])
        .unwrap();
        assert!(request_url_is_allowed(
            "https://example.test/task?q=1",
            &allowed
        ));
        assert!(request_url_is_allowed(
            "http://127.0.0.1:34001/mutate",
            &allowed
        ));
        assert!(!request_url_is_allowed(
            "https://redirected.example/task",
            &allowed
        ));
        assert!(!request_url_is_allowed("file:///tmp/secret", &allowed));
    }
}
