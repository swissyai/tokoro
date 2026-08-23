use crate::platform;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const FIRECRAWL_SEARCH_URL: &str = "https://api.firecrawl.dev/v2/search";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Recommendation {
    pub label: String,
    pub model: String,
    pub intelligence: Option<f64>,
    pub tasks_per_hour: Option<f64>,
    pub size_gb: Option<f64>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Reading {
    pub machine: String,
    pub memory_gb: f64,
    pub recommendations: Vec<Recommendation>,
    pub source_url: String,
    #[serde(default)]
    pub source_method: String,
    pub fetched_unix: u64,
}

pub enum Event {
    Updated(usize),
    Failed(String),
}

pub struct Source {
    reading: Option<Reading>,
    request_rx: Option<Receiver<Result<Reading, String>>>,
    last_error: Option<String>,
}

impl Source {
    pub fn new() -> Self {
        Self {
            reading: load_cache(),
            request_rx: None,
            last_error: None,
        }
    }

    pub fn refresh(&mut self, chip: &str, memory_gb: f64) -> Result<bool, String> {
        if self.request_rx.is_some() {
            return Ok(false);
        }
        let api_key = firecrawl_key().ok_or_else(|| {
            "no public-web search adapter detected; Hugging Face downloads remain available"
                .to_string()
        })?;
        let chip = chip.trim().to_string();
        if chip.is_empty() || memory_gb <= 0.0 {
            return Err("device details are not ready yet".into());
        }
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = fetch_recommendations(&api_key, &chip, memory_gb);
            let _ = tx.send(result);
        });
        self.request_rx = Some(rx);
        self.last_error = None;
        Ok(true)
    }

    pub fn poll(&mut self) -> Option<Event> {
        let rx = self.request_rx.as_ref()?;
        match rx.try_recv() {
            Ok(Ok(reading)) => {
                let count = reading.recommendations.len();
                if let Err(error) = save_cache(&reading) {
                    self.last_error = Some(error);
                }
                self.reading = Some(reading);
                self.request_rx = None;
                Some(Event::Updated(count))
            }
            Ok(Err(error)) => {
                self.last_error = Some(error.clone());
                self.request_rx = None;
                Some(Event::Failed(error))
            }
            Err(TryRecvError::Disconnected) => {
                let error = "local.ai source worker stopped".to_string();
                self.last_error = Some(error.clone());
                self.request_rx = None;
                Some(Event::Failed(error))
            }
            Err(TryRecvError::Empty) => None,
        }
    }

    pub const fn loading(&self) -> bool {
        self.request_rx.is_some()
    }

    pub fn reading_for(&self, chip: &str, memory_gb: f64) -> Option<&Reading> {
        self.reading.as_ref().filter(|reading| {
            same_machine(&reading.machine, chip) && (reading.memory_gb - memory_gb).abs() < 1.0
        })
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn adapter_label(&self) -> &'static str {
        if firecrawl_key().is_some() {
            "Firecrawl"
        } else {
            "none detected"
        }
    }
}

fn fetch_recommendations(api_key: &str, chip: &str, memory_gb: f64) -> Result<Reading, String> {
    let memory = memory_gb.round() as u64;
    let query = format!("site:local.ai \"{chip}\" \"{memory} GB\" \"Best fit\"");
    let prompt = format!(
        "Extract each local.ai machine recommendation. Keep only {chip} machines with {memory} GB. For each recommendation, copy the label, exact model and quantization, intelligence score, speed in tasks per hour, model size in GB, and model URL. Do not infer missing values."
    );
    let payload = serde_json::json!({
        "query": query,
        "limit": 3,
        "sources": ["web"],
        "scrapeOptions": {
            "formats": [{
                "type": "json",
                "prompt": prompt,
                "schema": extraction_schema()
            }]
        }
    });
    let response = ureq::post(FIRECRAWL_SEARCH_URL)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(60))
        .send_json(payload)
        .map_err(|error| format!("Firecrawl search failed: {error}"))?;
    let response = response
        .into_json::<SearchResponse>()
        .map_err(|error| format!("Firecrawl response was not valid JSON: {error}"))?;
    reading_from_response(response, chip, memory_gb)
}

fn extraction_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "machine": {"type": "string"},
            "memory_gb": {"type": "number"},
            "recommendations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": {"type": "string"},
                        "model": {"type": "string"},
                        "intelligence": {"type": "number"},
                        "tasks_per_hour": {"type": "number"},
                        "size_gb": {"type": "number"},
                        "url": {"type": "string"}
                    },
                    "required": ["label", "model"]
                }
            }
        },
        "required": ["machine", "memory_gb", "recommendations"]
    })
}

#[derive(Deserialize)]
struct SearchResponse {
    data: SearchData,
}

#[derive(Deserialize)]
struct SearchData {
    #[serde(default)]
    web: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    url: String,
    #[serde(default)]
    json: Option<ExtractedReading>,
}

#[derive(Deserialize)]
struct ExtractedReading {
    machine: String,
    memory_gb: f64,
    #[serde(default)]
    recommendations: Vec<Recommendation>,
}

fn reading_from_response(
    response: SearchResponse,
    chip: &str,
    memory_gb: f64,
) -> Result<Reading, String> {
    let result = response
        .data
        .web
        .into_iter()
        .filter_map(|result| result.json.map(|reading| (result.url, reading)))
        .find(|(_, reading)| {
            same_machine(&reading.machine, chip)
                && (reading.memory_gb - memory_gb).abs() < 1.0
                && !reading.recommendations.is_empty()
        })
        .ok_or_else(|| {
            format!(
                "local.ai has no public {chip} / {:.0} GB recommendation",
                memory_gb
            )
        })?;

    let mut recommendations = result
        .1
        .recommendations
        .into_iter()
        .filter(|recommendation| {
            !recommendation.label.trim().is_empty() && !recommendation.model.trim().is_empty()
        })
        .take(3)
        .collect::<Vec<_>>();
    for recommendation in &mut recommendations {
        if recommendation
            .url
            .as_deref()
            .is_some_and(|url| !url.starts_with("https://local.ai/"))
        {
            recommendation.url = None;
        }
    }

    Ok(Reading {
        machine: result.1.machine,
        memory_gb: result.1.memory_gb,
        recommendations,
        source_url: result.0,
        source_method: "Firecrawl public-web search".into(),
        fetched_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })
}

fn same_machine(left: &str, right: &str) -> bool {
    normalize_machine(left) == normalize_machine(right)
}

fn normalize_machine(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn firecrawl_key() -> Option<String> {
    env::var("FIRECRAWL_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            let content = fs::read_to_string(platform::config_home().join("firecrawl.env")).ok()?;
            content.lines().find_map(|line| {
                let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
                let (name, value) = line.split_once('=')?;
                (name.trim() == "FIRECRAWL_API_KEY").then(|| {
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string()
                })
            })
        })
        .filter(|value| !value.is_empty())
}

fn cache_path() -> std::path::PathBuf {
    platform::cache_home().join("tokoro").join("local-ai.json")
}

fn load_cache() -> Option<Reading> {
    serde_json::from_str(&fs::read_to_string(cache_path()).ok()?).ok()
}

fn save_cache(reading: &Reading) -> Result<(), String> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(reading).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_a_matching_machine_and_memory_reading() {
        let response: SearchResponse = serde_json::from_value(serde_json::json!({
            "data": {"web": [{
                "url": "https://local.ai/example",
                "json": {
                    "machine": "Apple M5 Max",
                    "memory_gb": 128,
                    "recommendations": [{
                        "label": "Best fit",
                        "model": "Qwen 35B · Q4",
                        "intelligence": 68.9,
                        "tasks_per_hour": 16.0,
                        "size_gb": 22.1,
                        "url": "https://local.ai/models/qwen"
                    }]
                }
            }]}
        }))
        .expect("fixture should decode");

        let reading = reading_from_response(response, "Apple M5 Max", 128.0)
            .expect("matching reading should be accepted");
        assert_eq!(reading.recommendations.len(), 1);
        assert_eq!(reading.recommendations[0].model, "Qwen 35B · Q4");
    }

    #[test]
    fn refuses_recommendations_for_a_different_device() {
        let response: SearchResponse = serde_json::from_value(serde_json::json!({
            "data": {"web": [{
                "url": "https://local.ai/example",
                "json": {
                    "machine": "Apple M4 Max",
                    "memory_gb": 64,
                    "recommendations": [{"label": "Best fit", "model": "Model A"}]
                }
            }]}
        }))
        .expect("fixture should decode");

        assert!(reading_from_response(response, "Apple M5 Max", 128.0).is_err());
    }
}
