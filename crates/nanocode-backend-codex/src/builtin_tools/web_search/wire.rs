use crate::client::responses::ResponseItem;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub(super) struct SearchRequest<'a> {
    pub(super) id: &'a str,
    pub(super) model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) input: Option<&'a [ResponseItem]>,
    pub(super) commands: &'a SearchCommands,
    pub(super) settings: SearchSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
pub(super) struct SearchCommands {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) search_query: Option<Vec<SearchQuery>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) image_query: Option<Vec<SearchQuery>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) open: Option<Vec<OpenOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) click: Option<Vec<ClickOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) find: Option<Vec<FindOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) screenshot: Option<Vec<ScreenshotOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) finance: Option<Vec<FinanceOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) weather: Option<Vec<WeatherOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sports: Option<Vec<SportsOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) time: Option<Vec<TimeOperation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) response_length: Option<SearchResponseLength>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(super) struct SearchQuery {
    pub(super) q: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) recency: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) domains: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(super) struct OpenOperation {
    pub(super) ref_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) lineno: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub(super) struct ClickOperation {
    pub(super) ref_id: String,

    pub(super) id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub(super) struct FindOperation {
    pub(super) ref_id: String,

    pub(super) pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub(super) struct ScreenshotOperation {
    pub(super) ref_id: String,

    pub(super) pageno: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(super) struct FinanceOperation {
    pub(super) ticker: String,

    pub(super) r#type: FinanceAssetType,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) market: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum FinanceAssetType {
    Equity,
    Fund,
    Crypto,
    Index,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(super) struct WeatherOperation {
    pub(super) location: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) start: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub(super) struct SportsOperation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool: Option<SportsToolName>,

    pub(super) r#fn: SportsFunction,

    pub(super) league: SportsLeague,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) team: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) opponent: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) date_from: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) date_to: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) num_games: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) locale: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum SportsToolName {
    Sports,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum SportsFunction {
    Schedule,
    Standings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum SportsLeague {
    Nba,
    Wnba,
    Nfl,
    Nhl,
    Mlb,
    Epl,
    Ncaamb,
    Ncaawb,
    Ipl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub(super) struct TimeOperation {
    pub(super) utc_offset: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum SearchResponseLength {
    Short,
    Medium,
    Long,
}

#[derive(Serialize)]
pub(super) struct SearchSettings {
    pub(super) allowed_callers: [&'static str; 1],
    pub(super) external_web_access: bool,
}

#[derive(Deserialize)]
pub(super) struct SearchResponse {
    #[serde(default, rename = "encrypted_output")]
    pub(super) _encrypted_output: Option<String>,
    pub(super) output: String,
    #[serde(default)]
    pub(super) results: Option<Vec<Value>>,
}
