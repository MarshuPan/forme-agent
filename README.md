# forme

forme is an open-source agent runtime for complex, ongoing work.

It combines proactive cognition, persistent memory, coordination, execution,
verification, and governed autonomy in one evolving runtime. It is designed for
work that must be understood over time, revisited as conditions change, and
carried through to a verified result.

## Why forme

Most agent interactions are bounded by one prompt and one response. Complex
work is different: goals change, useful context is distributed across time and
projects, decisions depend on experience, and a result is not complete until it
has been checked.

forme keeps that whole loop together. It can remember what happened, form a
working understanding of why it matters, notice what deserves attention, plan
an appropriate route, coordinate the work, verify the outcome, and use the
evidence to improve the next decision.

```text
observations and requests
            |
            v
   memory -> understanding -> judgment
            |                    |
            +--> active agenda --+--> coordination
                                           |
                                           v
                                      execution
                                           |
                                           v
                         verification, feedback, and failure evidence
                                           |
                                           +------> next judgment
```

## The core design

### Proactive cognition and memory

Memory in forme is more than a transcript archive. Events, outcomes, feedback,
and corrections are connected into scoped understanding that can guide future
work. The cognitive layer can maintain judgment frames, quality expectations,
and known blind spots, while keeping new conclusions as evidence-backed
candidates until they have earned a more stable place.

Proactivity is not a timer that blindly fires. forme watches authorized
observations, changes, unresolved gaps, goal pressure, contradictions, and
commitments. It gives those signals a bounded attention space, evaluates their
value and urgency, and chooses whether to observe, ask a focused question,
suggest a next step, prepare work, or act within an explicit authority.

Unfinished work can become a commitment or a prospective intention. Time,
events, conditions, or new evidence can bring it back to attention, with the
current situation checked again before anything proceeds.

### Two models that grow together

forme develops two complementary layers of attributes through use:

- **User model:** an evolving understanding of the user's goals, preferences,
  habits, risk boundaries, working style, and standards for a good result.
- **Agent self-model:** an evolving understanding of what the agent can do,
  where it is reliable, where it has gaps or blind spots, and which approaches
  have or have not worked.

Together they form a partnership model: a practical description of how the user
and the agent complement one another, correct one another, and gradually share
judgment. These models are not fully specified at startup. They grow from task
outcomes, verification, failure, feedback, and repeated correction. Different
people and different working rhythms therefore produce different paths of
development.

As the evidence becomes stronger, forme can take responsibility for a bounded
part of a decision. When the evidence is weak, the stakes are high, or the
context is unclear, it lowers its intervention level and asks, prepares, or
waits instead.

### Complex work needs coordination

A complex request is not necessarily a list of subtasks. forme first frames the
goal and the situation, then selects resources, execution routes, completion
criteria, and checkpoints that fit the available capabilities and the current
evidence.

The route can change as the work reveals new information. A task may stay in one
run, use a specialized tool, or create several bounded execution units for
research, construction, evaluation, or integration. Those units return results
and evidence to the coordinating runtime; they do not create separate stable
brains or independent long-term memory.

Long-running workspaces retain their charter, active goals, open loops,
decisions, and review points. The runtime can revisit a project, add or remove
work as evidence changes, and re-coordinate instead of treating a first plan as
permanent.

## From request to verified progress

1. **Understand.** Relevant history, memory, cognitive references, project
   constraints, and available capabilities are assembled for the current scope.
2. **Assess.** The runtime evaluates value, competence, risk, policy, and the
   authority available for the next intervention.
3. **Coordinate.** It chooses a route, resources, completion contract, and
   verification strategy. It can ask for the missing information that would
   change that decision.
4. **Execute.** Tools, skills, connectors, and execution units operate through
   the same governed harness.
5. **Verify.** The result is checked against the completion contract. Success,
   failure, rejection, uncertainty, and unknown side effects remain distinct
   outcomes.
6. **Learn and continue.** Evidence and feedback update candidates, judgment,
   the user model, and the agent self-model. The next step can then be chosen
   from a better understanding of the work.

## Governed autonomy

Autonomy in forme is earned through evidence and remains bounded by explicit
authority. A useful intervention must pass three questions:

1. Is it valuable enough to deserve attention or action?
2. Is the agent competent and sufficiently informed for this scope and risk?
3. Does policy and the current autonomy envelope allow it?

Depending on those answers, the same idea may be observed, suggested, prepared,
carried out with approval, or carried out within a previously granted boundary.
High-impact actions still require an explicit boundary. Trust can inform a
decision, but it never replaces permission. Permissions are checked again at
the execution boundary, and an unknown side effect is not blindly retried.

## One runtime across many surfaces

CLI, Gateway, scheduler, workspace activity, and background cognition compose
the same persistent runtime. They share one authoritative event history so that
memory, coordination, proactivity, approvals, and recovery describe the same
work.

The runtime is organized around three cooperating kernels:

| Kernel | Role |
| --- | --- |
| **Cognitive evolution** | Turn observations, outcomes, feedback, and corrections into reviewed memory, models, and judgment. |
| **Coordination** | Turn goals and context into resource choices, execution routes, completion criteria, and replanning. |
| **Proactivity** | Notice relevant changes, manage attention and intentions, and propose useful next work. |

Capabilities such as files, shell commands, browser and computer adapters, MCP
servers, skills, plugins, and application APIs enter through a common capability
and execution boundary. The boundary keeps the source of a capability, its
permissions, its risk, and its evidence visible to the rest of the runtime.

## What forme is built to handle

- Research and analysis that accumulate evidence and need to be revisited.
- Software, product, and operational projects with goals, dependencies, and
  changing constraints.
- Workflows that span multiple tools, models, people, or execution units.
- Monitoring and follow-up where a useful next step depends on a later event or
  condition.
- Decisions that benefit from learning the working style, standards, and
  reliable boundaries of the people involved.

## Design principles

- **Memory becomes understanding.** History is connected to judgment, not only
  retrieved as text.
- **Candidates come before stable conclusions.** New beliefs and attributes need
  evidence, review, and a path for retraction.
- **Autonomy follows demonstrated competence.** The agent can become more useful
  without becoming less accountable.
- **Verification is part of completion.** A fluent final response is not proof
  that the work succeeded.
- **Failure is usable evidence.** Rejections, mistakes, uncertainty, and
  unexpected effects shape the next route.
- **Delegation stays bounded.** Execution units receive the minimum scope they
  need and return results to the coordinating runtime.
- **The user remains in control.** Authority, approvals, data scope, and
  lifecycle decisions remain explicit and inspectable.

## Explore the design

- [Project documentation](docs/README.md)
- [Vision and scope](docs/requirements/01-vision-and-scope.md)
- [Capability requirements](docs/requirements/02-capability-requirements.md)
- [Canonical contract](docs/architecture/canonical-contract.md)
- [Proactive cognitive kernel](docs/architecture/02-proactive-cognitive-kernel.md)
- [Temporal memory and user model](docs/prd/11-temporal-memory-and-user-model.md)
- [Coordination and orchestration](docs/prd/12-coordination-orchestration-subagent.md)
- [Core runtime architecture](docs/architecture/09-v1-core-brain-runtime-closure-architecture.md)

## Project

forme is implemented as a Rust workspace and released under the [MIT License](LICENSE).
Engineering and contribution guidance lives in [AGENTS.md](AGENTS.md) and the
[code quality guide](docs/engineering/code-quality.md).
