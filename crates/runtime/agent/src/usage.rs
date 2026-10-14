pub use nanocode_types::pricing::{CostStatus, EstimatedUsdCost, ServiceTier, UsdAmount};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct TurnUsage {
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
    estimated_cost: Option<Box<EstimatedUsdCost>>,
    cost_status: CostStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportedTurnUsage {
    pub input_tokens: u64,

    pub cached_input_tokens: u64,

    pub cache_write_input_tokens: u64,

    pub output_tokens: u64,

    pub reasoning_output_tokens: u64,

    pub total_tokens: u64,

    pub estimated_cost: Option<EstimatedUsdCost>,

    pub cost_status: CostStatus,
}

#[allow(clippy::struct_field_names)]
#[derive(Clone)]
#[doc(hidden)]
pub struct TurnUsageCounts {
    #[doc(hidden)]
    pub input_tokens: u64,
    #[doc(hidden)]
    pub cached_input_tokens: u64,
    #[doc(hidden)]
    pub cache_write_input_tokens: u64,
    #[doc(hidden)]
    pub output_tokens: u64,
    #[doc(hidden)]
    pub reasoning_output_tokens: u64,
    #[doc(hidden)]
    pub total_tokens: u64,
    #[doc(hidden)]
    pub reported: bool,
    #[doc(hidden)]
    pub rate_estimated: bool,
    #[doc(hidden)]
    pub estimated_cost: Option<EstimatedUsdCost>,
}

impl TurnUsage {
    #[must_use]
    pub fn from_reported(reported: ReportedTurnUsage) -> Self {
        Self {
            input_tokens: reported.input_tokens,
            cached_input_tokens: reported.cached_input_tokens,
            cache_write_input_tokens: reported.cache_write_input_tokens,
            output_tokens: reported.output_tokens,
            reasoning_output_tokens: reported.reasoning_output_tokens,
            total_tokens: reported.total_tokens,
            estimated_cost: reported.estimated_cost.map(Box::new),
            cost_status: reported.cost_status,
        }
    }

    #[doc(hidden)]
    pub fn from_counts(counts: TurnUsageCounts) -> Self {
        let (estimated_cost, cost_status) = if !counts.reported {
            (None, CostStatus::UsageNotReported)
        } else {
            (
                counts.estimated_cost.map(Box::new),
                if counts.rate_estimated {
                    CostStatus::EstimatedFromUsage
                } else {
                    CostStatus::ReportedByProvider
                },
            )
        };
        Self {
            input_tokens: counts.input_tokens,
            cached_input_tokens: counts.cached_input_tokens,
            cache_write_input_tokens: counts.cache_write_input_tokens,
            output_tokens: counts.output_tokens,
            reasoning_output_tokens: counts.reasoning_output_tokens,
            total_tokens: counts.total_tokens,
            estimated_cost,
            cost_status,
        }
    }

    #[must_use]
    pub const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    #[must_use]
    pub const fn cached_input_tokens(&self) -> u64 {
        self.cached_input_tokens
    }

    #[must_use]
    pub const fn cache_write_input_tokens(&self) -> u64 {
        self.cache_write_input_tokens
    }

    #[must_use]
    pub const fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    #[must_use]
    pub const fn reasoning_output_tokens(&self) -> u64 {
        self.reasoning_output_tokens
    }

    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    #[must_use]
    pub fn estimated_cost(&self) -> Option<&EstimatedUsdCost> {
        self.estimated_cost.as_deref()
    }

    #[must_use]
    pub const fn cost_status(&self) -> CostStatus {
        self.cost_status
    }
}
