#![forbid(unsafe_code)]

use forme_protocol as p;

use super::{HostAuth, HostCredentialDomain, NativeServiceManager, ProductHostBroker};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    fn verify_web_configuration_auth(&self, auth: &HostAuth) -> p::Result<()> {
        if auth.domain != HostCredentialDomain::OwnerMaintenance
            || auth.actor != p::MaintenanceActorClass::OwnerReauthenticated
            || self.web_configuration_digest.as_ref() != Some(&auth.proof_ref)
        {
            return Err(p::Error(
                "host configuration authorization is invalid".into(),
            ));
        }
        Ok(())
    }

    pub fn configuration(&self, auth: &HostAuth) -> p::Result<p::ProductConfigView> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        let mut store = store
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
        store.snapshot()
    }

    pub fn apply_configuration(
        &self,
        auth: &HostAuth,
        patch: p::ProductConfigPatch,
    ) -> p::Result<p::ProductConfigResponse> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        store
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?
            .apply(patch)
    }

    pub fn bind_configuration_owner(
        &self,
        auth: &HostAuth,
        binding: p::ProductConfigOwnerBinding,
    ) -> p::Result<p::ProductConfigResponse> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        store
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?
            .bind_owner(binding)
    }

    pub fn authenticate_web_configuration(&self, bearer: &str) -> p::Result<HostAuth> {
        let Some(expected_bearer) = self.web_configuration_bearer.as_deref() else {
            return Err(p::Error(
                "Web configuration authentication is not configured".into(),
            ));
        };
        let expected = expected_bearer.as_bytes();
        let actual = bearer.as_bytes();
        if expected.len() != actual.len()
            || expected
                .iter()
                .zip(actual)
                .fold(0_u8, |difference, (left, right)| {
                    difference | (left ^ right)
                })
                != 0
        {
            return Err(p::Error(
                "Web configuration authorization is invalid".into(),
            ));
        }
        Ok(HostAuth {
            proof_ref: self
                .web_configuration_digest
                .clone()
                .expect("validated Web configuration digest"),
            actor: p::MaintenanceActorClass::OwnerReauthenticated,
            domain: HostCredentialDomain::OwnerMaintenance,
        })
    }
}
