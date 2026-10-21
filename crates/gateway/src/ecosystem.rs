//! Gateway ecosystem boundary; public facade remains in the crate root.
use super::*;

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + HarnessEcosystemGatewayControl + Send + Sync,
{
    pub fn ecosystem_snapshot(
        &self,
        scope: p::Scope,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityEcosystemSnapshot> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::capability_ecosystem_snapshot(self.harness.as_ref(), scope)
    }

    pub fn provision_capability_publisher(
        &self,
        run: p::RunId,
        grant: p::CapabilityPublisherGrant,
        previous: Option<p::CapabilityPublisherGrantRef>,
        expected: p::EcosystemAggregateVersion,
        auth: AuthContext,
    ) -> p::Result<p::ExpectedAppend> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::provision_capability_publisher(
            self.harness.as_ref(),
            run,
            grant,
            previous,
            expected,
            self.owner.clone(),
        )
    }

    pub fn admit_capability_package(
        &self,
        run: p::RunId,
        catalog_run: p::RunId,
        package: p::SignedCapabilityPackage,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityPackageAdmission> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::admit_capability_package(
            self.harness.as_ref(),
            run,
            catalog_run,
            package,
            now_ms(),
            self.owner.clone(),
        )
    }

    pub fn prepare_capability_change(
        &self,
        operation: p::CapabilityPackageOperation,
        package: p::CapabilityPackageRef,
        release: p::CapabilityReleaseRef,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityInstallPlan> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::prepare_capability_change(
            self.harness.as_ref(),
            operation,
            package,
            release,
            self.owner.clone(),
        )
    }

    pub fn apply_capability_change(
        &self,
        run: p::RunId,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
        reason: p::ReasonRef,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityPackageState> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::apply_capability_change(
            self.harness.as_ref(),
            run,
            plan,
            approval,
            reason,
            now_ms(),
        )
    }

    pub fn prepare_capability_distribution(
        &self,
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::prepare_capability_distribution(
            self.harness.as_ref(),
            plan,
            target_peer,
            authority_epoch,
            self.owner.clone(),
            now_ms(),
        )
    }
}
