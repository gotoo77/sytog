//! Functional capability offers and deterministic, explainable matching.

use std::{cmp::Ordering, collections::BTreeMap, collections::BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(CapabilityOfferId);
string_id!(CapabilityName);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareInventory {
    pub architecture: String,
    pub logical_cores: u16,
    pub total_memory_mib: u64,
    pub accelerator: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityOffer {
    pub id: CapabilityOfferId,
    pub name: CapabilityName,
    pub implementation: String,
    pub contract: CapabilityContract,
    pub concurrency: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "properties", rename_all = "snake_case")]
pub enum CapabilityContract {
    LlmInference(LlmInferenceContract),
    CpuCompute(CpuComputeContract),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmInferenceContract {
    pub models: BTreeSet<String>,
    pub context_limit: u32,
    pub languages: BTreeSet<String>,
    pub supports_streaming: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuComputeContract {
    pub architectures: BTreeSet<String>,
    pub logical_cores: u16,
    pub supports_wasm: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExposurePolicy {
    pub allowed_capabilities: BTreeSet<CapabilityName>,
    pub allowed_requesters: BTreeSet<String>,
    pub local_network_only: bool,
    pub manual_consent_required: bool,
    pub consent_granted: bool,
    pub max_memory_mib: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentAvailability {
    pub online: bool,
    pub active_jobs: BTreeMap<CapabilityOfferId, u16>,
    pub available_memory_mib: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub capability_offer_id: CapabilityOfferId,
    pub workload_profile: Option<String>,
    pub successful: bool,
    pub latency_ms: u32,
    pub throughput: Option<ThroughputMetric>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ThroughputMetric {
    TokensPerSecond(f64),
    TasksPerSecond(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeOffer {
    pub node_id: String,
    pub network: String,
    pub inventory: HardwareInventory,
    pub capabilities: Vec<CapabilityOffer>,
    pub policy: ExposurePolicy,
    pub availability: CurrentAvailability,
    pub observations: Vec<Observation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRequirement {
    pub capability: CapabilityName,
    pub requirement: CapabilityRequirement,
    pub workload_profile: Option<String>,
    pub local_network_only: bool,
    pub requester_id: String,
    pub requester_network: String,
    pub estimated_memory_mib: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "properties", rename_all = "snake_case")]
pub enum CapabilityRequirement {
    LlmInference(LlmInferenceRequirement),
    CpuCompute(CpuComputeRequirement),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmInferenceRequirement {
    pub model: Option<String>,
    pub minimum_context: u32,
    pub language: Option<String>,
    pub streaming_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuComputeRequirement {
    pub architecture: Option<String>,
    pub minimum_logical_cores: u16,
    pub wasm_required: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchResult {
    pub node_id: String,
    pub capability_offer_id: Option<CapabilityOfferId>,
    pub status: MatchStatus,
    pub score: Option<ScoreBreakdown>,
    pub reasons: Vec<MatchReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub scoring_version: String,
    pub success_rate: f64,
    pub latency_score: f64,
    pub contract_headroom: f64,
    pub final_score: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Compatible,
    Unavailable,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchReason {
    pub code: String,
    pub detail: String,
}

impl JobRequirement {
    /// Validates required identifiers and capability-specific constraints.
    ///
    /// # Errors
    ///
    /// Returns the first invalid field.
    pub fn validate(&self) -> Result<(), ValidationError> {
        required(&self.capability.0, "capability")?;
        required(&self.requester_id, "requester_id")?;
        required(&self.requester_network, "requester_network")?;
        if let Some(profile) = &self.workload_profile {
            required(profile, "workload_profile")?;
        }
        match &self.requirement {
            CapabilityRequirement::LlmInference(requirement) => {
                if requirement.minimum_context == 0 {
                    return Err(ValidationError::Invalid(
                        "minimum_context must be positive".to_owned(),
                    ));
                }
            }
            CapabilityRequirement::CpuCompute(requirement) => {
                if requirement.minimum_logical_cores == 0 {
                    return Err(ValidationError::Invalid(
                        "minimum_logical_cores must be positive".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }
}

impl NodeOffer {
    /// Validates offer identity, uniqueness, references, and concurrency.
    ///
    /// # Errors
    ///
    /// Returns the first invalid field or dangling observation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        required(&self.node_id, "node_id")?;
        required(&self.network, "network")?;
        let mut ids = BTreeSet::new();
        for offer in &self.capabilities {
            required(&offer.id.0, "capability_offer_id")?;
            required(&offer.name.0, "capability_name")?;
            required(&offer.implementation, "implementation")?;
            if offer.concurrency == 0 {
                return Err(ValidationError::Invalid(format!(
                    "offer {} has zero concurrency",
                    offer.id.0
                )));
            }
            if !ids.insert(offer.id.clone()) {
                return Err(ValidationError::Invalid(format!(
                    "duplicate offer id {}",
                    offer.id.0
                )));
            }
        }
        for observation in &self.observations {
            if !ids.contains(&observation.capability_offer_id) {
                return Err(ValidationError::Invalid(format!(
                    "observation references unknown offer {}",
                    observation.capability_offer_id.0
                )));
            }
        }
        for offer_id in self.availability.active_jobs.keys() {
            if !ids.contains(offer_id) {
                return Err(ValidationError::Invalid(format!(
                    "availability references unknown offer {}",
                    offer_id.0
                )));
            }
        }
        Ok(())
    }
}

/// Validates every node and rejects duplicate node identities.
///
/// # Errors
///
/// Returns the first node error or duplicated identity.
pub fn validate_nodes(nodes: &[NodeOffer]) -> Result<(), ValidationError> {
    let mut ids = BTreeSet::new();
    for node in nodes {
        node.validate()?;
        if !ids.insert(&node.node_id) {
            return Err(ValidationError::Invalid(format!(
                "duplicate node id {}",
                node.node_id
            )));
        }
    }
    Ok(())
}

#[must_use]
pub fn rank(job: &JobRequirement, nodes: &[NodeOffer]) -> Vec<MatchResult> {
    let mut results = Vec::new();
    for node in nodes {
        let offers: Vec<_> = node
            .capabilities
            .iter()
            .filter(|offer| offer.name == job.capability)
            .collect();
        if offers.is_empty() {
            results.push(missing_capability(job, node));
        } else {
            results.extend(offers.into_iter().map(|offer| evaluate(job, node, offer)));
        }
    }
    results.sort_by(compare_results);
    results
}

#[must_use]
pub fn evaluate(job: &JobRequirement, node: &NodeOffer, offer: &CapabilityOffer) -> MatchResult {
    let mut rejected = Vec::new();
    let mut unavailable = Vec::new();

    let contract_headroom = check_contract(job, offer, &mut rejected);
    check_policy(job, node, &mut rejected);
    if !rejected.is_empty() {
        return result(node, Some(offer), MatchStatus::Rejected, None, rejected);
    }

    if !node.availability.online {
        unavailable.push(reason("offline", "node is not currently online"));
    }
    let active_jobs = node
        .availability
        .active_jobs
        .get(&offer.id)
        .copied()
        .unwrap_or(0);
    if active_jobs >= offer.concurrency {
        unavailable.push(reason(
            "saturated",
            format!(
                "{active_jobs} active job(s) for concurrency {}",
                offer.concurrency
            ),
        ));
    }
    if node.availability.available_memory_mib < job.estimated_memory_mib {
        unavailable.push(reason(
            "memory_unavailable",
            format!(
                "{} MiB available, {} MiB required",
                node.availability.available_memory_mib, job.estimated_memory_mib
            ),
        ));
    }
    if !unavailable.is_empty() {
        return result(
            node,
            Some(offer),
            MatchStatus::Unavailable,
            None,
            unavailable,
        );
    }

    let observations: Vec<_> = node
        .observations
        .iter()
        .filter(|item| {
            item.capability_offer_id == offer.id
                && matches_workload_profile(job.workload_profile.as_deref(), item)
        })
        .collect();
    let success_rate = if observations.is_empty() {
        0.5
    } else {
        let successes = observations.iter().filter(|item| item.successful).count();
        ratio(successes, observations.len())
    };
    let latency_score = observations
        .iter()
        .filter(|item| item.successful)
        .map(|item| item.latency_ms)
        .min()
        .map_or(0.5, |latency| 1.0 / (1.0 + f64::from(latency) / 1_000.0));
    let final_score =
        round_score(0.4 * success_rate + 0.35 * latency_score + 0.25 * contract_headroom);
    let score = ScoreBreakdown {
        scoring_version: "v1".to_owned(),
        success_rate: round_score(success_rate),
        latency_score: round_score(latency_score),
        contract_headroom: round_score(contract_headroom),
        final_score,
    };
    result(
        node,
        Some(offer),
        MatchStatus::Compatible,
        Some(score),
        vec![
            reason(
                "contract_compatible",
                format!("{} contract satisfies the job", job.capability.0),
            ),
            reason(
                "policy_allows",
                "local exposure policy permits this requester and job",
            ),
            reason(
                "currently_available",
                "node and concrete offer have current capacity",
            ),
        ],
    )
}

fn matches_workload_profile(required: Option<&str>, observation: &Observation) -> bool {
    required.is_none() || observation.workload_profile.as_deref() == required
}

fn check_contract(
    job: &JobRequirement,
    offer: &CapabilityOffer,
    rejected: &mut Vec<MatchReason>,
) -> f64 {
    match (&job.requirement, &offer.contract) {
        (
            CapabilityRequirement::LlmInference(required),
            CapabilityContract::LlmInference(declared),
        ) => check_llm_contract(required, declared, rejected),
        (CapabilityRequirement::CpuCompute(required), CapabilityContract::CpuCompute(declared)) => {
            check_cpu_contract(required, declared, rejected)
        }
        _ => {
            rejected.push(reason(
                "contract_kind_mismatch",
                "offer contract family does not match the job requirement",
            ));
            0.0
        }
    }
}

fn check_llm_contract(
    required: &LlmInferenceRequirement,
    declared: &LlmInferenceContract,
    rejected: &mut Vec<MatchReason>,
) -> f64 {
    if let Some(model) = &required.model {
        if !declared.models.contains(model) {
            rejected.push(reason(
                "model_unavailable",
                format!("required model {model} is unavailable"),
            ));
        }
    }
    if declared.context_limit < required.minimum_context {
        rejected.push(reason(
            "context_too_small",
            format!(
                "context {} is below required {}",
                declared.context_limit, required.minimum_context
            ),
        ));
    }
    if let Some(language) = &required.language {
        if !declared.languages.contains(language) {
            rejected.push(reason(
                "language_unsupported",
                format!("language {language} is unsupported"),
            ));
        }
    }
    if required.streaming_required && !declared.supports_streaming {
        rejected.push(reason(
            "streaming_unsupported",
            "streaming is required but not declared",
        ));
    }
    f64::from(
        declared
            .context_limit
            .saturating_sub(required.minimum_context),
    ) / f64::from(declared.context_limit.max(1))
}

fn check_cpu_contract(
    required: &CpuComputeRequirement,
    declared: &CpuComputeContract,
    rejected: &mut Vec<MatchReason>,
) -> f64 {
    if let Some(architecture) = &required.architecture {
        if !declared.architectures.contains(architecture) {
            rejected.push(reason(
                "architecture_unsupported",
                format!("architecture {architecture} is unsupported"),
            ));
        }
    }
    if declared.logical_cores < required.minimum_logical_cores {
        rejected.push(reason(
            "cores_insufficient",
            format!(
                "{} logical cores declared, {} required",
                declared.logical_cores, required.minimum_logical_cores
            ),
        ));
    }
    if required.wasm_required && !declared.supports_wasm {
        rejected.push(reason(
            "wasm_unsupported",
            "Wasm execution is required but not declared",
        ));
    }
    f64::from(
        declared
            .logical_cores
            .saturating_sub(required.minimum_logical_cores),
    ) / f64::from(declared.logical_cores.max(1))
}

fn check_policy(job: &JobRequirement, node: &NodeOffer, rejected: &mut Vec<MatchReason>) {
    if !node.policy.allowed_capabilities.contains(&job.capability) {
        rejected.push(reason(
            "policy_capability_forbidden",
            format!("policy does not expose {}", job.capability.0),
        ));
    }
    if !node.policy.allowed_requesters.is_empty()
        && !node.policy.allowed_requesters.contains(&job.requester_id)
    {
        rejected.push(reason(
            "policy_requester_forbidden",
            format!("requester {} is not allowed", job.requester_id),
        ));
    }
    if (job.local_network_only || node.policy.local_network_only)
        && job.requester_network != node.network
    {
        rejected.push(reason(
            "policy_locality_forbidden",
            "job and node are not on the same declared network",
        ));
    }
    if node.policy.manual_consent_required && !node.policy.consent_granted {
        rejected.push(reason(
            "consent_required",
            "manual consent has not been granted",
        ));
    }
    if job.estimated_memory_mib > node.policy.max_memory_mib {
        rejected.push(reason(
            "policy_memory_limit",
            format!(
                "{} MiB exceeds policy maximum {} MiB",
                job.estimated_memory_mib, node.policy.max_memory_mib
            ),
        ));
    }
}

fn missing_capability(job: &JobRequirement, node: &NodeOffer) -> MatchResult {
    result(
        node,
        None,
        MatchStatus::Rejected,
        None,
        vec![reason(
            "capability_missing",
            format!("{} is not declared", job.capability.0),
        )],
    )
}

fn reason(code: &str, detail: impl Into<String>) -> MatchReason {
    MatchReason {
        code: code.to_owned(),
        detail: detail.into(),
    }
}

fn result(
    node: &NodeOffer,
    offer: Option<&CapabilityOffer>,
    status: MatchStatus,
    score: Option<ScoreBreakdown>,
    reasons: Vec<MatchReason>,
) -> MatchResult {
    MatchResult {
        node_id: node.node_id.clone(),
        capability_offer_id: offer.map(|item| item.id.clone()),
        status,
        score,
        reasons,
    }
}

fn compare_results(left: &MatchResult, right: &MatchResult) -> Ordering {
    status_order(&left.status)
        .cmp(&status_order(&right.status))
        .then_with(|| {
            right
                .score
                .as_ref()
                .map(|score| score.final_score)
                .partial_cmp(&left.score.as_ref().map(|score| score.final_score))
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| left.node_id.cmp(&right.node_id))
        .then_with(|| left.capability_offer_id.cmp(&right.capability_offer_id))
}

const fn status_order(status: &MatchStatus) -> u8 {
    match status {
        MatchStatus::Compatible => 0,
        MatchStatus::Unavailable => 1,
        MatchStatus::Rejected => 2,
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    let numerator = u32::try_from(numerator).unwrap_or(u32::MAX);
    let denominator = u32::try_from(denominator).unwrap_or(u32::MAX);
    f64::from(numerator) / f64::from(denominator)
}

fn round_score(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn required(value: &str, field: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::Invalid(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("invalid capability data: {0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn names(items: &[&str]) -> BTreeSet<CapabilityName> {
        items
            .iter()
            .map(|item| CapabilityName::from(*item))
            .collect()
    }

    fn llm_job() -> JobRequirement {
        JobRequirement {
            capability: CapabilityName::from("llm.inference"),
            requirement: CapabilityRequirement::LlmInference(LlmInferenceRequirement {
                model: Some("qwen3:4b".to_owned()),
                minimum_context: 16_000,
                language: Some("fr".to_owned()),
                streaming_required: true,
            }),
            workload_profile: Some("qwen3:4b/default".to_owned()),
            local_network_only: true,
            requester_id: "alice".to_owned(),
            requester_network: "home".to_owned(),
            estimated_memory_mib: 4_096,
        }
    }

    fn node(id: &str) -> NodeOffer {
        let offer_id = CapabilityOfferId::from("llm-noema");
        NodeOffer {
            node_id: id.to_owned(),
            network: "home".to_owned(),
            inventory: HardwareInventory {
                architecture: "x86_64".to_owned(),
                logical_cores: 8,
                total_memory_mib: 16_384,
                accelerator: None,
            },
            capabilities: vec![CapabilityOffer {
                id: offer_id.clone(),
                name: CapabilityName::from("llm.inference"),
                implementation: "noema".to_owned(),
                contract: CapabilityContract::LlmInference(LlmInferenceContract {
                    models: set(&["qwen3:4b"]),
                    context_limit: 32_768,
                    languages: set(&["fr", "en"]),
                    supports_streaming: true,
                }),
                concurrency: 1,
            }],
            policy: ExposurePolicy {
                allowed_capabilities: names(&["llm.inference"]),
                allowed_requesters: BTreeSet::new(),
                local_network_only: true,
                manual_consent_required: false,
                consent_granted: false,
                max_memory_mib: 8_192,
            },
            availability: CurrentAvailability {
                online: true,
                active_jobs: BTreeMap::new(),
                available_memory_mib: 8_192,
            },
            observations: vec![Observation {
                capability_offer_id: offer_id,
                workload_profile: Some("qwen3:4b/default".to_owned()),
                successful: true,
                latency_ms: 500,
                throughput: Some(ThroughputMetric::TokensPerSecond(25.0)),
            }],
        }
    }

    #[test]
    fn observations_are_scoped_to_the_concrete_offer() {
        let mut candidate = node("a");
        let second_id = CapabilityOfferId::from("llm-other");
        candidate.capabilities.push(CapabilityOffer {
            id: second_id.clone(),
            ..candidate.capabilities[0].clone()
        });
        candidate.observations.push(Observation {
            capability_offer_id: second_id,
            workload_profile: None,
            successful: false,
            latency_ms: 10_000,
            throughput: None,
        });
        let results = rank(&llm_job(), &[candidate]);
        assert_eq!(results.len(), 2);
        assert_ne!(results[0].score, results[1].score);
    }

    #[test]
    fn cpu_capability_is_matched_without_llm_fields() {
        let mut candidate = node("cpu");
        candidate.capabilities = vec![CapabilityOffer {
            id: CapabilityOfferId::from("cpu-native"),
            name: CapabilityName::from("compute.cpu"),
            implementation: "native-worker".to_owned(),
            contract: CapabilityContract::CpuCompute(CpuComputeContract {
                architectures: set(&["x86_64"]),
                logical_cores: 8,
                supports_wasm: true,
            }),
            concurrency: 2,
        }];
        candidate.policy.allowed_capabilities = names(&["compute.cpu"]);
        candidate.observations.clear();
        let job = JobRequirement {
            capability: CapabilityName::from("compute.cpu"),
            requirement: CapabilityRequirement::CpuCompute(CpuComputeRequirement {
                architecture: Some("x86_64".to_owned()),
                minimum_logical_cores: 4,
                wasm_required: true,
            }),
            workload_profile: None,
            estimated_memory_mib: 512,
            ..llm_job()
        };
        assert_eq!(rank(&job, &[candidate])[0].status, MatchStatus::Compatible);
    }

    #[test]
    fn ranking_is_deterministic_for_every_input_order() {
        let nodes = vec![node("b"), node("a"), node("c")];
        let expected = vec!["a", "b", "c"];
        for permutation in [
            vec![nodes[0].clone(), nodes[1].clone(), nodes[2].clone()],
            vec![nodes[2].clone(), nodes[0].clone(), nodes[1].clone()],
            vec![nodes[1].clone(), nodes[2].clone(), nodes[0].clone()],
        ] {
            let ids: Vec<_> = rank(&llm_job(), &permutation)
                .into_iter()
                .map(|result| result.node_id)
                .collect();
            assert_eq!(ids, expected);
        }
    }
}
