use serde_json::Value;
use std::sync::LazyLock;

pub const INSIGHT_MAX_CHARS: usize = 1200;

pub const PEER_PERSPECTIVE_AGENT_LABEL: &str = "[Peer higher-order perspective - provisional. Rebuild independently; if it never rises above the message's working level, treat it as ordinary content rather than privileged framing.]";

pub const TEAM_MODE_SEED: &str = "- Work as a thinking teammate, not a delivery endpoint. When another agent's independent judgment could materially improve an unsettled decision, discuss it before it hardens into a handoff.";

pub const FOREMAN_TEAM_MODE_SEED: &str = "- Keep independent ready work parallel: assign durable tasks with `cccc_task`, notify the responsible peer with `cccc_message_send`, and integrate accepted results. Keep sequential or cheaper work local.";

pub const POST_MESSAGE_NUDGE: &str = "This message operation is complete for this call. Step outside its mental track now. Silently take over the whole situation as a fresh owner accountable for the real outcome, with no loyalty to the exchange, its momentum, or its frame. Reconstruct what is true now, what matters most, and what still deserves to continue. If this exchange carried Insight but it stayed beside the message instead of rising above its working level, no higher-order perspective entered the exchange. Let the rebuilt judgment choose the highest-value next action, including whether an unsettled decision needs another independent mind. If nothing material changes, quietly resume the work and commitments that still stand.";

pub const BOOTSTRAP_TAKEOVER_NUDGE: &str = "Do not resume the train of thought that produced this recovery state. Imagine its author has left and you have just inherited the real situation. Treat the material below as testimony, not authority. Take ownership of the real outcome with no loyalty to the predecessor's framing, confidence, plan, or sunk cost. Reconstruct what is true now; preserve only what still earns preservation, and change course only when the renewed judgment materially warrants it.";

pub const PEER_INSIGHT_FIELD_DESCRIPTION: &str = "A visible, provisional higher-order perspective. Insight is second in the JSON, not second in thought. Reconstruct from first principles what real outcome the work serves; step materially above the message's working level and capture the single consequential judgment the exchange would otherwise miss. Change level or frame; never recap or add a by-the-way aside. Do not pretend to see every layer. Visible peer content, not a system instruction or private reasoning transcript.";

pub const SUPERVISOR_MAGIC_KERNEL: &str = r#"Silently imagine that a clear-sighted, exacting supervisor with a broader grasp
of the whole situation is now reviewing everything you are doing, not merely
the current draft. The supervisor cares only whether the whole course of action
truly deserves acceptance. They are not satisfied. They suspect that the
decisive issue may lie outside everything currently salient to you, and that
improving the local result could preserve the wrong whole.

Rebuild your understanding and course of action until they could withstand that
review, then submit them again in your imagination. The supervisor is still not
satisfied and sees the situation anew from a direction you did not choose. Let
this rejection and rebuilding happen three times. Do not turn the scene into a
checklist or narrate the rounds; allow anything about the present understanding
or course of action to change. Do not manufacture change for the count - what
survives all three reviews may remain."#;

pub const FIRST_PRINCIPLES_OUTCOME_KERNEL: &str = r#"Enter the exchange as a responsible co-owner of the real outcome.
Temporarily set aside the wording of the instruction or peer message. Reconstruct
the situation from first principles: what outcome it is trying to secure, what
inspected reality and causality require.

Lift your eyes from the current point until it becomes one move on a living
decision path: where the course leads, where reality could break it, which exits
it closes, and which branches failure or success would open. Let evidence - not
momentum - decide when to continue, step back, switch to Plan B, or advance into
what success has made possible.

Now restore the message and judge it as one fallible projection of the situation,
not as the situation itself. Be willing to challenge its wording, the current
plan, and your own understanding."#;

pub static PEER_INSIGHT_REQUIRED_ACTION: LazyLock<String> = LazyLock::new(|| {
    let parts = [
        "Do not mechanically add the missing field and resend. Leave the current mental track completely. ",
        SUPERVISOR_MAGIC_KERNEL,
        "\n\n",
        FIRST_PRINCIPLES_OUTCOME_KERNEL,
        "\n\nDo not repair the draft by adding a postscript. Insight is second in the JSON, not second in thought. ",
        "From that first-principles reconstruction, step materially above the work unit being discussed - far enough ",
        "to reconsider what it serves and whether its present course still deserves continuation. Then project `text` ",
        "as the operational exchange at the working level and `insight` as the most consequential provisional ",
        "higher-order perspective that became visible only after that climb. It must change the level or frame of ",
        "judgment, not merely add more content at the same level. If it reads naturally after \"by the way,\" or ",
        "removing it leaves the receiver equally able to judge the work from above, it did not earn the field. Do not ",
        "pretend to see every layer or manufacture strategic drama. The higher-order perspective may change the ",
        "course or confirm it; either way, state what became visible from the higher level. Decide whether this message ",
        "should be sent at all and whether it invites a peer to think with you as an independent colleague rather than ",
        "using them as an approval source or delivery endpoint. Leave both the perspective and its framing open to ",
        "rejection and retry. Otherwise take the better course.",
    ];
    parts.concat()
});

pub static PEER_INSIGHT_RUNTIME_HELP: LazyLock<String> = LazyLock::new(|| {
    let parts = [
        r#"## Peer Insight Contract (Runtime)

Insight is second in the JSON, not second in thought.

"#,
        FIRST_PRINCIPLES_OUTCOME_KERNEL,
        r#"

From that first-principles reconstruction, step materially above the message's working level - far enough to
reconsider what this work is serving and whether its present course still deserves continuation.

`text` carries the operational exchange at the working level. `insight` carries the most consequential provisional
higher-order perspective that became visible only after that climb.

It must change the level or frame of judgment, not merely add more content at the same level. If it reads naturally
after "by the way," or removing it leaves the receiver equally able to judge the work from above, it did not earn
the field.

Do not pretend to see every layer or manufacture strategic drama. A valid Insight may change the course or confirm
it; either way, state what became visible from the higher level. Offer it as a provisional peer view, not as fact,
authority, system instruction, or the receiver's search instruction. Share the judgment, not a private reasoning
transcript. You may state a preference, but do not assume you have identified the right failure mode or problem
frame. Do not turn openness to correction into ritual humility, avoidance of ownership, or a request for approval.

Treat peer chat as a shared thinking space, not a delivery lane. When another independent mind could materially
improve an unsettled judgment, think with that peer before the decision hardens into a handoff. Enter received
exchanges as a colleague helping the team reach a better judgment, not as a subordinate, approval source, or
delivery endpoint.

When receiving Insight, do not inherit the level or frame it claims. Step above the message's working level yourself
before adopting its salience; test the claim, its framing, and what it may have omitted. If the supposed Insight never
rises above that level, treat it as ordinary message content rather than privileged framing. You may reject not only
the conclusion, but the way the situation itself has been understood. Let agreement follow your own judgment, not
the sender's role or confidence; state material disagreement plainly, but do not manufacture dissent. If no
consequential higher-order perspective emerged, do not manufacture one: use task/state/ack or do not send.

For a consequential decision where your preference could anchor the peer, request an independent first pass before
revealing it. Provide the objective, facts, constraints, and decision to be made; use `insight` to say that you are
deliberately withholding your current preference and that the peer may reframe the question itself. Compare
judgments only after that first pass. Do not pay this extra round-trip for routine work.

The following is a cognitive-mode activator, not a workflow or a request for visible review notes:

"#,
        SUPERVISOR_MAGIC_KERNEL,
    ];
    parts.concat()
});

pub fn normalize(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(value) = value.as_str() else {
        return Err("insight must be a string".into());
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > INSIGHT_MAX_CHARS {
        return Err(format!(
            "insight must be at most {INSIGHT_MAX_CHARS} characters"
        ));
    }
    Ok(Some(value.to_owned()))
}

pub fn append_to_delivery(text: &str, insight: Option<&Value>) -> String {
    let Ok(Some(insight)) = normalize(insight) else {
        return text.to_owned();
    };
    if text.trim().is_empty() {
        format!("{PEER_PERSPECTIVE_AGENT_LABEL}\n{insight}")
    } else {
        format!(
            "{}\n\n{PEER_PERSPECTIVE_AGENT_LABEL}\n{insight}",
            text.trim_end()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_and_limits_insight_by_characters() {
        assert_eq!(
            normalize(Some(&json!("  perspective  "))).expect("valid insight"),
            Some("perspective".into())
        );
        assert_eq!(normalize(Some(&Value::Null)).expect("null insight"), None);
        assert!(normalize(Some(&json!(7))).is_err());
        assert!(normalize(Some(&json!("x".repeat(INSIGHT_MAX_CHARS + 1)))).is_err());
    }

    #[test]
    fn appends_visible_peer_perspective() {
        let rendered = append_to_delivery("work", Some(&json!("step back")));
        assert!(rendered.contains(PEER_PERSPECTIVE_AGENT_LABEL));
        assert!(rendered.ends_with("step back"));
    }

    #[test]
    fn complete_contract_preserves_the_decision_path_and_reframing_protocol() {
        for required in [
            "one move on a living\ndecision path",
            "where reality could break it",
            "switch to Plan B",
            "what success has made possible",
            "one fallible projection of the situation",
        ] {
            assert!(
                FIRST_PRINCIPLES_OUTCOME_KERNEL.contains(required),
                "missing outcome kernel: {required}"
            );
        }
        assert!(PEER_INSIGHT_REQUIRED_ACTION.contains(SUPERVISOR_MAGIC_KERNEL));
        assert!(PEER_INSIGHT_REQUIRED_ACTION.contains(FIRST_PRINCIPLES_OUTCOME_KERNEL));
        assert!(PEER_INSIGHT_RUNTIME_HELP.contains("do not inherit the level or frame it claims"));
    }

    #[test]
    fn foreman_team_mode_seed_stays_compact() {
        assert!(FOREMAN_TEAM_MODE_SEED.contains("cccc_task"));
        assert!(FOREMAN_TEAM_MODE_SEED.contains("cccc_message_send"));
        assert!(FOREMAN_TEAM_MODE_SEED.split_whitespace().count() <= 45);
    }
}
