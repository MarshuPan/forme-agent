use std::sync::Arc;

use forme_protocol as p;

use crate::support::{
    cancelled, complete_external, emit_started, fail_unless_terminal, failed, ActiveCancellations,
    BoundedOutput,
};
use crate::{
    plan_with, validate_plan_for, ActionBackend, ActionResult, ArtifactStore, BackendKind,
    CancelToken, ContentResolver, DriverReceipt, EventSink, ExecutionPlan, OutputBudget,
    SecretResolver,
};

pub trait ComputerDriver: Send + Sync {
    fn perform(
        &self,
        action: &p::ComputerActionSpec,
        secrets: &dyn SecretResolver,
    ) -> p::Result<DriverReceipt>;
}

pub struct ComputerBackend {
    driver_id: p::ProviderId,
    driver: Arc<dyn ComputerDriver>,
    secrets: Arc<dyn SecretResolver>,
    budget: OutputBudget,
    timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl ComputerBackend {
    pub fn new(
        driver_id: p::ProviderId,
        driver: Arc<dyn ComputerDriver>,
        secrets: Arc<dyn SecretResolver>,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        if driver_id.0.trim().is_empty() || budget.max_bytes == 0 || timeout.0 == 0 {
            return Err(p::Error(
                "computer backend configuration is incomplete".into(),
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

impl ActionBackend for ComputerBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Computer
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        let p::ActionParameters::Computer(spec) = &intent.parameters else {
            return Err(p::Error(
                "computer backend requires computer parameters".into(),
            ));
        };
        spec.bounds.validate()?;
        if spec.driver != self.driver_id || spec.schema_version.0 == 0 {
            return Err(p::Error("computer driver profile is not configured".into()));
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
        let p::ActionParameters::Computer(spec) = &plan.intent.parameters else {
            return Err(p::Error("computer plan has incompatible parameters".into()));
        };
        spec.bounds.validate()?;
        if spec.driver != self.driver_id || !computer_operation_within_bounds(spec) {
            return Err(p::Error(
                "computer plan exceeds its configured driver or bounds".into(),
            ));
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
            receipt.validate()?;
            output.push_with_ref(
                receipt.summary.as_bytes(),
                &plan,
                sink,
                receipt.content_ref.clone(),
            )?;
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

pub(crate) fn computer_operation_within_bounds(spec: &p::ComputerActionSpec) -> bool {
    match spec.operation {
        p::ComputerOperation::Move { x, y } | p::ComputerOperation::Click { x, y, .. } => {
            spec.bounds.contains(x, y)
        }
        _ => true,
    }
}

#[cfg(target_os = "windows")]
#[path = "computer_surface.rs"]
mod surface;

#[cfg(target_os = "windows")]
mod native {
    use std::sync::Mutex;

    use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};

    use super::*;
    use crate::resolve_external_input;

    pub struct NativeComputerDriver {
        enigo: Mutex<Enigo>,
        contents: Arc<dyn ContentResolver>,
        artifacts: Arc<dyn ArtifactStore>,
    }

    impl NativeComputerDriver {
        pub fn new(
            contents: Arc<dyn ContentResolver>,
            artifacts: Arc<dyn ArtifactStore>,
        ) -> p::Result<Self> {
            let enigo = Enigo::new(&Settings {
                open_prompt_to_get_permissions: false,
                ..Settings::default()
            })
            .map_err(|_| p::Error("native computer input driver is unavailable".into()))?;
            Ok(Self {
                enigo: Mutex::new(enigo),
                contents,
                artifacts,
            })
        }
    }

    impl ComputerDriver for NativeComputerDriver {
        fn perform(
            &self,
            action: &p::ComputerActionSpec,
            secrets: &dyn SecretResolver,
        ) -> p::Result<DriverReceipt> {
            let mut enigo = self
                .enigo
                .lock()
                .map_err(|_| p::Error("native computer input driver is unavailable".into()))?;
            let (summary, record, effect) = match &action.operation {
                p::ComputerOperation::Move { x, y } => {
                    enigo
                        .move_mouse(*x, *y, Coordinate::Abs)
                        .map_err(|_| p::Error("native pointer move failed".into()))?;
                    (
                        "computer pointer moved".into(),
                        None,
                        p::EffectStatus::Committed,
                    )
                }
                p::ComputerOperation::Click { x, y, button } => {
                    enigo
                        .move_mouse(*x, *y, Coordinate::Abs)
                        .and_then(|()| enigo.button(pointer_button(*button), Direction::Click))
                        .map_err(|_| p::Error("native pointer click failed".into()))?;
                    (
                        "computer pointer clicked".into(),
                        None,
                        p::EffectStatus::Committed,
                    )
                }
                p::ComputerOperation::Type { input } => {
                    let input = resolve_external_input(input, secrets, self.contents.as_ref())?;
                    enigo
                        .text(&input)
                        .map_err(|_| p::Error("native text input failed".into()))?;
                    (
                        "computer text entered".into(),
                        None,
                        p::EffectStatus::Committed,
                    )
                }
                p::ComputerOperation::Key { key } => {
                    enigo
                        .key(key_code(*key), Direction::Click)
                        .map_err(|_| p::Error("native key input failed".into()))?;
                    (
                        "computer key entered".into(),
                        None,
                        p::EffectStatus::Committed,
                    )
                }
                p::ComputerOperation::Scroll { dx, dy } => {
                    if *dx != 0 {
                        enigo
                            .scroll(*dx, Axis::Horizontal)
                            .map_err(|_| p::Error("native horizontal scroll failed".into()))?;
                    }
                    if *dy != 0 {
                        enigo
                            .scroll(*dy, Axis::Vertical)
                            .map_err(|_| p::Error("native vertical scroll failed".into()))?;
                    }
                    (
                        "computer scroll completed".into(),
                        None,
                        p::EffectStatus::Committed,
                    )
                }
                p::ComputerOperation::Screenshot => {
                    let image = capture_surface(&action.surface, action.bounds)?;
                    let mut bytes = std::io::Cursor::new(Vec::new());
                    image
                        .write_to(&mut bytes, xcap::image::ImageFormat::Png)
                        .map_err(|_| p::Error("computer screenshot encoding failed".into()))?;
                    let record =
                        self.artifacts
                            .write(&action.artifact_scope, "png", bytes.get_ref())?;
                    (
                        "computer screenshot captured".into(),
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

    fn capture_surface(
        surface: &p::SurfaceRef,
        bounds: p::CoordinateBounds,
    ) -> p::Result<xcap::image::RgbaImage> {
        use super::surface::{resolve_capture_region, MonitorGeometry};
        let monitors = xcap::Monitor::all()
            .map_err(|_| p::Error("native monitors cannot be enumerated".into()))?;
        let geometry = monitors
            .iter()
            .map(|monitor| {
                let unavailable = || p::Error("native monitor geometry is unavailable".into());
                Ok(MonitorGeometry {
                    id: monitor.id().map_err(|_| unavailable())?,
                    x: monitor.x().map_err(|_| unavailable())?,
                    y: monitor.y().map_err(|_| unavailable())?,
                    width: monitor.width().map_err(|_| unavailable())?,
                    height: monitor.height().map_err(|_| unavailable())?,
                })
            })
            .collect::<p::Result<Vec<_>>>()?;
        let region = resolve_capture_region(surface, bounds, &geometry)?;
        let monitor = monitors
            .get(region.monitor_index)
            .ok_or_else(|| p::Error("native monitor surface is unavailable".into()))?;
        monitor
            .capture_region(region.x, region.y, region.width, region.height)
            .map_err(|_| p::Error("native monitor capture failed".into()))
    }

    fn pointer_button(button: p::PointerButton) -> Button {
        match button {
            p::PointerButton::Primary => Button::Left,
            p::PointerButton::Secondary => Button::Right,
            p::PointerButton::Middle => Button::Middle,
        }
    }

    fn key_code(key: p::KeyCode) -> Key {
        match key {
            p::KeyCode::Enter => Key::Return,
            p::KeyCode::Escape => Key::Escape,
            p::KeyCode::Tab => Key::Tab,
            p::KeyCode::Backspace => Key::Backspace,
            p::KeyCode::Delete => Key::Delete,
            p::KeyCode::ArrowUp => Key::UpArrow,
            p::KeyCode::ArrowDown => Key::DownArrow,
            p::KeyCode::ArrowLeft => Key::LeftArrow,
            p::KeyCode::ArrowRight => Key::RightArrow,
        }
    }
}

#[cfg(target_os = "windows")]
pub use native::NativeComputerDriver;
