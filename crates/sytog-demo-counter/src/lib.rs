//! Example activity kept outside the generic SYTOG session core.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sytog_domain::{
    ActiveActivity, ActivityCommandEnvelope, ActivityDescriptor, ActivityId, ParticipantId,
};
use sytog_runtime::{ActivityEngine, ActivityRejection, ActivityTransition};

pub const ACTIVITY_ID: &str = "demo.counter";
pub const ACTIVITY_VERSION: &str = "1.0.0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterState {
    pub counter: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CounterCommand {
    Increment { amount: i64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CounterEvent {
    Incremented {
        participant: ParticipantId,
        amount: i64,
    },
}

pub struct CounterActivity;

impl CounterActivity {
    #[must_use]
    pub fn command(amount: i64) -> ActivityCommandEnvelope {
        ActivityCommandEnvelope {
            descriptor: Self::descriptor(),
            command_type: "increment".to_owned(),
            payload: json!({"type": "increment", "amount": amount}),
        }
    }

    #[must_use]
    pub fn descriptor() -> ActivityDescriptor {
        ActivityDescriptor {
            activity_id: ActivityId::from(ACTIVITY_ID),
            version: ACTIVITY_VERSION.to_owned(),
        }
    }
}

impl ActivityEngine for CounterActivity {
    fn descriptor(&self) -> ActivityDescriptor {
        Self::descriptor()
    }

    fn initial_state(&self) -> Value {
        json!(CounterState { counter: 0 })
    }

    fn decide(
        &self,
        actor: &ParticipantId,
        current: &ActiveActivity,
        command: &ActivityCommandEnvelope,
    ) -> Result<ActivityTransition, ActivityRejection> {
        if command.command_type != "increment" {
            return Err(rejection(
                "unknown_command",
                format!("unsupported counter command {}", command.command_type),
            ));
        }
        let command: CounterCommand = serde_json::from_value(command.payload.clone())
            .map_err(|error| rejection("invalid_payload", error.to_string()))?;
        let state: CounterState = serde_json::from_value(current.state.clone())
            .map_err(|error| rejection("invalid_state", error.to_string()))?;
        let CounterCommand::Increment { amount } = command;
        if amount <= 0 {
            return Err(rejection(
                "invalid_amount",
                "increment amount must be positive",
            ));
        }
        let counter = state
            .counter
            .checked_add(amount)
            .ok_or_else(|| rejection("counter_overflow", "counter would overflow"))?;
        let event = CounterEvent::Incremented {
            participant: actor.clone(),
            amount,
        };
        Ok(ActivityTransition {
            event_type: "incremented".to_owned(),
            payload: serde_json::to_value(event)
                .map_err(|error| rejection("serialization_failed", error.to_string()))?,
            resulting_state: json!(CounterState { counter }),
        })
    }
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

    #[test]
    fn counter_rejects_non_positive_increment() {
        let activity = CounterActivity;
        let active = ActiveActivity {
            descriptor: CounterActivity::descriptor(),
            revision: 0,
            state: activity.initial_state(),
        };
        assert!(
            activity
                .decide(
                    &ParticipantId::from("alice"),
                    &active,
                    &CounterActivity::command(0),
                )
                .is_err()
        );
    }
}
