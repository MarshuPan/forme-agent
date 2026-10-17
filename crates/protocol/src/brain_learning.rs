//! Scoped goal dependencies and traceable learning, using existing events.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LearningTarget {
    UserAttribute(UserAttributeRef),
    CognitiveMap(ObjectRef),
}

impl LearningTarget {
    pub fn key(&self) -> &str {
        match self {
            Self::UserAttribute(key) => &key.0,
            Self::CognitiveMap(key) => &key.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRequirement {
    pub schema_version: SchemaVersion,
    pub target: LearningTarget,
    pub reason: ReasonRef,
    pub impact: Impact,
    pub minimum_confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalKnowledge {
    pub schema_version: SchemaVersion,
    pub scope: Scope,
    pub allow_questions: bool,
    pub requirements: Vec<KnowledgeRequirement>,
}

impl GoalKnowledge {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.scope.0.trim().is_empty()
            || self.requirements.is_empty()
            || self.requirements.len() > 32
        {
            return Err(Error("goal knowledge requirements are invalid".into()));
        }
        let mut targets = std::collections::BTreeSet::new();
        for requirement in &self.requirements {
            if requirement.schema_version != SchemaVersion(1)
                || requirement.target.key().trim().is_empty()
                || requirement.target.key().len() > 512
                || requirement.reason.0.trim().is_empty()
                || requirement.reason.0.len() > 1024
                || !requirement.minimum_confidence.0.is_finite()
                || !(0.0..=1.0).contains(&requirement.minimum_confidence.0)
                || !targets.insert(&requirement.target)
            {
                return Err(Error(
                    "goal knowledge dependency is invalid or duplicated".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskToLearnRecord {
    pub schema_version: SchemaVersion,
    pub goal: GoalFrameRef,
    pub scope: Scope,
    pub target: LearningTarget,
    pub reason: ReasonRef,
    pub impact: Impact,
    pub origin: SchemaDigest,
}

impl AskToLearnRecord {
    pub fn expected_origin(&self) -> Result<SchemaDigest> {
        canonical_digest(&(&self.scope, &self.target))
    }
    pub fn validate(&self) -> Result<()> {
        GoalKnowledge {
            schema_version: self.schema_version,
            scope: self.scope.clone(),
            allow_questions: true,
            requirements: vec![KnowledgeRequirement {
                schema_version: self.schema_version,
                target: self.target.clone(),
                reason: self.reason.clone(),
                impact: self.impact,
                minimum_confidence: Confidence(0.0),
            }],
        }
        .validate()?;
        if self.goal.0.trim().is_empty() || self.origin != self.expected_origin()? {
            return Err(Error(
                "learning proposal is not bound to its goal and origin".into(),
            ));
        }
        Ok(())
    }
    pub fn seed(&self) -> NodeId {
        NodeId(format!("knowledge-gap:{}", self.origin.0))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningReplyRecord {
    pub schema_version: SchemaVersion,
    pub proposal: ProposalRef,
    /// An owner-owned content reference, never a claim to stable knowledge.
    pub value: UserAttributeValueRef,
}

impl LearningReplyRecord {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.proposal.0.trim().is_empty()
            || self.value.0.trim().is_empty()
            || self.value.0.len() > 4096
        {
            return Err(Error("learning reply is incomplete".into()));
        }
        Ok(())
    }
}
