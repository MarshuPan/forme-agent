//! Gateway scheduled boundary; public facade remains in the crate root.
use super::*;

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessSchedulerGatewayControl + HarnessSchedulerService + Send + Sync,
{
    pub fn schedule_job(
        &self,
        command: p::ScheduleCommand,
        auth: AuthContext,
    ) -> p::Result<p::IntentionId> {
        self.validate_owner_control(auth)?;
        HarnessSchedulerGatewayControl::schedule(self.harness.as_ref(), command)
    }

    pub fn list_jobs(&self, auth: AuthContext) -> p::Result<Vec<p::ScheduledJob>> {
        self.validate_owner_control(auth)?;
        HarnessSchedulerGatewayControl::list_jobs(self.harness.as_ref())
    }

    pub fn cancel_job(&self, intention: p::IntentionId, auth: AuthContext) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        HarnessSchedulerService::cancel(self.harness.as_ref(), intention, p::Actor::Owner)
    }
}
