use std::sync::{Arc, Mutex};

use forme_protocol as p;

use crate::support::{cancelled, complete, emit_started, ActiveCancellations, BoundedOutput};
use crate::{
    plan_with, validate_plan_for, ActionBackend, ActionResult, BackendKind, CancelToken, EventSink,
    ExecutionPlan, OutputBudget,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNotification {
    pub schema_version: p::SchemaVersion,
    pub surface: p::SurfaceRef,
    pub target: p::ParticipantId,
    pub title: String,
    pub body_ref: p::ContentRef,
    pub plan_digest: p::PlanDigest,
}

pub trait LocalNotificationSink: Send + Sync {
    fn deliver(&self, notification: LocalNotification) -> p::Result<()>;
}

#[derive(Debug, Default)]
pub struct ConsoleNotificationSink;

impl LocalNotificationSink for ConsoleNotificationSink {
    fn deliver(&self, notification: LocalNotification) -> p::Result<()> {
        eprintln!(
            "forme notification: {} [{}]",
            notification.title, notification.body_ref.0
        );
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryNotificationSink {
    delivered: Mutex<Vec<LocalNotification>>,
}

impl InMemoryNotificationSink {
    pub fn delivered(&self) -> Vec<LocalNotification> {
        self.delivered
            .lock()
            .map(|items| items.clone())
            .unwrap_or_default()
    }
}

impl LocalNotificationSink for InMemoryNotificationSink {
    fn deliver(&self, notification: LocalNotification) -> p::Result<()> {
        self.delivered
            .lock()
            .map_err(|_| p::Error("local notification sink is unavailable".into()))?
            .push(notification);
        Ok(())
    }
}

pub struct NotificationBackend {
    sink: Arc<dyn LocalNotificationSink>,
    budget: OutputBudget,
    timeout: p::DurationMs,
    active: ActiveCancellations,
}

impl Default for NotificationBackend {
    fn default() -> Self {
        Self::new(
            Arc::new(ConsoleNotificationSink),
            OutputBudget::truncate_at(512),
            p::DurationMs(5_000),
        )
    }
}

impl NotificationBackend {
    pub fn new(
        sink: Arc<dyn LocalNotificationSink>,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> Self {
        Self {
            sink,
            budget,
            timeout,
            active: ActiveCancellations::default(),
        }
    }
}

impl ActionBackend for NotificationBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Notification
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        validate_notification_intent(intent)?;
        plan_with(intent, self.budget.clone(), self.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        cancel: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, self.kind())?;
        validate_notification_intent(&plan.intent)?;
        self.active
            .register(plan.intent.intent_id.clone(), cancel.clone())?;
        let result = (|| {
            emit_started(&plan, sink)?;
            let mut output = BoundedOutput::new(plan.budget.clone());
            if cancel.is_cancelled() {
                return cancelled(&plan, sink, &output);
            }
            let p::ActionParameters::Notification {
                surface,
                target,
                title,
                body_ref,
            } = &plan.intent.parameters
            else {
                return Err(p::Error(
                    "notification plan has incompatible parameters".into(),
                ));
            };
            self.sink.deliver(LocalNotification {
                schema_version: p::SchemaVersion(1),
                surface: surface.clone(),
                target: target.clone(),
                title: title.clone(),
                body_ref: body_ref.clone(),
                plan_digest: plan.digest.clone(),
            })?;
            output.push(
                format!("local notification delivered by ref {}", body_ref.0).as_bytes(),
                &plan,
                sink,
            )?;
            complete(&plan, sink, &output, None, None)
        })();
        self.active.remove(&plan.intent.intent_id);
        result
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        self.active.cancel(&action)
    }
}

fn validate_notification_intent(intent: &p::ActionIntent) -> p::Result<()> {
    let p::ActionParameters::Notification {
        surface,
        target,
        title,
        body_ref,
    } = &intent.parameters
    else {
        return Err(p::Error(
            "notification backend requires notification parameters".into(),
        ));
    };
    if intent.schema_version.0 == 0
        || intent.backend_hint != p::BackendKind::Notification
        || intent.action_type != p::ActionType::Deliver
        || intent.expected_effect != p::ExpectedEffect::Outward
        || !surface.0.starts_with("surface:local")
        || target.0.trim().is_empty()
        || title.trim().is_empty()
        || title.len() > 120
        || title.chars().any(char::is_control)
        || body_ref.0.trim().is_empty()
    {
        return Err(p::Error(
            "notification intent must target a bounded local surface with a safe summary ref"
                .into(),
        ));
    }
    Ok(())
}
