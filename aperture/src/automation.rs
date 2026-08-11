//! Event-driven task automation: reacts to OS events by spawning tasks.

use std::error::Error as StdError;

use aperture_http::{RegenerateCertificateDefinition, RegenerateCertificateInput};
use aperture_runtime::{Stop, Worker};
use aperture_storage::ActorId;
use aperture_tasks::Tasks;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

use aperture_os::OsEvent;

pub struct AutomationWorker {
    events: Receiver<OsEvent>,
    tasks: Tasks,
}

impl AutomationWorker {
    pub(crate) const fn new(events: Receiver<OsEvent>, tasks: Tasks) -> Self {
        Self { events, tasks }
    }
}

impl Worker for AutomationWorker {
    async fn run(mut self, stop: Stop) {
        loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                recv = self.events.recv() => {
                    match recv {
                        Ok(event) => self.handle_event(event).await,
                        Err(RecvError::Lagged(n)) => {
                            tracing::warn!(skipped = n, "os event feed lagged");
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        }
    }
}

impl AutomationWorker {
    async fn handle_event(&self, event: OsEvent) {
        match event {
            OsEvent::HostnameApplied(hostname) => {
                let name = hostname.as_str().to_owned();
                if let Err(err) = self
                    .tasks
                    .spawn::<RegenerateCertificateDefinition>(
                        RegenerateCertificateInput {
                            hostname: Some(name),
                        },
                        ActorId::SYSTEM,
                    )
                    .await
                {
                    tracing::warn!(
                        error = &err as &dyn StdError,
                        "failed to trigger certificate regeneration"
                    );
                }
            }
        }
    }
}
