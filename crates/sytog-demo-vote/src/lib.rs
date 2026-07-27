//! A second example activity proving the generic activity extension seam.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sytog_domain::{
    ActiveActivity, ActivityCommandEnvelope, ActivityDescriptor, ActivityId, ParticipantId,
};
use sytog_runtime::{ActivityEngine, ActivityRejection, ActivityTransition};

pub const ACTIVITY_ID: &str = "demo.vote";
pub const ACTIVITY_VERSION: &str = "1.0.0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VotePhase {
    Draft,
    Open,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteState {
    pub phase: VotePhase,
    pub moderator: Option<ParticipantId>,
    pub options: Vec<String>,
    pub votes: BTreeMap<ParticipantId, String>,
    pub result: Option<BTreeMap<String, u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VoteCommand {
    Open { options: Vec<String> },
    SubmitChoice { choice: String },
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VoteEvent {
    Opened {
        moderator: ParticipantId,
        options: Vec<String>,
    },
    ChoiceSubmitted {
        participant: ParticipantId,
        choice: String,
    },
    Closed {
        result: BTreeMap<String, u32>,
    },
}

pub struct VoteActivity;

impl VoteActivity {
    #[must_use]
    pub fn descriptor() -> ActivityDescriptor {
        ActivityDescriptor {
            activity_id: ActivityId::from(ACTIVITY_ID),
            version: ACTIVITY_VERSION.to_owned(),
        }
    }

    #[must_use]
    pub fn open(options: &[&str]) -> ActivityCommandEnvelope {
        Self::envelope(
            "open",
            &VoteCommand::Open {
                options: options.iter().map(|option| (*option).to_owned()).collect(),
            },
        )
    }

    #[must_use]
    pub fn submit(choice: &str) -> ActivityCommandEnvelope {
        Self::envelope(
            "submit_choice",
            &VoteCommand::SubmitChoice {
                choice: choice.to_owned(),
            },
        )
    }

    #[must_use]
    pub fn close() -> ActivityCommandEnvelope {
        Self::envelope("close", &VoteCommand::Close)
    }

    fn envelope(command_type: &str, command: &VoteCommand) -> ActivityCommandEnvelope {
        ActivityCommandEnvelope {
            descriptor: Self::descriptor(),
            command_type: command_type.to_owned(),
            payload: json!(command),
        }
    }
}

impl ActivityEngine for VoteActivity {
    fn descriptor(&self) -> ActivityDescriptor {
        Self::descriptor()
    }

    fn initial_state(&self) -> Value {
        json!(VoteState {
            phase: VotePhase::Draft,
            moderator: None,
            options: Vec::new(),
            votes: BTreeMap::new(),
            result: None,
        })
    }

    fn decide(
        &self,
        actor: &ParticipantId,
        current: &ActiveActivity,
        command: &ActivityCommandEnvelope,
    ) -> Result<ActivityTransition, ActivityRejection> {
        let command: VoteCommand = serde_json::from_value(command.payload.clone())
            .map_err(|error| rejection("invalid_payload", error.to_string()))?;
        let mut state: VoteState = serde_json::from_value(current.state.clone())
            .map_err(|error| rejection("invalid_state", error.to_string()))?;

        let event = match command {
            VoteCommand::Open { options } => open_vote(actor, &mut state, options)?,
            VoteCommand::SubmitChoice { choice } => submit_choice(actor, &mut state, choice)?,
            VoteCommand::Close => close_vote(actor, &mut state)?,
        };
        let event_type = match &event {
            VoteEvent::Opened { .. } => "opened",
            VoteEvent::ChoiceSubmitted { .. } => "choice_submitted",
            VoteEvent::Closed { .. } => "closed",
        };
        Ok(ActivityTransition {
            event_type: event_type.to_owned(),
            payload: serde_json::to_value(event)
                .map_err(|error| rejection("serialization_failed", error.to_string()))?,
            resulting_state: json!(state),
        })
    }
}

fn open_vote(
    actor: &ParticipantId,
    state: &mut VoteState,
    options: Vec<String>,
) -> Result<VoteEvent, ActivityRejection> {
    if state.phase != VotePhase::Draft {
        return Err(rejection("already_opened", "vote has already been opened"));
    }
    let mut normalized = Vec::new();
    for option in options {
        let option = option.trim();
        if option.is_empty() || normalized.iter().any(|existing| existing == option) {
            return Err(rejection(
                "invalid_options",
                "options must be non-empty and unique",
            ));
        }
        normalized.push(option.to_owned());
    }
    if normalized.len() < 2 {
        return Err(rejection(
            "invalid_options",
            "a vote requires at least two options",
        ));
    }
    state.phase = VotePhase::Open;
    state.moderator = Some(actor.clone());
    state.options.clone_from(&normalized);
    Ok(VoteEvent::Opened {
        moderator: actor.clone(),
        options: normalized,
    })
}

fn submit_choice(
    actor: &ParticipantId,
    state: &mut VoteState,
    choice: String,
) -> Result<VoteEvent, ActivityRejection> {
    if state.phase != VotePhase::Open {
        return Err(rejection("vote_not_open", "vote is not open"));
    }
    if !state.options.contains(&choice) {
        return Err(rejection("unknown_choice", "choice is not a vote option"));
    }
    if state.votes.contains_key(actor) {
        return Err(rejection(
            "already_voted",
            "participant has already submitted a choice",
        ));
    }
    state.votes.insert(actor.clone(), choice.clone());
    Ok(VoteEvent::ChoiceSubmitted {
        participant: actor.clone(),
        choice,
    })
}

fn close_vote(
    actor: &ParticipantId,
    state: &mut VoteState,
) -> Result<VoteEvent, ActivityRejection> {
    if state.phase != VotePhase::Open {
        return Err(rejection("vote_not_open", "vote is not open"));
    }
    if state.moderator.as_ref() != Some(actor) {
        return Err(rejection(
            "not_moderator",
            "only the participant who opened the vote may close it",
        ));
    }
    let mut result: BTreeMap<_, u32> = state
        .options
        .iter()
        .cloned()
        .map(|option| (option, 0))
        .collect();
    for choice in state.votes.values() {
        if let Some(total) = result.get_mut(choice) {
            *total += 1;
        }
    }
    state.phase = VotePhase::Closed;
    state.result = Some(result.clone());
    Ok(VoteEvent::Closed { result })
}

fn rejection(code: &str, detail: impl Into<String>) -> ActivityRejection {
    ActivityRejection {
        code: code.to_owned(),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(
        activity: &VoteActivity,
        active: &mut ActiveActivity,
        actor: &str,
        command: &ActivityCommandEnvelope,
    ) {
        let transition = activity
            .decide(&ParticipantId::from(actor), active, command)
            .expect("test transition is valid");
        active.state = transition.resulting_state;
        active.revision += 1;
    }

    #[test]
    fn vote_opens_collects_choices_and_closes_with_result() {
        let activity = VoteActivity;
        let mut active = ActiveActivity {
            descriptor: VoteActivity::descriptor(),
            revision: 0,
            state: activity.initial_state(),
        };
        apply(
            &activity,
            &mut active,
            "alice",
            &VoteActivity::open(&["tea", "coffee"]),
        );
        apply(
            &activity,
            &mut active,
            "bob",
            &VoteActivity::submit("coffee"),
        );
        apply(
            &activity,
            &mut active,
            "alice",
            &VoteActivity::submit("tea"),
        );
        apply(&activity, &mut active, "alice", &VoteActivity::close());

        let state: VoteState = serde_json::from_value(active.state).expect("state is valid");
        assert_eq!(state.phase, VotePhase::Closed);
        assert_eq!(state.result.expect("result exists")["coffee"], 1);
    }

    #[test]
    fn only_the_opening_participant_can_close() {
        let activity = VoteActivity;
        let mut active = ActiveActivity {
            descriptor: VoteActivity::descriptor(),
            revision: 0,
            state: activity.initial_state(),
        };
        apply(
            &activity,
            &mut active,
            "alice",
            &VoteActivity::open(&["yes", "no"]),
        );
        assert!(
            activity
                .decide(&ParticipantId::from("bob"), &active, &VoteActivity::close(),)
                .is_err()
        );
    }
}
