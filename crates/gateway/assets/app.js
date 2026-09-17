const state = {
  token: sessionStorage.getItem("forme.gateway.token") || "",
  profile: null,
  activeRun: null,
  activeSession: null,
  runs: [],
  events: [],
  cases: [],
  jobs: [],
};

const terminalRunStatuses = new Set(["Complete", "Aborted", "Failed", "Limited"]);
const activePollIntervalMs = 1500;
let activePollTimer;

const element = (id) => document.getElementById(id);
const tokenInput = element("token-input");
const connectionState = element("connection-state");
const runList = element("run-list");
const timeline = element("timeline");
const runTitle = element("run-title");
const runStatus = element("run-status");
const cancelButton = element("cancel-run");
const approvalList = element("approval-list");
const traceSummary = element("trace-summary");
const traceList = element("trace-list");
const evalList = element("eval-list");
const jobList = element("job-list");
const toast = element("toast");

tokenInput.value = state.token;

function setConnection(label, status) {
  connectionState.textContent = label;
  connectionState.dataset.state = status;
}

function notify(message) {
  toast.textContent = message;
  toast.hidden = false;
  window.setTimeout(() => {
    toast.hidden = true;
  }, 3600);
}

function authHeaders(mutation = false, json = false) {
  const headers = new Headers();
  headers.set("Authorization", `Bearer ${state.token}`);
  if (mutation) {
    headers.set("x-forme-csrf", "1");
  }
  if (json) {
    headers.set("Content-Type", "application/json");
  }
  return headers;
}

async function api(path, options = {}) {
  if (!state.token) {
    throw new Error("Access token is required");
  }
  const response = await fetch(path, options);
  if (!response.ok) {
    let message = `Request failed (${response.status})`;
    try {
      const body = await response.json();
      if (body.error) {
        message = body.error;
      }
    } catch (_error) {
      // The status remains the authoritative fallback.
    }
    throw new Error(message);
  }
  return response;
}

async function connect() {
  try {
    const response = await api("/v1/profile", {
      headers: authHeaders(),
    });
    state.profile = await response.json();
    setConnection("Ready", "ready");
    await Promise.all([loadRuns(), loadCases(), loadJobs()]);
  } catch (error) {
    stopActivePolling();
    state.profile = null;
    setConnection("Locked", "error");
    notify(error.message);
  }
}

let tokenConnectTimer;
tokenInput.addEventListener("input", () => {
  state.token = tokenInput.value.trim();
  sessionStorage.setItem("forme.gateway.token", state.token);
  window.clearTimeout(tokenConnectTimer);
  tokenConnectTimer = window.setTimeout(connect, 250);
});

element("run-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const session = element("session-input").value.trim();
  const input = element("request-input").value.trim();
  if (!session || !input) {
    return;
  }
  const request = {
    schema_version: 1,
    source: "UserTurn",
    session,
    agent_profile: state.profile?.gateway?.surface ? "agent:forme-local" : "agent:forme-local",
    input,
    budget: null,
    idempotency_key: `web:${crypto.randomUUID()}`,
  };
  try {
    setConnection("Running", "ready");
    const response = await api("/v1/runs", {
      method: "POST",
      headers: authHeaders(true, true),
      body: JSON.stringify(request),
    });
    const run = await response.json();
    state.runs.unshift({ run, session, input, status: "Accepted" });
    state.activeRun = run;
    state.activeSession = session;
    state.events = [];
    element("request-input").value = "";
    renderRuns();
    renderTimeline();
    await loadActive({ replaceEvents: true });
  } catch (error) {
    setConnection("Error", "error");
    notify(error.message);
  }
});

function renderRuns() {
  runList.replaceChildren();
  if (!state.runs.length) {
    runList.append(empty("No runs recorded."));
    return;
  }
  for (const item of state.runs) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "run-item";
    button.setAttribute("aria-current", String(item.run === state.activeRun));
    const title = document.createElement("p");
    title.className = "item-title";
    title.textContent = item.input;
    const meta = document.createElement("p");
    meta.className = "item-meta";
    meta.textContent = `${item.status || "Unknown"} | ${item.session} | ${item.run}`;
    button.append(title, meta);
    button.addEventListener("click", async () => {
      stopActivePolling();
      state.activeRun = item.run;
      state.activeSession = item.session;
      state.events = [];
      renderRuns();
      renderTimeline();
      await loadActive({ replaceEvents: true });
    });
    runList.append(button);
  }
}

async function loadRuns() {
  try {
    const response = await api("/v1/runs", { headers: authHeaders() });
    const summaries = await response.json();
    const browserRuns = new Map(state.runs.map((item) => [item.run, item]));
    state.runs = summaries.map((summary) => {
      const existing = browserRuns.get(summary.run);
      return {
        run: summary.run,
        session: summary.session,
        input: existing?.input || summary.run,
        status: summary.status,
      };
    });
    renderRuns();
  } catch (error) {
    notify(error.message);
  }
}

function stopActivePolling() {
  if (activePollTimer) {
    window.clearTimeout(activePollTimer);
    activePollTimer = undefined;
  }
}

function scheduleActivePoll(run, delay = activePollIntervalMs) {
  stopActivePolling();
  activePollTimer = window.setTimeout(async () => {
    if (state.activeRun !== run) {
      return;
    }
    try {
      await loadActive({ replaceEvents: false });
    } catch (error) {
      if (state.activeRun === run) {
        setConnection("Disconnected", "error");
        notify(error.message);
        scheduleActivePoll(run, 2500);
      }
    }
  }, delay);
}

async function loadActive({ replaceEvents = true } = {}) {
  if (!state.activeRun) {
    return;
  }
  stopActivePolling();
  const run = state.activeRun;
  const summaryResponse = await api(`/v1/runs/${encodeURIComponent(run)}`, {
    headers: authHeaders(),
  });
  const summary = await summaryResponse.json();
  if (state.activeRun !== run) {
    return;
  }
  const after = replaceEvents ? 0 : (state.events.at(-1)?.stream_seq || 0);
  await loadEvents(after, run, replaceEvents);
  if (state.activeRun !== run) {
    return;
  }
  const listed = state.runs.find((item) => item.run === run);
  if (listed) {
    listed.status = summary.status;
    listed.session = summary.session;
  }
  renderRuns();
  runTitle.textContent = run;
  runStatus.textContent = summary.status;
  cancelButton.disabled = terminalRunStatuses.has(summary.status);
  if (terminalRunStatuses.has(summary.status)) {
    setConnection("Ready", "ready");
  } else {
    setConnection(summary.status, "ready");
    scheduleActivePoll(run);
  }
}

async function loadEvents(after, run = state.activeRun, replace = after === 0) {
  if (!run) {
    return;
  }
  const response = await api(
    `/v1/runs/${encodeURIComponent(run)}/events?after=${after}`,
    { headers: authHeaders() },
  );
  const text = await response.text();
  const incoming = parseEventStream(text);
  if (state.activeRun !== run) {
    return;
  }
  if (replace) {
    state.events = incoming;
  } else {
    const known = new Set(state.events.map((event) => event.stream_seq));
    state.events.push(...incoming.filter((event) => !known.has(event.stream_seq)));
    state.events.sort((left, right) => left.stream_seq - right.stream_seq);
  }
  renderTimeline();
}

function parseEventStream(text) {
  const events = [];
  for (const block of text.split("\n\n")) {
    const data = block
      .split("\n")
      .find((line) => line.startsWith("data: "));
    if (data) {
      events.push(JSON.parse(data.slice(6)));
    }
  }
  return events;
}

function renderTimeline() {
  timeline.replaceChildren();
  if (!state.events.length) {
    timeline.append(empty("No events in this snapshot."));
    return;
  }
  for (const event of state.events) {
    const row = eventRow(event);
    const candidate = event.payload?.CandidateCreated;
    if (candidate) {
      const review = document.createElement("button");
      review.type = "button";
      review.className = "quiet-command event-review";
      review.textContent = "Review";
      review.addEventListener("click", () => openCandidate(candidate));
      row.append(review);
    }
    timeline.append(row);
  }
}

function eventRow(event) {
  const row = document.createElement("article");
  row.className = "event-row";
  const sequence = document.createElement("div");
  sequence.className = "event-seq";
  sequence.textContent = `#${event.stream_seq}`;
  const content = document.createElement("div");
  const kind = document.createElement("p");
  kind.className = "event-kind";
  kind.textContent = event.kind;
  const meta = document.createElement("p");
  meta.className = "event-meta";
  meta.textContent = `${event.provenance.source} | ${event.event_id}`;
  const detail = document.createElement("pre");
  detail.className = "event-detail";
  detail.textContent = JSON.stringify(event.payload, null, 2);
  content.append(kind, meta, detail);
  row.append(sequence, content);
  return row;
}

function openCandidate(candidate) {
  element("candidate-id").value = candidate.candidate_id;
  element("candidate-evidence").value = candidate.evidence_refs?.[0] || "";
  element("retraction-target").value = candidate.target || "";
  element("retraction-lineage").value = "";
  element("retraction-derived").value = "";
  element("candidate-dialog").showModal();
}

element("candidate-form").addEventListener("submit", async (event) => {
  if (event.submitter?.value === "cancel") {
    return;
  }
  event.preventDefault();
  const candidate = element("candidate-id").value;
  const decision = element("candidate-decision").value;
  const retraction = decision === "Retract"
    ? {
        schema_version: 1,
        target: element("retraction-target").value.trim(),
        lineage: element("retraction-lineage").value.trim(),
        derived_refs: [element("retraction-derived").value.trim()],
      }
    : null;
  const command = {
    schema_version: 1,
    run: state.activeRun,
    candidate,
    expected_state: element("candidate-state").value,
    decision,
    actor: "Owner",
    evidence: [element("candidate-evidence").value.trim()],
    retraction,
  };
  try {
    await api(`/v1/candidates/${encodeURIComponent(candidate)}/review`, {
      method: "POST",
      headers: authHeaders(true, true),
      body: JSON.stringify(command),
    });
    element("candidate-dialog").close();
    await loadEvents(state.events.at(-1)?.stream_seq || 0);
    notify("Candidate review recorded");
  } catch (error) {
    notify(error.message);
  }
});

async function loadApprovals() {
  if (!state.activeSession) {
    approvalList.replaceChildren(empty("Select a run first."));
    return;
  }
  try {
    const response = await api(
      `/v1/sessions/${encodeURIComponent(state.activeSession)}/approvals`,
      { headers: authHeaders() },
    );
    renderApprovals(await response.json());
  } catch (error) {
    notify(error.message);
  }
}

function renderApprovals(approvals) {
  approvalList.replaceChildren();
  if (!approvals.length) {
    approvalList.append(empty("No pending approvals."));
    return;
  }
  for (const approval of approvals) {
    const item = document.createElement("article");
    item.className = "approval-item";
    const title = document.createElement("p");
    title.className = "item-title";
    title.textContent = approval.action_summary;
    const meta = document.createElement("p");
    meta.className = "item-meta";
    meta.textContent = `${approval.risk_level} | ${approval.scope}`;
    const actions = document.createElement("div");
    actions.className = "approval-actions";
    actions.append(
      approvalButton("Grant once", approval, "Granted", "primary-command"),
      approvalButton("Deny", approval, "Denied", "danger-command"),
    );
    item.append(title, meta, actions);
    approvalList.append(item);
  }
}

function approvalButton(label, approval, outcome, className) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = className;
  button.textContent = label;
  button.addEventListener("click", async () => {
    const decision = {
      schema_version: 1,
      approval_id: approval.approval_id,
      outcome,
      approver: state.profile.owner,
      bound_plan_digest: approval.plan_digest,
      policy_version: approval.policy_version,
      tool_schema_version: approval.tool_schema_version,
      nonce: `web:${crypto.randomUUID()}`,
      use_by: Math.min(approval.expires_at - 1, Date.now() + 30000),
    };
    try {
      await api(`/v1/runs/${encodeURIComponent(approval.run)}/control`, {
        method: "POST",
        headers: authHeaders(true, true),
        body: JSON.stringify({ ResolveApproval: decision }),
      });
      state.activeRun = approval.run;
      await loadActive();
      await loadApprovals();
    } catch (error) {
      notify(error.message);
    }
  });
  return button;
}

async function loadTrace() {
  if (!state.activeRun) {
    traceList.replaceChildren(empty("Select a run first."));
    return;
  }
  try {
    const response = await api(`/v1/runs/${encodeURIComponent(state.activeRun)}/trace`, {
      headers: authHeaders(),
    });
    const trace = await response.json();
    renderTrace(trace);
  } catch (error) {
    notify(error.message);
  }
}

function renderTrace(trace) {
  traceSummary.replaceChildren(
    traceDatum("Events", String(trace.events.length)),
    traceDatum("Failures", String(trace.failure_refs.length)),
    traceDatum("Snapshot", String(trace.snapshot_upper_bound)),
  );
  traceList.replaceChildren();
  const selected = trace.events.filter((event) => [
    "DecisionTraceRecorded",
    "FailureEvidenceRecorded",
    "VerificationFinished",
    "CandidateCreated",
    "CandidatePromoted",
    "CandidateRejected",
    "CandidateDowngraded",
    "RetractionEvent",
  ].includes(event.kind));
  if (!selected.length) {
    traceList.append(empty("No decision, failure, verification, or candidate trace entries."));
    return;
  }
  for (const event of selected) {
    traceList.append(eventRow(event));
  }
}

function traceDatum(label, value) {
  const wrapper = document.createElement("div");
  const term = document.createElement("dt");
  term.textContent = label;
  const detail = document.createElement("dd");
  detail.textContent = value;
  wrapper.append(term, detail);
  return wrapper;
}

async function loadCases() {
  try {
    const response = await api("/v1/evals/cases", { headers: authHeaders() });
    state.cases = await response.json();
    renderCases();
  } catch (error) {
    notify(error.message);
  }
}

function localDateTime(timestamp) {
  const due = new Date(timestamp);
  const local = new Date(due.getTime() - due.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 16);
}

function resetJobDue() {
  const input = element("job-due");
  input.min = localDateTime(Date.now() + 60 * 1000);
  input.value = localDateTime(Date.now() + 5 * 60 * 1000);
}

resetJobDue();

element("job-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const session = element("job-session").value.trim();
  const message = element("job-message").value.trim();
  const due = new Date(element("job-due").value).getTime();
  const risk = element("job-risk").value;
  if (!session || !message || !Number.isFinite(due)) return;
  const now = Date.now();
  if (due <= now) {
    notify("Due time must be in the future.");
    return;
  }
  if (!state.profile) {
    notify("Connect before scheduling a job.");
    return;
  }
  const expires = due + 24 * 60 * 60 * 1000;
  const workspace = state.profile.gateway.workspace;
  const intention = `intention:web:${crypto.randomUUID()}`;
  const command = {
    schema_version: 1,
    intention: {
      schema_version: 1,
      id: intention,
      source: "Commitment",
      trigger: { At: due },
      state: "Pending",
      seed: message,
      provenance: {
        source: "UserTurn",
        actor: "Owner",
        trust_tier: "OwnerInput",
        caused_by: null,
      },
      expires_at: expires,
    },
    session,
    envelope: {
      schema_version: 1,
      scope: workspace,
      capability: {
        schema_version: 1,
        capabilities: ["capability:local-notification"],
        permissions: ["permission:local-notification"],
      },
      action_type: ["Deliver"],
      risk_limit: risk,
      approval_rule: risk === "High" ? "Ask" : "Allow",
      budget: "units:1",
      timebox: {
        schema_version: 1,
        starts_at: now,
        expires_at: expires,
        max_turns: 1,
      },
      rollback: {
        schema_version: 1,
        required: false,
        boundary: null,
      },
    },
    budget: "units:1",
  };
  try {
    await api("/v1/jobs", {
      method: "POST",
      headers: authHeaders(true, true),
      body: JSON.stringify(command),
    });
    element("job-message").value = "";
    resetJobDue();
    await loadJobs();
  } catch (error) {
    notify(error.message);
  }
});

async function loadJobs() {
  try {
    const response = await api("/v1/jobs", { headers: authHeaders() });
    state.jobs = await response.json();
    renderJobs();
  } catch (error) {
    notify(error.message);
  }
}

function renderJobs() {
  jobList.replaceChildren();
  if (!state.jobs.length) {
    jobList.append(empty("No scheduled jobs."));
    return;
  }
  for (const job of state.jobs) {
    const item = document.createElement("article");
    item.className = "job-item";
    const title = document.createElement("p");
    title.className = "item-title";
    title.textContent = job.intention.seed;
    const meta = document.createElement("p");
    meta.className = "item-meta";
    const trigger = job.intention.trigger?.At ?? "event/condition";
    const due = Number.isFinite(trigger) ? new Date(trigger).toLocaleString() : trigger;
    meta.textContent = `${job.intention.state} | ${job.run_status || "Not started"} | ${due}`;
    const actions = document.createElement("div");
    actions.className = "job-actions";
    if (job.run) {
      const open = document.createElement("button");
      open.type = "button";
      open.className = "quiet-command";
      open.textContent = "Open run";
      open.addEventListener("click", async () => {
        state.activeRun = job.run;
        state.activeSession = job.binding.session;
        state.events = [];
        await loadRuns();
        await loadActive({ replaceEvents: true });
      });
      actions.append(open);
    }
    if (["Pending", "Fired"].includes(job.intention.state)) {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.className = "danger-command";
      cancel.textContent = "Cancel";
      cancel.addEventListener("click", async () => {
        try {
          await api(`/v1/jobs/${encodeURIComponent(job.intention.id)}/cancel`, {
            method: "POST",
            headers: authHeaders(true),
          });
          await loadJobs();
        } catch (error) {
          notify(error.message);
        }
      });
      actions.append(cancel);
    }
    item.append(title, meta, actions);
    jobList.append(item);
  }
}

function renderCases() {
  evalList.replaceChildren();
  for (const evalCase of state.cases) {
    const item = document.createElement("article");
    item.className = "eval-item";
    const title = document.createElement("p");
    title.className = "item-title";
    title.textContent = evalCase.case_ref;
    const meta = document.createElement("p");
    meta.className = "item-meta";
    meta.textContent = `${evalCase.kind} | ${evalCase.rubric}`;
    const actions = document.createElement("div");
    actions.className = "eval-actions";
    const run = document.createElement("button");
    run.type = "button";
    run.className = "quiet-command";
    run.textContent = "Run case";
    run.addEventListener("click", () => runCase(evalCase, item));
    actions.append(run);
    item.append(title, meta, actions);
    evalList.append(item);
  }
}

async function runCase(evalCase, item) {
  const gateway = state.profile.gateway;
  const evalRef = `eval:web:${crypto.randomUUID()}`;
  const request = {
    schema_version: 1,
    case: evalCase,
    profile: {
      schema_version: 1,
      eval_ref: evalRef,
      model: gateway.model,
      policy: gateway.policy,
      toolset: gateway.toolset,
      workspace: gateway.workspace,
      event_schema: 1,
      replay_snapshot: `snapshot:web:${crypto.randomUUID()}`,
    },
  };
  try {
    const response = await api("/v1/evals/run", {
      method: "POST",
      headers: authHeaders(true, true),
      body: JSON.stringify(request),
    });
    const report = await response.json();
    const result = document.createElement("p");
    result.className = report.outcome === "Pass" ? "item-meta status-pass" : "item-meta status-fail";
    result.textContent = typeof report.outcome === "string"
      ? report.outcome
      : JSON.stringify(report.outcome);
    item.append(result);
    state.activeRun = report.run;
    state.activeSession = evalCase.request.session;
    state.runs.unshift({
      run: report.run,
      session: evalCase.request.session,
      input: evalCase.case_ref,
    });
    renderRuns();
    await loadActive();
  } catch (error) {
    notify(error.message);
  }
}

function empty(message) {
  const value = document.createElement("p");
  value.className = "empty-state";
  value.textContent = message;
  return value;
}

document.querySelectorAll("[data-view]").forEach((button) => {
  button.addEventListener("click", async () => {
    const view = button.dataset.view;
    document.querySelectorAll("[data-view]").forEach((tab) => {
      tab.setAttribute("aria-selected", String(tab === button));
    });
    document.querySelectorAll("[data-view-panel]").forEach((panel) => {
      panel.hidden = panel.dataset.viewPanel !== view;
    });
    if (view === "approvals") await loadApprovals();
    if (view === "trace") await loadTrace();
    if (view === "jobs") await loadJobs();
    if (view === "evals") await loadCases();
  });
});

element("refresh-active").addEventListener("click", loadActive);
element("refresh-runs").addEventListener("click", loadRuns);
element("refresh-approvals").addEventListener("click", loadApprovals);
element("refresh-trace").addEventListener("click", loadTrace);
element("refresh-jobs").addEventListener("click", loadJobs);
element("refresh-evals").addEventListener("click", loadCases);

cancelButton.addEventListener("click", async () => {
  if (!state.activeRun) return;
  try {
    await api(`/v1/runs/${encodeURIComponent(state.activeRun)}/control`, {
      method: "POST",
      headers: authHeaders(true, true),
      body: JSON.stringify({
        Cancel: {
          schema_version: 1,
          reason: "owner cancelled from local web",
        },
      }),
    });
    await loadActive();
  } catch (error) {
    notify(error.message);
  }
});

if (state.token) {
  connect();
}
