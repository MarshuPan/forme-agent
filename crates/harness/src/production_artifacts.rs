//! Context hydration for owner-uploaded, session-scoped workbench artifacts.

use std::sync::Arc;

use forme_context::ContextSlice;
use forme_protocol as p;
use forme_store::SqliteEventStore;

pub(crate) fn workbench_artifact_slices(
    store: &Arc<SqliteEventStore>,
    owner: &p::OwnerPrincipalRef,
    session: &p::SessionId,
) -> p::Result<Vec<ContextSlice>> {
    let Some(protection) = store.content_protection()? else {
        return Ok(Vec::new());
    };
    let principal = p::VerifiedPrincipal(owner.0.clone());
    let scope = p::Scope(format!("workbench-artifact:{}", session.0));
    let lifecycle = protection.lifecycle_service(store.as_ref().clone());
    let mut slices = Vec::new();
    for content_ref in store.protected_content_refs(&principal, &scope)? {
        let Some(record) = lifecycle.protected_content(&principal, &content_ref)? else {
            continue;
        };
        let bytes = protection
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        let text = String::from_utf8(bytes).unwrap_or_else(|_| {
            "binary content is retained as an authorized artifact; a compatible driver is required to inspect it".into()
        });
        slices.push(ContextSlice {
            schema_version: p::SchemaVersion(1),
            id: p::ContextSliceRef(format!("attachment:{}", content_ref.0)),
            content: format!(
                "owner-uploaded session artifact {}:\n{}",
                content_ref.0, text
            ),
            provenance: p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        });
    }
    Ok(slices)
}
