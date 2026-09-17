use std::sync::{Arc, Mutex};

use forme_protocol as p;

type Observer = Arc<dyn Fn(&p::EventPayload) -> p::Result<()> + Send + Sync>;

#[derive(Default)]
pub struct EventSink {
    events: Mutex<Vec<p::EventPayload>>,
    observer: Option<Observer>,
}

impl EventSink {
    pub fn with_observer<F>(observer: F) -> Self
    where
        F: Fn(&p::EventPayload) -> p::Result<()> + Send + Sync + 'static,
    {
        Self {
            events: Mutex::new(Vec::new()),
            observer: Some(Arc::new(observer)),
        }
    }

    pub fn emit(&self, event: p::EventPayload) -> p::Result<()> {
        if let Some(observer) = &self.observer {
            observer(&event)?;
        }
        self.events
            .lock()
            .map_err(|_| p::Error("execution event sink is unavailable".into()))?
            .push(event);
        Ok(())
    }

    pub fn events(&self) -> Vec<p::EventPayload> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub fn take(&self) -> Vec<p::EventPayload> {
        self.events
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }
}
