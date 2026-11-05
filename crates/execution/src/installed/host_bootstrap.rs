//! First data adoption holds config -> claim locks and cannot start a service.
use super::{NativeServiceManager, ProductHostBroker};
use crate::installed::bootstrap::{
    BootstrapAuthority, BootstrapError, BootstrapResult, BootstrapServiceAuth,
};
use forme_protocol::{self as p, BootstrapProblemCode as Code};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn adopt_bootstrap_data(
        &self,
        authority: &BootstrapAuthority,
        auth: &BootstrapServiceAuth,
        request: p::BootstrapClaimRequestV2,
        now: i64,
    ) -> BootstrapResult<p::SignedBootstrapClaimActivationV2> {
        let started = std::time::Instant::now();
        let store = self
            .product_config
            .as_ref()
            .ok_or(BootstrapError(Code::Unavailable))?;
        let mut config = store
            .lock()
            .map_err(|_| BootstrapError(Code::Unavailable))?;
        if self
            .journal
            .operations(0)
            .map_err(|_| BootstrapError(Code::RecoveryRequired))?
            .iter()
            .any(|op| !op.stage.is_terminal())
        {
            return Err(BootstrapError(Code::RecoveryRequired));
        }
        let admitted_at = now
            .checked_add(
                i64::try_from(started.elapsed().as_millis())
                    .map_err(|_| BootstrapError(Code::InvalidInput))?,
            )
            .ok_or(BootstrapError(Code::InvalidInput))?;
        authority.adopt_data(auth, request, admitted_at, &mut config)
    }
}
