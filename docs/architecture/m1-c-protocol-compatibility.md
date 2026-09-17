# M1-C protocol compatibility

M1-C keeps the 86-event taxonomy and every M0 event payload kind unchanged. The
shared protocol additions are `SchemaDigest`, the optional
`ActionParameters::Mcp.schema_digest` field required by deferred MCP schema
resolution, and the additive `ConfigCheck::Context` doctor row.

Compatibility rules:

- historical MCP action parameters without the field decode as
  `schema_digest = None`;
- `None` remains readable for replay and trace export, but is not executable by
  the M1-C capability recheck path;
- newly selected MCP tools bind a SHA-256 digest of the canonical selected input
  schema before Harness planning, so the existing execution plan digest also
  covers it;
- discovery metadata, search hits, disabled servers, and unselected schemas do
  not create a new stable protocol object or EventKind;
- plugin reload/failure state and compaction token metrics are owner-crate
  projections over existing events, not new event facts.
- old ConfigDoctor reports remain readable; new reports may include the Context
  check alongside the existing capability, MCP, and plugin rows.

This is an additive schema evolution. No crate dependency edge changes.
