//! Pure route declaration validation, shared by ledger transitions.
use super::*;

pub(super) fn validate_route_declaration(
    route: &RouteDeclaration,
    project: &WorkspaceProject,
) -> Result<()> {
    if route.schema_version != SchemaVersion(1)
        || route.route.0.is_empty()
        || route.parent.0.is_empty()
        || route.goal != project.goal
        || route.done != project.done
        || route.signature.schema_version != SchemaVersion(1)
        || route.budget_units == 0
        || route.budget_units > project.budget_units
        || route.nodes.len() > 32
        || route.dependencies.len() > 128
        || route.charter_digest.0.is_empty()
        || route.input_digest.0.is_empty()
        || route.review_digest.0.is_empty()
        || route.scores.len() > 64
        || route.threshold == 0
        || route.threshold > 6400
        || route.confidence_millis > 1000
        || route.scores.iter().any(|s| {
            s.schema_version != SchemaVersion(1)
                || s.pattern.0.is_empty()
                || s.dimensions.iter().any(|d| *d != 0 && *d != 1000)
                || s.anchors > 200
                || s.fitness > 200
                || s.total != s.dimensions.iter().sum::<u16>() + s.anchors + s.fitness
        })
        || route.pattern.as_ref().is_some_and(|pattern| {
            !route
                .scores
                .iter()
                .any(|s| &s.pattern == pattern && s.compatible && s.total >= route.threshold)
        })
        || route.pattern.is_none()
            && route
                .scores
                .iter()
                .any(|s| s.compatible && s.total >= route.threshold)
        || (route.pattern.is_none()
            && (route.confidence_millis > 250
                || !route.checkpoint_required
                || !route.nodes.iter().any(|n| n.role.0 == "evaluator")))
    {
        return Err(Error(
            "invalid route declaration or unsafe custom route".into(),
        ));
    }
    let ids = route
        .nodes
        .iter()
        .map(|n| n.id.clone())
        .collect::<BTreeSet<_>>();
    if ids.len() != route.nodes.len()
        || route
            .nodes
            .iter()
            .chain(std::iter::once(&route.main))
            .any(|n| {
                n.schema_version != SchemaVersion(1)
                    || n.id.is_empty()
                    || n.role.0.is_empty()
                    || n.instruction.is_empty()
                    || n.instruction.len() > 16384
                    || n.intent.0.is_empty()
                    || n.model != project.model
                    || !charter_scope_contains(&project.scope, &n.scope)
                    || n.capabilities
                        .iter()
                        .any(|c| !project.capabilities.contains(c))
                    || n.budget_units == 0
                    || n.budget_units > route.budget_units
                    || n.max_attempts == 0
                    || n.max_attempts > 3
                    || n.requirements.is_empty()
                    || n.requirements.len() > 16
            })
    {
        return Err(Error(
            "route node exceeds project scope, resources or bounds".into(),
        ));
    }
    let mut visited = BTreeSet::new();
    if route.dependencies.iter().any(|e| {
        e.schema_version != SchemaVersion(1)
            || !ids.contains(&e.from)
            || !ids.contains(&e.to)
            || e.from == e.to
    }) {
        return Err(Error("route dependency is invalid".into()));
    }
    loop {
        let before = visited.len();
        for id in &ids {
            if route
                .dependencies
                .iter()
                .filter(|e| &e.to == id)
                .all(|e| visited.contains(&e.from))
            {
                visited.insert(id.clone());
            }
        }
        if visited.len() == ids.len() {
            break;
        }
        if visited.len() == before {
            return Err(Error("route dependency cycle".into()));
        }
    }
    Ok(())
}
