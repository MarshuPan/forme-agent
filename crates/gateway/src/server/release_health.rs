//! Internal live proof from the running managed runtime, independent of work admission.
use super::*;
use ed25519_dalek::{Signature, Signer, SigningKey};
use zeroize::Zeroizing;

pub(super) struct ReleaseHealth {
    readiness: p::ProductReleaseReadiness,
    key: SigningKey,
    rate: Mutex<RateWindow>,
}

impl ReleaseHealth {
    pub(super) fn new(
        readiness: p::ProductReleaseReadiness,
        credential: &LocalBearerSecret,
    ) -> p::Result<Self> {
        readiness.validate()?;
        let seed = Zeroizing::new(p::product_release_probe_seed(credential.0.as_bytes())?);
        Ok(Self {
            readiness,
            key: SigningKey::from_bytes(&seed),
            rate: Mutex::new(RateWindow::new()),
        })
    }

    fn prove(
        &self,
        request: &p::ProductReleaseReadinessProbe,
    ) -> p::Result<p::ProductReleaseReadinessProof> {
        request.validate()?;
        if request.install_ref != self.readiness.binding.install_ref {
            return Err(invalid());
        }
        self.key
            .verifying_key()
            .verify_strict(
                request.signing_digest()?.0.as_bytes(),
                &Signature::from_slice(&request.signature).map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())?;
        let mut proof = p::ProductReleaseReadinessProof {
            schema_version: p::SchemaVersion(1),
            nonce: request.nonce.clone(),
            readiness: self.readiness.clone(),
            signature: Vec::new(),
        };
        proof.signature = self
            .key
            .sign(proof.signing_digest()?.0.as_bytes())
            .to_bytes()
            .to_vec();
        proof.validate_for(request)?;
        Ok(proof)
    }
}

pub(super) async fn probe(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(request): Json<p::ProductReleaseReadinessProbe>,
) -> Response {
    let Some(health) = &state.release_health else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    // A browser credential is never an alternative to the signed challenge.
    if headers.contains_key(header::ORIGIN)
        || headers.contains_key(header::AUTHORIZATION)
        || headers.get(header::HOST).and_then(|h| h.to_str().ok())
            != state.origin.strip_prefix("http://")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !health.rate.lock().is_ok_and(|mut rate| rate.admit(30)) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    match health.prove(&request) {
        Ok(proof) => ([(header::CACHE_CONTROL, "no-store")], Json(proof)).into_response(),
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}

fn invalid() -> p::Error {
    p::Error("live installed release proof is invalid".into())
}

#[cfg(test)]
mod tests;
