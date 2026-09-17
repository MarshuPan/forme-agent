use std::collections::BTreeMap;
use std::sync::RwLock;

use forme_protocol as p;

pub struct ResolvedSecret(String);

impl ResolvedSecret {
    pub fn new(value: impl Into<String>) -> p::Result<Self> {
        let value = value.into();
        if value.is_empty() {
            return Err(p::Error("resolved secret is empty".into()));
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for ResolvedSecret {
    fn drop(&mut self) {
        self.0.clear();
    }
}

pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &p::SecretRef) -> p::Result<ResolvedSecret>;
}

pub trait ContentResolver: Send + Sync {
    fn resolve(&self, reference: &p::ContentRef) -> p::Result<Vec<u8>>;
}

#[derive(Default)]
pub struct RejectingSecretResolver;

impl SecretResolver for RejectingSecretResolver {
    fn resolve(&self, _reference: &p::SecretRef) -> p::Result<ResolvedSecret> {
        Err(p::Error("secret resolution is not configured".into()))
    }
}

#[derive(Default)]
pub struct RejectingContentResolver;

impl ContentResolver for RejectingContentResolver {
    fn resolve(&self, _reference: &p::ContentRef) -> p::Result<Vec<u8>> {
        Err(p::Error("content resolution is not configured".into()))
    }
}

#[derive(Default)]
pub struct InMemorySecretResolver {
    values: RwLock<BTreeMap<p::SecretRef, String>>,
}

impl InMemorySecretResolver {
    pub fn insert(&self, reference: p::SecretRef, value: impl Into<String>) -> p::Result<()> {
        if reference.0.trim().is_empty() {
            return Err(p::Error("secret reference is empty".into()));
        }
        let value = value.into();
        if value.is_empty() {
            return Err(p::Error("secret value is empty".into()));
        }
        self.values
            .write()
            .map_err(|_| p::Error("secret resolver is unavailable".into()))?
            .insert(reference, value);
        Ok(())
    }
}

impl SecretResolver for InMemorySecretResolver {
    fn resolve(&self, reference: &p::SecretRef) -> p::Result<ResolvedSecret> {
        let value = self
            .values
            .read()
            .map_err(|_| p::Error("secret resolver is unavailable".into()))?
            .get(reference)
            .cloned()
            .ok_or_else(|| p::Error("secret reference is unresolved".into()))?;
        ResolvedSecret::new(value)
    }
}

#[derive(Default)]
pub struct InMemoryContentResolver {
    values: RwLock<BTreeMap<p::ContentRef, Vec<u8>>>,
}

impl InMemoryContentResolver {
    pub fn insert(&self, reference: p::ContentRef, value: Vec<u8>) -> p::Result<()> {
        if reference.0.trim().is_empty() || value.is_empty() {
            return Err(p::Error("content reference or value is empty".into()));
        }
        let mut values = self
            .values
            .write()
            .map_err(|_| p::Error("content resolver is unavailable".into()))?;
        if values.contains_key(&reference) {
            return Err(p::Error(
                "content reference is immutable once registered".into(),
            ));
        }
        values.insert(reference, value);
        Ok(())
    }
}

impl ContentResolver for InMemoryContentResolver {
    fn resolve(&self, reference: &p::ContentRef) -> p::Result<Vec<u8>> {
        self.values
            .read()
            .map_err(|_| p::Error("content resolver is unavailable".into()))?
            .get(reference)
            .cloned()
            .ok_or_else(|| p::Error("content reference is unresolved".into()))
    }
}

pub(crate) fn resolve_external_input(
    input: &p::ExternalInput,
    secrets: &dyn SecretResolver,
    contents: &dyn ContentResolver,
) -> p::Result<String> {
    match input {
        p::ExternalInput::Literal(value) => Ok(value.clone()),
        p::ExternalInput::Content(reference) => String::from_utf8(contents.resolve(reference)?)
            .map_err(|_| p::Error("resolved content is not UTF-8 text".into())),
        p::ExternalInput::Secret(reference) => Ok(secrets.resolve(reference)?.expose().to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverReceipt {
    pub schema_version: p::SchemaVersion,
    pub summary: String,
    pub content_ref: Option<p::ContentRef>,
    pub digest: Option<p::SchemaDigest>,
    pub effect: p::EffectStatus,
}

impl DriverReceipt {
    pub(crate) fn validate(&self) -> p::Result<()> {
        self.validate_common()?;
        if self.effect == p::EffectStatus::Unknown {
            return Err(p::Error("external driver receipt is incomplete".into()));
        }
        Ok(())
    }

    pub(crate) fn validate_common(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.summary.is_empty()
            || self.summary.len() > 1_048_576
            || self.content_ref.is_some() != self.digest.is_some()
        {
            return Err(p::Error("external driver receipt is incomplete".into()));
        }
        Ok(())
    }
}
