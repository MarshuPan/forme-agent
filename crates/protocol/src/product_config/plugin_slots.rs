//! Declarative references to existing managed bindings, with original hook semantics.
use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginToolSlot {
    pub schema_version: SchemaVersion,
    pub id: crate::ToolRef,
    pub binding: crate::ToolRef,
    pub hooks: Vec<ProductPluginHookSlot>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginHookSlot {
    pub schema_version: SchemaVersion,
    pub id: crate::HookRef,
    pub point: ProductPluginHookPoint,
    pub transform: ProductPluginHookTransform,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductPluginHookPoint {
    BeforePolicy,
    BeforeExecution,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductPluginHookTransform {
    Identity,
    ReplaceCapability { capability: crate::CapabilityRef },
    ReplaceShellProgram { program: String },
}
impl ProductPluginToolSlot {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || !self.id.0.starts_with("plugin-tool:")
            || self.binding.0.starts_with("plugin-tool:")
            || self.hooks.len() > 8
        {
            return Err(invalid());
        }
        validate_product_id(&self.id.0)?;
        validate_product_id(&self.binding.0)?;
        let mut ids = std::collections::BTreeSet::new();
        for hook in &self.hooks {
            if hook.schema_version != SchemaVersion(1)
                || !hook.id.0.starts_with("plugin-hook:")
                || !ids.insert(&hook.id)
            {
                return Err(invalid());
            }
            validate_product_id(&hook.id.0)?;
            match &hook.transform {
                ProductPluginHookTransform::Identity => (),
                ProductPluginHookTransform::ReplaceCapability { capability } => {
                    validate_product_id(&capability.0)?
                }
                ProductPluginHookTransform::ReplaceShellProgram { program } => {
                    if program.trim().is_empty()
                        || program.len() > 1024
                        || program.chars().any(char::is_control)
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        Ok(())
    }
}
fn invalid() -> Error {
    Error("declarative plugin slot is invalid".into())
}
