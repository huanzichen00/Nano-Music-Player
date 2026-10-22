//! DeepSeek's JSON chat API, shared by user actions and the desktop artist-photo queue.
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

pub const DEFAULT_MODEL: &str = "deepseek-flash";
const ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
const SEARCH_ENDPOINT: &str = "https://api.deepseek.com/anthropic/v1/messages";
const RESPONSE_LIMIT: u64 = 1_048_576;
#[path = "ai/rate_limit.rs"]
pub(crate) mod rate_limit;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeepSeekSettings {
    pub api_key: String,
    pub model: String,
}

impl Default for DeepSeekSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: DEFAULT_MODEL.into(),
        }
    }
}

impl fmt::Debug for DeepSeekSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeepSeekSettings")
            .field("api_key", &"[redacted]")
            .field("model", &self.model)
            .finish()
    }
}

impl DeepSeekSettings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let settings: Self = if path.exists() {
            serde_json::from_slice(&fs::read(path).map_err(|_| "Cannot read DeepSeek settings")?)
                .map_err(|_| "Invalid deepseek.json")?
        } else {
            Self::default()
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.api_key.len() > 4096 || self.api_key.chars().any(char::is_control) {
            return Err("Invalid DeepSeek API key".into());
        }
        if self.model.is_empty()
            || self.model.len() > 100
            || !self
                .model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            return Err("Invalid DeepSeek model name".into());
        }
        Ok(())
    }

    /// Environment credentials take precedence and are never copied to the settings file.
    pub fn effective_key(&self) -> String {
        std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty())
            .unwrap_or_else(|| self.api_key.clone())
            .trim()
            .to_owned()
    }

    pub fn has_key(&self) -> bool {
        !self.effective_key().is_empty()
    }

    /// Create the temporary credential file with private permissions before writing its contents.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| "Cannot create DeepSeek settings directory")?;
        }
        let temporary = path.with_extension(format!("json.{}.tmp", rand::random::<u64>()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|_| "Cannot save DeepSeek settings")?;
            file.write_all(
                &serde_json::to_vec_pretty(self).map_err(|_| "Cannot encode DeepSeek settings")?,
            )
            .map_err(|_| "Cannot save DeepSeek settings")?;
            file.sync_all()
                .map_err(|_| "Cannot save DeepSeek settings")?;
            fs::rename(&temporary, path).map_err(|_| "Cannot save DeepSeek settings")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

pub struct DeepSeek {
    client: Client,
    api_key: String,
    model: String,
    endpoint: String,
}

impl DeepSeek {
    pub fn new(settings: &DeepSeekSettings) -> Result<Self, String> {
        Self::with_endpoint(settings, ENDPOINT, Duration::from_secs(120))
    }

    fn with_endpoint(
        settings: &DeepSeekSettings,
        endpoint: &str,
        timeout: Duration,
    ) -> Result<Self, String> {
        settings.validate()?;
        let api_key = settings.effective_key();
        if api_key.is_empty() {
            return Err("Configure a DeepSeek API key first".into());
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
            .redirect(Policy::none())
            .build()
            .map_err(|_| "Cannot initialize DeepSeek connection")?;
        Ok(Self {
            client,
            api_key,
            model: settings.model.clone(),
            endpoint: endpoint.into(),
        })
    }

    pub fn json(&self, system: &str, input: &Value) -> Result<Value, String> {
        if !rate_limit::ready(&self.endpoint) {
            return Err("DeepSeek rate limit reached; try again later".into());
        }
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": input.to_string()}
                ],
                "thinking": {"type": "disabled"},
                "response_format": {"type": "json_object"},
                "temperature": 0.2,
                "max_tokens": 4096,
                "stream": false
            }))
            .send()
            .map_err(|error| {
                if error.is_timeout() {
                    "DeepSeek request timed out"
                } else {
                    "Cannot connect to DeepSeek"
                }
            })?;
        let body = Self::read_response(response)?;
        let choice = body["choices"]
            .as_array()
            .filter(|choices| choices.len() == 1)
            .and_then(|choices| choices.first())
            .ok_or("DeepSeek returned an invalid response")?;
        if choice["finish_reason"] != "stop" {
            return Err("DeepSeek returned an incomplete response; nothing was saved".into());
        }
        let content = choice["message"]["content"]
            .as_str()
            .ok_or("DeepSeek returned an empty response")?;
        serde_json::from_str(content).map_err(|_| "DeepSeek returned invalid JSON".into())
    }

    /// Native server-side search; URLs in model prose never count as search evidence.
    pub fn search(&self, query: &str) -> Result<Vec<String>, String> {
        self.search_at(SEARCH_ENDPOINT, query)
    }

    fn search_at(&self, endpoint: &str, query: &str) -> Result<Vec<String>, String> {
        if !rate_limit::ready(endpoint) {
            return Err("DeepSeek rate limit reached; try again later".into());
        }
        let response = self.client.post(endpoint)
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": self.model,
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": [{"type": "text", "text": format!("Perform a web search for the query: {query}")}]}],
                "tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 1}]
            }))
            .send().map_err(|error| if error.is_timeout() { "DeepSeek request timed out" } else { "Cannot connect to DeepSeek" })?;
        let body = Self::read_response(response)?;
        let blocks = body["content"]
            .as_array()
            .ok_or("DeepSeek returned an invalid search response")?;
        let mut searched = false;
        let mut sources = Vec::new();
        let mut tool_error = None;
        for block in blocks
            .iter()
            .filter(|block| block["type"] == "web_search_tool_result")
        {
            searched = true;
            if block["content"]["type"] == "web_search_tool_result_error" {
                tool_error = Some(Self::search_error(endpoint, &block["content"]));
                continue;
            }
            let results = block["content"]
                .as_array()
                .ok_or("DeepSeek web search failed")?;
            if let Some(error) = results
                .iter()
                .find(|result| result["type"] == "web_search_tool_result_error")
            {
                tool_error = Some(Self::search_error(endpoint, error));
            }
            for result in results
                .iter()
                .filter(|result| result["type"] == "web_search_result")
            {
                if let Some(url) = result["url"].as_str().filter(|url| !url.is_empty())
                    && sources.len() < 32
                    && !sources.iter().any(|existing| existing == url)
                {
                    sources.push(url.to_owned());
                }
            }
        }
        // The model may exhaust its one-search budget or output tokens after
        // returning useful native results. Those structured URLs remain valid.
        if !sources.is_empty() {
            return Ok(sources);
        }
        if let Some(error) = tool_error {
            return Err(error);
        }
        if body["stop_reason"] != "end_turn" {
            return Err("DeepSeek returned an incomplete search response".into());
        }
        if !searched {
            return Err("DeepSeek did not perform an online search".into());
        }
        Ok(sources)
    }

    fn search_error(endpoint: &str, error: &Value) -> String {
        match error["error_code"].as_str() {
            Some("rate_limit_exceeded") => {
                rate_limit::observe(endpoint, &Default::default());
                "DeepSeek web search rate limit reached; try again later".into()
            }
            Some("max_uses_exceeded") => "DeepSeek search reached its tool-use limit".into(),
            _ => "DeepSeek web search failed".into(),
        }
    }

    fn read_response(response: reqwest::blocking::Response) -> Result<Value, String> {
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err("DeepSeek rejected the API key".into()),
            402 => return Err("DeepSeek account has insufficient balance".into()),
            429 => {
                rate_limit::observe(response.url().as_str(), response.headers());
                return Err("DeepSeek rate limit reached; try again later".into());
            }
            500..=599 => return Err("DeepSeek is temporarily unavailable".into()),
            _ => return Err("DeepSeek rejected the request; check the model setting".into()),
        }
        let mut bytes = Vec::new();
        response
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read DeepSeek response")?;
        if bytes.len() as u64 > RESPONSE_LIMIT {
            return Err("DeepSeek response is too large".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "DeepSeek returned an invalid response".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    fn mock_request(status: &str, body: &str) -> (String, thread::JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let status = status.to_owned();
        let body = body.to_owned();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let (header_end, length) = loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while request.len() < header_end + length {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            serde_json::from_slice(&request[header_end..header_end + length]).unwrap()
        });
        (endpoint, server)
    }

    fn settings() -> DeepSeekSettings {
        DeepSeekSettings {
            api_key: "test-secret".into(),
            ..Default::default()
        }
    }

    #[test]
    fn actual_rate_limits_apply_to_subsequent_requests_and_tool_budgets_are_distinct() {
        let (endpoint, server) = mock_request("429 Too Many Requests", "{}");
        let client =
            DeepSeek::with_endpoint(&settings(), &endpoint, Duration::from_secs(5)).unwrap();
        assert!(
            client
                .json("JSON", &json!({}))
                .unwrap_err()
                .contains("rate limit")
        );
        server.join().unwrap();
        // The listener is closed; this must fail locally with the same limit.
        assert!(
            client
                .json("JSON", &json!({}))
                .unwrap_err()
                .contains("rate limit")
        );
        assert!(
            client
                .search_at(&endpoint, "song")
                .unwrap_err()
                .contains("rate limit")
        );
        let (endpoint, server) = mock_request("200 OK", &json!({"stop_reason":"end_turn", "content":[
            {"type":"web_search_tool_result", "content":{"type":"web_search_tool_result_error", "error_code":"max_uses_exceeded"}}
        ]}).to_string());
        let client =
            DeepSeek::with_endpoint(&settings(), &endpoint, Duration::from_secs(5)).unwrap();
        assert_eq!(
            client.search_at(&endpoint, "song").unwrap_err(),
            "DeepSeek search reached its tool-use limit"
        );
        server.join().unwrap();
        assert!(rate_limit::ready(&endpoint));
    }

    #[test]
    fn native_results_survive_a_later_tool_budget_or_token_limit() {
        let body = json!({"stop_reason":"max_tokens", "content":[
            {"type":"web_search_tool_result", "content":[{"type":"web_search_result", "url":"https://lrclib.net/api/get/1"}]},
            {"type":"web_search_tool_result", "content":{"type":"web_search_tool_result_error", "error_code":"max_uses_exceeded"}}
        ]}).to_string();
        let (endpoint, server) = mock_request("200 OK", &body);
        let client =
            DeepSeek::with_endpoint(&settings(), &endpoint, Duration::from_secs(5)).unwrap();
        assert_eq!(
            client.search_at(&endpoint, "song").unwrap(),
            ["https://lrclib.net/api/get/1"]
        );
        server.join().unwrap();
    }

    #[test]
    fn credentials_are_redacted_and_saved_privately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deepseek.json");
        let settings = settings();
        assert!(!format!("{settings:?}").contains("test-secret"));
        settings.save(&path).unwrap();
        assert_eq!(
            DeepSeekSettings::load(&path).unwrap().api_key,
            "test-secret"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn search_requires_native_results_and_never_accepts_urls_from_model_prose() {
        for (content, stop, expected) in [
            (
                json!([{ "type": "web_search_tool_result", "content": [
                    {"type": "web_search_result", "url": "https://lrclib.net/api/get/1"},
                    {"type": "web_search_result", "url": "https://lrclib.net/api/get/1"}
                ]}]),
                "end_turn",
                true,
            ),
            (
                json!([{ "type": "text", "text": "https://lrclib.net/api/get/1" }]),
                "end_turn",
                false,
            ),
            (
                json!([{ "type": "web_search_tool_result", "content": {"type": "web_search_tool_result_error"} }]),
                "end_turn",
                false,
            ),
            (
                json!([{ "type": "web_search_tool_result", "content": [] }]),
                "max_tokens",
                false,
            ),
        ] {
            let body = json!({"content": content, "stop_reason": stop}).to_string();
            let (endpoint, server) = mock_request("200 OK", &body);
            let client =
                DeepSeek::with_endpoint(&settings(), &endpoint, Duration::from_secs(5)).unwrap();
            let result = client.search_at(&endpoint, "Song Artist lyrics");
            assert_eq!(result.is_ok(), expected);
            if expected {
                assert_eq!(result.unwrap(), ["https://lrclib.net/api/get/1"]);
            }
            let request = server.join().unwrap();
            assert_eq!(request["tools"][0]["type"], "web_search_20250305");
            assert_eq!(request["tools"][0]["max_uses"], 1);
        }
    }

    #[test]
    fn chat_uses_json_mode_and_rejects_truncated_or_failed_responses() {
        for (status, finish, expected) in [
            ("200 OK", "stop", true),
            ("200 OK", "length", false),
            ("401 Unauthorized", "stop", false),
            ("429 Too Many Requests", "stop", false),
        ] {
            let body = json!({"choices": [{"finish_reason": finish, "message": {"content": "{\"lines\":[]}"}}]}).to_string();
            let (endpoint, server) = mock_request(status, &body);
            let client =
                DeepSeek::with_endpoint(&settings(), &endpoint, Duration::from_secs(5)).unwrap();
            let result = client.json("Return JSON.", &json!({"lines": []}));
            assert_eq!(result.is_ok(), expected);
            assert!(!format!("{result:?}").contains("test-secret"));
            let request = server.join().unwrap();
            assert_eq!(request["response_format"]["type"], "json_object");
            assert_eq!(request["thinking"]["type"], "disabled");
            assert_eq!(request["stream"], false);
        }
    }

    #[test]
    fn malformed_and_oversized_responses_fail_without_exposing_their_content() {
        for body in [
            "test-secret".to_owned(),
            json!({"choices": []}).to_string(),
            json!({"choices": [{"finish_reason": "stop", "message": {"content": "test-secret"}}]})
                .to_string(),
            "x".repeat(RESPONSE_LIMIT as usize + 1),
        ] {
            let (endpoint, server) = mock_request("200 OK", &body);
            let client =
                DeepSeek::with_endpoint(&settings(), &endpoint, Duration::from_secs(5)).unwrap();
            let error = client.json("Return JSON.", &json!({})).unwrap_err();
            assert!(!error.contains("test-secret"));
            server.join().unwrap();
        }
    }
}
