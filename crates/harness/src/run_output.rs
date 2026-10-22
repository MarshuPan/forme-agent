//! Read-only recovery of committed public answers; no model or stable mutation.
use super::*;

const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;

impl ReactiveHarness {
    /// Return the live result or recover a committed final answer from its
    /// authenticated event stream. Incomplete/tool/internal outputs stay hidden.
    pub fn output_text(&self, run: p::RunId) -> p::Result<Option<String>> {
        let _data_access = self.data_read_guard()?;
        match self.run_handle(&run) {
            Ok(handle) => Ok(handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?
                .final_output
                .clone()),
            Err(error) => {
                let mut events = self.store.read_run(run).peekable();
                if events.peek().is_none() {
                    return Err(error);
                }
                committed_answer(events.map(|e| e.map(|e| e.payload)))
            }
        }
    }
}

fn committed_answer(
    events: impl Iterator<Item = p::Result<p::EventPayload>>,
) -> p::Result<Option<String>> {
    let mut call = None;
    let mut profile = None;
    let mut text = String::new();
    let mut finished = false;
    let mut final_output = false;
    for event in events {
        match event? {
            p::EventPayload::ModelCallStarted(p) => {
                call = Some(p.call_id);
                profile = Some(p.model_profile);
                text.clear();
                finished = false;
                final_output = false;
            }
            p::EventPayload::ModelCallDelta(p) if call.as_ref() == Some(&p.call_id) => {
                if finished || text.len().saturating_add(p.delta.len()) > MAX_ANSWER_BYTES {
                    return Err(p::Error(
                        "committed answer stream is invalid or exceeds its bound".into(),
                    ));
                }
                text.push_str(&p.delta);
            }
            p::EventPayload::ModelCallFinished(p) => {
                finished = call.as_ref() == Some(&p.call_id)
                    && profile.as_ref() == Some(&p.model_profile)
                    && p.finish_reason.0 != "provider_error";
                final_output = false;
            }
            p::EventPayload::OutputClassified(p) => {
                final_output = finished && p.kind == p::OutputKind::Final;
            }
            p::EventPayload::RunComplete(p) => {
                return Ok((p.stop_reason.0 == "final_output" && final_output).then_some(text));
            }
            p::EventPayload::RunFailed(_)
            | p::EventPayload::RunAborted(_)
            | p::EventPayload::RunLimited(_) => return Ok(None),
            _ => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn started(id: &str) -> p::EventPayload {
        p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
            call_id: p::ModelCallId(id.into()),
            model_profile: p::ModelProfileRef("model:test".into()),
        })
    }
    fn delta(id: &str, text: &str) -> p::EventPayload {
        p::EventPayload::ModelCallDelta(p::ModelCallDeltaPayload {
            call_id: p::ModelCallId(id.into()),
            delta: text.into(),
        })
    }
    fn finished(id: &str) -> p::EventPayload {
        p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
            call_id: p::ModelCallId(id.into()),
            model_profile: p::ModelProfileRef("model:test".into()),
            usage: p::ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
            finish_reason: p::FinishReason("stop".into()),
        })
    }
    fn classified(kind: p::OutputKind) -> p::EventPayload {
        p::EventPayload::OutputClassified(p::OutputClassifiedPayload { kind })
    }
    fn complete() -> p::EventPayload {
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("final_output".into()),
            result_ref: None,
        })
    }
    fn read(events: Vec<p::EventPayload>) -> Option<String> {
        committed_answer(events.into_iter().map(Ok)).unwrap()
    }
    #[test]
    fn committed_answers_require_a_finished_classified_call_and_a_terminal_commit() {
        let answer = vec![
            started("first"),
            delta("first", "Hello "),
            delta("other", "hidden"),
            delta("first", "world"),
            finished("first"),
            classified(p::OutputKind::Final),
        ];
        assert_eq!(read(answer.clone()), None);
        let mut committed = answer.clone();
        committed.push(complete());
        committed.extend([
            started("internal:reflection"),
            delta("internal:reflection", "private"),
        ]);
        assert_eq!(read(committed), Some("Hello world".into()));
        for kind in [p::OutputKind::Tool, p::OutputKind::Handoff] {
            let mut events = answer.clone();
            events.push(classified(kind));
            events.push(complete());
            assert_eq!(read(events), None);
        }
        let mut interrupted = answer;
        interrupted.extend([started("new"), complete()]);
        assert_eq!(read(interrupted), None);
        let failed = vec![
            started("failed"),
            delta("failed", "partial"),
            finished("failed"),
            classified(p::OutputKind::Final),
            p::EventPayload::RunFailed(p::RunFailedPayload {
                stop_reason: p::StopReason("verification_failed".into()),
                result_ref: None,
            }),
        ];
        assert_eq!(read(failed), None);
    }
    #[test]
    fn committed_answer_limits_and_store_errors_are_not_silently_truncated() {
        let events = vec![
            started("big"),
            delta("big", &"x".repeat(MAX_ANSWER_BYTES + 1)),
        ];
        assert!(committed_answer(events.into_iter().map(Ok)).is_err());
        assert!(committed_answer([Err(p::Error("read failed".into()))].into_iter()).is_err());
    }
}
