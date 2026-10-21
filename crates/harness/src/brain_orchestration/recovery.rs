use super::support::{
    coordination_node, final_node_output, node_record_attempts, verify_work_node,
};
use crate::{now_ms, production::ProductionRuntime, AgentHarness};
use forme_protocol as p;
use forme_store::EventStore;
use std::path::Path;

impl ProductionRuntime {
    pub(super) fn finish_node_from_events(
        &self,
        route: &p::RouteDeclaration,
        spec: &p::WorkNode,
        attempt: u32,
        run: &p::RunId,
        directory: &Path,
    ) -> p::Result<()> {
        let events = self
            .services()
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let terminal = events.iter().rev().find(|e| {
            matches!(
                e.kind,
                p::EventKind::RunComplete
                    | p::EventKind::RunFailed
                    | p::EventKind::RunAborted
                    | p::EventKind::RunLimited
            )
        });
        let unknown = events
            .iter()
            .any(|e| e.kind == p::EventKind::ActionOutcomeUnknown);
        let waiting =
            events.iter().any(|e| e.kind == p::EventKind::RunWaiting) && terminal.is_none();
        let effects_absent = !events.iter().any(|e| {
            matches!(
                e.kind,
                p::EventKind::ActionStarted
                    | p::EventKind::ActionCompleted
                    | p::EventKind::ActionOutcomeUnknown
            )
        });
        let summary = final_node_output(&events);
        let used = events
            .iter()
            .filter(|e| e.kind == p::EventKind::ModelCallStarted)
            .count() as u64;
        let (verification, evidence) = verify_work_node(spec, &events, &summary, directory)?;
        let status = if unknown || (terminal.is_none() && !waiting) {
            p::WorkNodeStatus::OutcomeUnknown
        } else if waiting {
            p::WorkNodeStatus::Waiting
        } else if terminal.is_some_and(|e| e.kind == p::EventKind::RunComplete)
            && verification == p::VerificationOutcome::Pass
        {
            p::WorkNodeStatus::Done
        } else if terminal.is_some_and(|e| e.kind == p::EventKind::RunAborted) && effects_absent {
            p::WorkNodeStatus::Cancelled
        } else {
            p::WorkNodeStatus::Failed
        };
        let summary = summary.chars().take(4000).collect::<String>();
        self.append_orchestration(
            p::IdempotencyKey(format!(
                "finish:{}:{}:{attempt}:{}",
                route.route.0,
                spec.id,
                events.last().map_or(0, |e| e.stream_seq)
            )),
            p::OrchestrationOperation::NodeFinished {
                route: route.route.clone(),
                node: spec.id.clone(),
                status,
                consumed: used,
                result: terminal.map(|_| p::ResultRef(format!("result:{}", run.0))),
                summary: summary.clone(),
                verification,
                evidence,
                effects_absent,
            },
            false,
            now_ms(),
        )?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&summary) {
            if spec.role.0 == "evaluator" || spec.role.0 == "integrator" {
                if let Some(conflicts) = value.get("conflicts").and_then(|v| v.as_array()) {
                    for (index, conflict) in conflicts.iter().take(8).enumerate() {
                        let nodes = conflict
                            .get("nodes")
                            .and_then(|v| v.as_array())
                            .map(|v| {
                                v.iter()
                                    .filter_map(|v| v.as_str().map(str::to_owned))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let kind = match conflict.get("kind").and_then(|v| v.as_str()) {
                            Some("fact") => p::ResultConflictKind::Fact,
                            Some("design") => p::ResultConflictKind::Design,
                            _ => p::ResultConflictKind::Quality,
                        };
                        if nodes.len() >= 2 {
                            self.append_orchestration(
                            p::IdempotencyKey(format!(
                                "conflict:{}:{}:{index}",
                                route.route.0, spec.id
                            )),
                            p::OrchestrationOperation::ConflictReported {
                                route: route.route.clone(),
                                conflict: p::RouteConflict {
                                    schema_version: p::SchemaVersion(1),
                                    kind,
                                    nodes,
                                    evidence: vec![p::EvidenceRef(format!("result:{}", run.0))],
                                    reason: p::ReasonRef(
                                        "execution results disagree; owner brain review required"
                                            .into(),
                                    ),
                                },
                            },
                            false,
                            now_ms(),
                        )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn cancel_project_route(
        &self,
        owner: &p::VerifiedPrincipal,
        route: &p::ExecutionRouteRef,
        command: p::IdempotencyKey,
    ) -> p::Result<()> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        self.append_orchestration(
            command,
            p::OrchestrationOperation::RouteCancelled {
                route: route.clone(),
            },
            true,
            now_ms(),
        )?;
        let record = self
            .orchestration_snapshot()?
            .routes
            .get(route)
            .cloned()
            .ok_or_else(|| p::Error("unknown route".into()))?;
        let h = self.harness();
        if h.run_handle(&record.declaration.parent).is_ok() {
            h.cancel(record.declaration.parent.clone())?;
        }
        h.cancel_child_runs(&record.declaration.parent)?;
        Ok(())
    }

    pub fn retry_project_node(
        &self,
        owner: &p::VerifiedPrincipal,
        route: p::ExecutionRouteRef,
        node: String,
        intent: p::ActionId,
        command: p::IdempotencyKey,
    ) -> p::Result<()> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        self.append_orchestration(
            command,
            p::OrchestrationOperation::NodeRetried {
                route,
                node,
                intent,
            },
            true,
            now_ms(),
        )
        .map(|_| ())
    }

    pub(super) fn finish_cancelled_before_spawn(
        &self,
        route: &p::ExecutionRouteRef,
        spec: &p::WorkNode,
    ) -> p::Result<()> {
        self.append_orchestration(
            p::IdempotencyKey(format!("cancel-before-spawn:{}:{}", route.0, spec.intent.0)),
            p::OrchestrationOperation::NodeFinished {
                route: route.clone(),
                node: spec.id.clone(),
                status: p::WorkNodeStatus::Cancelled,
                consumed: 0,
                result: None,
                summary: String::new(),
                verification: p::VerificationOutcome::Unverifiable(p::ReasonRef(
                    "cancelled before admission".into(),
                )),
                evidence: Vec::new(),
                effects_absent: true,
            },
            false,
            now_ms(),
        )
        .map(|_| ())
    }

    pub(crate) fn recover_route_nodes(&self, route: &p::ExecutionRouteRef) -> p::Result<()> {
        let state = self.orchestration_snapshot()?;
        let Some(record) = state.routes.get(route) else {
            return Err(p::Error("unknown route".into()));
        };
        for (id, node) in &record.nodes {
            if matches!(
                node.status,
                p::WorkNodeStatus::Running | p::WorkNodeStatus::Waiting
            ) {
                let spec = record.node_spec(id).expect("recorded node");
                let run = record.declaration.node_run_id(id, node.attempts);
                // A live worker owns this claim; another execute signal observes
                // it and returns rather than converting it to Unknown.
                let h = self.harness();
                let live = h.run_handle(&run).is_ok()
                    || h.child_harnesses
                        .lock()
                        .map_err(|_| p::Error("child registry is unavailable".into()))?
                        .contains_key(&run);
                let events = self
                    .services()
                    .store
                    .read_run(run.clone())
                    .collect::<p::Result<Vec<_>>>()?;
                let unknown = events
                    .iter()
                    .any(|e| e.kind == p::EventKind::ActionOutcomeUnknown);
                let terminal = events.iter().any(|e| {
                    matches!(
                        e.kind,
                        p::EventKind::RunComplete
                            | p::EventKind::RunFailed
                            | p::EventKind::RunAborted
                            | p::EventKind::RunLimited
                    )
                });
                if live && !terminal && !unknown {
                    continue;
                }
                let dir = self.project_node_directory(&record.declaration, id, node.attempts)?;
                if !live
                    && !terminal
                    && !unknown
                    && node.status == p::WorkNodeStatus::Waiting
                    && node.effects_absent
                {
                    if record.declaration.nodes.is_empty() {
                        h.restore_formal_approval(&run, spec.budget_units)?;
                    } else {
                        self.ensure_parent_handle(&record.declaration)?;
                        let mut node =
                            coordination_node(spec, &record.declaration, self.node_toolset(spec)?);
                        node.subtask.id =
                            format!("{}:attempt:{}", spec.id, node_record_attempts(record, id));
                        h.spawn_subagent_isolated(
                            record.declaration.parent.clone(),
                            &node,
                            Some(&dir),
                        )?;
                    }
                    continue;
                }
                self.finish_node_from_events(&record.declaration, spec, node.attempts, &run, &dir)?;
            }
        }
        Ok(())
    }
}
