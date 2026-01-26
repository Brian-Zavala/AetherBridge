use crate::Provider;
use async_trait::async_trait;
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct GoogleClient {
    client: Client,
    base_url: String,
}

#[async_trait]
impl Provider for GoogleClient {
    async fn generate(&self, prompt: &str) -> Result<String> {
        let payload = self.serialize_request(prompt);

        // Google internal APIs often use a form-encoded POST where `f.req` contains the JSON.
        let params = [("f.req", payload.to_string())];

        let resp = self.client.post(format!("{}/_/Gho/Request", self.base_url))
            .form(&params)
            .send()
            .await?;

        if !resp.status().is_success() {
            tracing::error!("Google API Request Failed. Status: {}", resp.status());
            return Err(anyhow!("Google API request failed: {}", resp.status()));
        }

        let text = resp.text().await?;
        tracing::debug!("Raw Google Response: {}", text);
        self.deserialize_response(&text)
    }
}

impl GoogleClient {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: "https://ide.google.com".to_string(), // Targeted endpoint
        }
    }

    /// Serializes a chat prompt into the Google "Batched JSON" format.
    /// This format is typically a nested array structure used by Google's internal APIs.
    /// Structure based on reverse-engineering of similar internal APIs (e.g., Bard/Gemini web).
    fn serialize_request(&self, prompt: &str) -> Value {
        // NOTE: This is a hypothesized structure based on common Google internal API patterns (RPCs).
        // The actual payload for Antigravity will need to be verified against network traces.
        // Usually looks like: [null, "[[[\"prompt\", ...]]]", null, "generic_rpc_method"]

        let req_payload = json!([
            [prompt],
            null,
            [] // Context/History placeholders
        ]);

        // Wrap in the outer RPC envelope
        json!([
            null,
            req_payload.to_string(),
            null,
            "boq.antigravity.AgentService.Generate" // Hypothesized RPC method name
        ])
    }



    fn deserialize_response(&self, raw_resp: &str) -> Result<String> {
        // Google responses are often "junk-prefixed" JSON (e.g., `)]}'\n` to prevent script inclusion).
        let clean_json = raw_resp.trim_start_matches(")]}'\n");

        let json: Value = serde_json::from_str(clean_json)
            .map_err(|e| anyhow!("Failed to parse Google JSON response: {}", e))?;

        // Extract the actual text content from the deep nested array
        // Expected path: [0, 2, "response_string"]
        // This path is fragile and will need adjustment based on real traffic.
        // Extract the actual text content from the deep nested array
        // Expected path: [0, 2, "response_string"]
        // This path is fragile and will need adjustment based on real traffic.
        json.get(0)
            .and_then(|v: &Value| v.get(2))
            .and_then(|v: &Value| v.as_str())
            .map(|s: &str| s.to_string())
            .ok_or_else(|| anyhow!("Could not extract text from Google response"))
    }
}
