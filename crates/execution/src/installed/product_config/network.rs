//! Staged versus published Web exposure under the configuration writer lock.
use super::*;
use rustls_pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use zeroize::Zeroizing;

impl ProductConfigStore {
    pub(crate) fn network_is_managed(&self) -> bool {
        self.template.web_network_config_path.is_some()
    }

    pub(crate) fn network_validation_digest(
        &mut self,
        revision: u64,
        owner: &p::OwnerPrincipalRef,
    ) -> p::Result<p::SchemaDigest> {
        self.recover()?;
        self.check_revision(revision, owner, true)?;
        self.verify_published()?;
        validation_digest(&materialize::effective_network(
            &self.template,
            &self.state,
        )?)
    }

    /// Caller holds the product configuration lock and a durable maintenance
    /// stage intent. Publication never authorizes native side effects or retry.
    pub(crate) fn activate_network(
        &mut self,
        operation: String,
        revision: u64,
        owner: &p::OwnerPrincipalRef,
        validation_digest: &p::SchemaDigest,
    ) -> p::Result<()> {
        self.recover()?;
        self.check_revision(revision, owner, true)?;
        self.verify_published()?;
        let network = materialize::effective_network(&self.template, &self.state)?;
        if self.network_validation_digest(revision, owner)? != *validation_digest {
            return Err(p::Error(
                "LAN configuration validation is stale; refresh before applying".into(),
            ));
        }
        let digest = p::canonical_digest(&(
            "forme-product-network-activation-v1",
            &operation,
            revision,
            owner,
            validation_digest,
        ))?;
        if self.journal.existing(&operation, &digest)? {
            return Ok(());
        }
        let reference = materialize::stage_with_network(&self.template, &self.state, &network)?;
        self.journal.prepare(Intent {
            operation,
            digest,
            snapshot: reference,
        })?;
        self.recover()
    }

    pub(crate) fn active_network(&self) -> p::Result<p::ProductNetworkConfig> {
        self.verify_published()?;
        let Some(reference) = self.journal.current() else {
            return Ok(p::ProductNetworkConfig::loopback(self.template.web_bind));
        };
        if reference.web_network_digest.is_none() {
            return Ok(p::ProductNetworkConfig::loopback(self.template.web_bind));
        }
        serde_json::from_slice(&materialize::web_network_bytes(&self.template, reference)?)
            .map_err(|_| invalid())
    }
}

/// Certificate identity is part of the maintenance authorization. Do not
/// retain or expose private material, PEM parser errors or private paths.
pub(super) fn validation_digest(network: &p::ProductNetworkConfig) -> p::Result<p::SchemaDigest> {
    let config_digest = network.validation_digest()?;
    let Some(tls) = &network.tls else {
        return Ok(config_digest);
    };
    let invalid_material =
        || p::Error("Web TLS material must be bounded, valid and matched".into());
    let certificate_bytes =
        storage::read(&tls.certificate_path, 65_536).map_err(|_| invalid_material())?;
    let certificates = CertificateDer::pem_slice_iter(&certificate_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid_material())?;
    if certificates.is_empty() || certificates.len() > 8 {
        return Err(invalid_material());
    }
    let fingerprint = p::canonical_digest(
        &certificates
            .iter()
            .map(|cert| p::sha256_content_digest(cert.as_ref()))
            .collect::<Vec<_>>(),
    )?;
    let key_bytes = Zeroizing::new(
        storage::read(&tls.private_key_path, 65_536).map_err(|_| invalid_material())?,
    );
    let key = PrivateKeyDer::from_pem_slice(&key_bytes).map_err(|_| invalid_material())?;
    rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|_| invalid_material())?
    .with_no_client_auth()
    .with_single_cert(certificates, key)
    .map_err(|_| invalid_material())?;
    p::canonical_digest(&(
        "forme-product-network-material-validation-v1",
        config_digest,
        fingerprint,
    ))
}
