//! Gateway evaluation boundary; public facade remains in the crate root.
use super::*;

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessManualEvaluator + Send + Sync,
{
    pub fn run_eval(
        &self,
        case: p::ManualEvalCase,
        profile: p::EvalProfile,
        auth: AuthContext,
    ) -> p::Result<p::ManualEvalReport> {
        self.validate_owner_control(auth)?;
        HarnessManualEvaluator::run_case(self.harness.as_ref(), case, profile)
    }

    pub fn export_eval(
        &self,
        eval_ref: p::EvalRef,
        auth: AuthContext,
    ) -> p::Result<p::ManualEvalReport> {
        self.validate_owner_control(auth)?;
        HarnessManualEvaluator::export_report(self.harness.as_ref(), eval_ref)
    }
}
