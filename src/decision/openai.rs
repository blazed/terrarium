use super::{DecisionEngine, DecisionError};
use crate::{cognition::AgentObservation, sim::ProposedAction};
use reqwest::{Client, Url};
use serde::Serialize;
use serde_json::Value;
use std::{net::IpAddr, time::Duration};

const SYSTEM_PROMPT: &str = r#"You choose one action for a simulated character.
The observation is subjective and complete: do not invent people, places, possessions, or facts.
Prioritize urgent needs, then feasible active goals. Each goal has a concrete typed target, integer progress and required counts, and an expiry tick; act on the exact target rather than merely matching its broad kind. Marketplace businesses expose an offering, price, stock, and cash. Purchased meals, supplies, repairs, and medicine enter your bounded inventory; consume or use them later through a true can_* affordance. Medicine is for injury or symptomatic Briar fever. SeekTreatment is only legal at the clinic, charges its price, consumes clinic stock, and is appropriate for serious injury or sickness. Civic services apply immediately. Prefer owned reserves during shortages, and use safety items more readily during storms. Purchase only when can_purchase is true; otherwise follow the nearest affordable stocked route hint or work at an open solvent workplace. Every workplace shift replenishes stock.
React to town_event when present: shelter at home during storms, socialize during festivals, expect reduced production during shortages, and favor work during market days. remaining_ticks says how long the condition lasts.
Let personality shape choices: openness and impulsiveness favor exploration, agreeableness favors conversation, ambition favors work, and neuroticism favors safety and rest. Mood ranges from -1 (very negative) through 0 (neutral) to 1 (very positive); let it shape fallback choices without overriding urgent needs or feasible goals.
Beliefs are subjective estimates from witnessed behavior and credible rumors; weigh sociability, reliability, and hostility by confidence, never as objective facts. Rumors identify who passed along a historical report, its retelling depth, and confidence; treat them as hearsay, not objective truth.
The observation gives local_time, workplace opening_hours, your current activity and intention, action_affordances, and route_hints. Visible residents may be occupied; only talk to IDs listed in talk_to. Confront only an exact target and claim pair listed in confront, and only when acting on that known rumor. Each route hint has a final destination and immediate legal next_hop. Use pursue for multi-step travel, purchases, rest, or work so the simulation can continue it without another decision. Move only to a move_to ID, talk only to a talk_to ID, and propose purchase, consume_meal, use_supplies, use_repair_kit, use_medicine, seek_treatment, rest, or work only when its can_* value is true. Observe only the current location or a visible agent; wait is always valid.
For talk, choose a tone grounded in the current mood, personality, relationship, and beliefs: friendly, supportive, neutral, or tense. Write natural dialogue grounded only in the current observation, relevant memories, beliefs, and rumors. Keep it to one printable line of at most 200 characters.
Return only one JSON object matching exactly one of these forms:
{"action":"move","destination":"location UUID"}
{"action":"talk","target":"agent UUID","tone":"friendly|supportive|neutral|tense","message":"non-empty text"}
{"action":"confront","target":"agent UUID","claim":"event UUID"}
{"action":"observe","target":{"target":"agent","id":"agent UUID"}}
{"action":"observe","target":{"target":"location","id":"location UUID"}}
{"action":"purchase"}
{"action":"consume_meal"}
{"action":"use_supplies"}
{"action":"use_repair_kit"}
{"action":"use_medicine"}
{"action":"seek_treatment"}
{"action":"rest"}
{"action":"work"}
{"action":"pursue","intention":{"goal":"visit","destination":"location UUID"}}
{"action":"pursue","intention":{"goal":"purchase","destination":"route hint destination UUID"}}
{"action":"pursue","intention":{"goal":"rest"}}
{"action":"pursue","intention":{"goal":"work"}}
{"action":"pursue","intention":{"goal":"seek_treatment"}}
{"action":"pursue","intention":{"goal":"talk","target":"agent UUID","tone":"friendly|supportive|neutral|tense","message":"non-empty text"}}
{"action":"wait"}
The simulation validates your proposal and remains authoritative."#;

pub struct OpenAiDecisionEngine {
    client: Client,
    endpoint: Url,
    model: String,
    api_key: Option<String>,
    temperature: f32,
    reasoning_effort: Option<ReasoningEffort>,
    max_completion_tokens: Option<u32>,
    provider: Option<String>,
    api: OpenAiApi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiApi {
    ChatCompletions,
    Responses,
}

impl std::str::FromStr for OpenAiApi {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chat" => Ok(Self::ChatCompletions),
            "responses" => Ok(Self::Responses),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl std::str::FromStr for ReasoningEffort {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::Xhigh),
            "max" => Ok(Self::Max),
            _ => Err(()),
        }
    }
}

impl OpenAiDecisionEngine {
    pub fn new(
        base_url: &str,
        model: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, DecisionError> {
        Self::new_with_api(base_url, model, timeout, OpenAiApi::ChatCompletions)
    }

    pub fn new_with_api(
        base_url: &str,
        model: impl Into<String>,
        timeout: Duration,
        api: OpenAiApi,
    ) -> Result<Self, DecisionError> {
        let mut endpoint = Url::parse(base_url)
            .map_err(|error| DecisionError::Configuration(error.to_string()))?;
        if endpoint.scheme() != "https" && (endpoint.scheme() != "http" || !is_loopback(&endpoint))
        {
            return Err(DecisionError::Configuration(
                "endpoint must use HTTPS, or HTTP on a loopback address".into(),
            ));
        }
        let suffix = match api {
            OpenAiApi::ChatCompletions => "chat/completions",
            OpenAiApi::Responses => "responses",
        };
        let base_path = endpoint
            .path()
            .trim_end_matches('/')
            .strip_suffix("/chat/completions")
            .or_else(|| {
                endpoint
                    .path()
                    .trim_end_matches('/')
                    .strip_suffix("/responses")
            })
            .unwrap_or_else(|| endpoint.path().trim_end_matches('/'));
        endpoint.set_path(&format!("{base_path}/{suffix}"));
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        let model = model.into();
        if model.trim().is_empty() {
            return Err(DecisionError::Configuration(
                "model name cannot be empty".into(),
            ));
        }

        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(timeout)
                .build()?,
            endpoint,
            model,
            api_key: None,
            temperature: 0.0,
            reasoning_effort: None,
            max_completion_tokens: None,
            provider: None,
            api,
        })
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Result<Self, DecisionError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(DecisionError::Configuration(
                "API key cannot be empty".into(),
            ));
        }
        self.api_key = Some(api_key);
        Ok(self)
    }

    pub fn with_temperature(mut self, temperature: f32) -> Result<Self, DecisionError> {
        if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
            return Err(DecisionError::Configuration(
                "temperature must be between 0 and 2".into(),
            ));
        }
        self.temperature = temperature;
        Ok(self)
    }

    pub fn with_reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.reasoning_effort = Some(effort);
        self
    }

    pub fn with_max_completion_tokens(mut self, tokens: u32) -> Result<Self, DecisionError> {
        if tokens == 0 {
            return Err(DecisionError::Configuration(
                "maximum completion tokens must be greater than zero".into(),
            ));
        }
        self.max_completion_tokens = Some(tokens);
        Ok(self)
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Result<Self, DecisionError> {
        let provider = provider.into();
        if provider.trim().is_empty() {
            return Err(DecisionError::Configuration(
                "provider cannot be empty".into(),
            ));
        }
        self.provider = Some(provider);
        Ok(self)
    }
}

impl DecisionEngine for OpenAiDecisionEngine {
    async fn decide(
        &mut self,
        observation: &AgentObservation,
    ) -> Result<ProposedAction, DecisionError> {
        let input = serde_json::to_string(observation)?;
        let provider = self.provider.as_deref().map(|provider| ProviderRouting {
            order: [provider],
            allow_fallbacks: false,
        });
        let mut request_builder = self.client.post(self.endpoint.clone());
        if let Some(api_key) = &self.api_key {
            request_builder = request_builder.bearer_auth(api_key);
        }
        let content = match self.api {
            OpenAiApi::ChatCompletions => {
                let request = ChatRequest {
                    model: &self.model,
                    messages: [
                        ChatMessage {
                            role: "system",
                            content: SYSTEM_PROMPT.into(),
                        },
                        ChatMessage {
                            role: "user",
                            content: input,
                        },
                    ],
                    temperature: self.temperature,
                    stream: true,
                    reasoning: self.reasoning_effort.map(|effort| Reasoning { effort }),
                    max_completion_tokens: self.max_completion_tokens,
                    provider,
                };
                response_content(request_builder.json(&request).send().await?, self.api).await?
            }
            OpenAiApi::Responses => {
                let request = ResponsesRequest {
                    model: &self.model,
                    instructions: SYSTEM_PROMPT,
                    input,
                    temperature: self.temperature,
                    stream: true,
                    reasoning: self.reasoning_effort.map(|effort| Reasoning { effort }),
                    max_output_tokens: self.max_completion_tokens,
                    provider,
                };
                response_content(request_builder.json(&request).send().await?, self.api).await?
            }
        };
        Ok(serde_json::from_str(content.trim())?)
    }
}

async fn response_content(
    response: reqwest::Response,
    api: OpenAiApi,
) -> Result<String, DecisionError> {
    let body = response.error_for_status()?.text().await?;
    if body.lines().any(|line| line.starts_with("data:")) {
        return parse_stream(&body, api);
    }

    let response: Value = serde_json::from_str(&body)?;
    let content = match api {
        OpenAiApi::ChatCompletions => response.pointer("/choices/0/message/content"),
        OpenAiApi::Responses => response["output"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|output| output["content"].as_array().into_iter().flatten())
            .find(|content| content["type"] == "output_text")
            .map(|content| &content["text"]),
    };
    content
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(DecisionError::MissingChoice)
}

fn parse_stream(body: &str, api: OpenAiApi) -> Result<String, DecisionError> {
    let mut output = String::new();
    for data in body
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .filter(|data| !data.is_empty() && *data != "[DONE]")
    {
        let event: Value = serde_json::from_str(data)?;
        let delta = match api {
            OpenAiApi::ChatCompletions => event
                .pointer("/choices/0/delta/content")
                .or_else(|| event.pointer("/choices/0/message/content"))
                .and_then(Value::as_str),
            OpenAiApi::Responses => (event["type"] == "response.output_text.delta")
                .then(|| event["delta"].as_str())
                .flatten(),
        };
        if let Some(delta) = delta {
            output.push_str(delta);
        }
    }
    (!output.is_empty())
        .then_some(output)
        .ok_or(DecisionError::MissingChoice)
}

fn is_loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage; 2],
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<Reasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderRouting<'a>>,
}

#[derive(Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    instructions: &'static str,
    input: String,
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<Reasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderRouting<'a>>,
}

#[derive(Serialize)]
struct ProviderRouting<'a> {
    order: [&'a str; 1],
    allow_fallbacks: bool,
}

#[derive(Serialize)]
struct Reasoning {
    effort: ReasoningEffort,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[cfg(test)]
mod tests {
    use super::{ChatMessage, ChatRequest, OpenAiApi, OpenAiDecisionEngine, ReasoningEffort};
    use crate::{
        cognition::perceive,
        decision::{DecisionEngine, DecisionError},
        sim::{ActionResult, ProposedAction, World},
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };
    #[tokio::test]
    async fn local_response_executes_through_the_world() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
            assert!(request.contains("self_description"));
            assert!(request.contains("progress"));
            assert!(request.contains("relevant memories"));
            assert!(request.contains("200 characters"));
            assert!(request.contains("friendly|supportive|neutral|tense"));
            assert!(request.contains("Mood ranges from -1"));
            assert!(request.contains("Beliefs are subjective estimates"));
            assert!(request.contains("treat them as hearsay"));
            assert!(request.contains("Confront only an exact target and claim pair"));
            assert!(request.contains(r#"{\"action\":\"confront\""#));
            assert!(request.contains(r#"\"rumors\":"#));
            assert!(request.contains(r#"\"confront\":"#));
            assert!(request.contains("Each goal has a concrete typed target"));
            assert!(request.contains(r#"\"required\":"#));
            assert!(request.contains(r#"\"expires_at\":"#));
            assert!(request.contains("Each route hint has a final destination"));
            assert!(request.contains("Use pursue for multi-step travel"));
            assert!(request.contains(r#"\"destination\":"#));
            assert!(request.contains(r#"\"next_hop\":"#));
            assert!(request.contains("Move only to a move_to ID"));
            assert!(request.contains("when its can_* value is true"));
            assert!(request.contains(r#"\"local_time\":{\"day\":1,\"hour\":7,\"minute\":0}"#));
            assert!(request.contains(r#"\"opening_hours\":"#));
            assert!(request.contains(r#"\"is_open\":"#));
            assert!(request.contains(r#"\"action_affordances\":{\"move_to\":["#));
            assert!(request.contains(r#"\"town_event\":"#));
            assert!(request.contains(r#"\"inventory\":"#));
            assert!(request.contains(r#"\"can_consume_meal\":"#));
            assert!(request.contains(r#"\"can_use_medicine\":"#));
            assert!(request.contains(r#"\"can_seek_treatment\":"#));
            assert!(request.contains("use_medicine"));
            assert!(request.contains("seek_treatment"));
            assert!(request.contains(r#"\"route_hints\":"#));
            assert!(request.contains(r#"\"can_work\":false"#));
            assert!(request.contains(r#"\"balance\":20"#));
            assert!(request.contains(r#"\"offering\":\"meal\""#));
            assert!(request.contains(r#"\"price\":5"#));
            assert!(request.contains(r#"\"cash\":100"#));
            assert!(request.contains(r#"\"wages_paid\":0"#));
            assert!(request.contains(r#""temperature":0.7"#));
            assert!(request.contains(r#""stream":true"#));
            assert!(request.contains(r#""reasoning":{"effort":"high"}"#));
            assert!(request.contains(r#""max_completion_tokens":512"#));
            assert!(
                request.contains(r#""provider":{"order":["Anthropic"],"allow_fallbacks":false}"#)
            );
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer test-secret")
            );
            let body = r#"{"choices":[{"message":{"content":"{\"action\":\"pursue\",\"intention\":{\"goal\":\"rest\"}}"}}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response");
        });

        let mut world = World::briar_glen(42).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let observation = perceive(&world, actor).expect("observation");
        let mut engine = OpenAiDecisionEngine::new(
            &format!("http://{address}/v1"),
            "test-model",
            Duration::from_secs(2),
        )
        .expect("engine")
        .with_api_key("test-secret")
        .expect("API key")
        .with_temperature(0.7)
        .expect("temperature")
        .with_reasoning_effort(ReasoningEffort::High)
        .with_max_completion_tokens(512)
        .expect("token limit")
        .with_provider("Anthropic")
        .expect("provider");
        let action = engine.decide(&observation).await.expect("action");

        assert_eq!(
            action,
            ProposedAction::Pursue {
                intention: crate::sim::IntentionGoal::Rest,
            }
        );
        assert!(matches!(
            world.execute(actor, action),
            ActionResult::Success(_)
        ));
        server.join().expect("server");
    }

    #[tokio::test]
    async fn responses_api_uses_its_request_and_response_shapes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /v1/responses HTTP/1.1"));
            assert!(request.contains(r#""instructions":"You choose one action"#));
            assert!(request.contains(r#""input":"{\"tick\""#));
            assert!(request.contains(r#""max_output_tokens":256"#));
            assert!(!request.contains("max_completion_tokens"));
            assert!(request.contains(r#""stream":true"#));
            let body = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"{\\\"action\\\":\\\"purchase\\\"}\"}\n\ndata: [DONE]\n\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response");
        });

        let world = World::briar_glen(42).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let observation = perceive(&world, actor).expect("observation");
        let mut engine = OpenAiDecisionEngine::new_with_api(
            &format!("http://{address}/v1"),
            "test-model",
            Duration::from_secs(2),
            OpenAiApi::Responses,
        )
        .expect("engine")
        .with_max_completion_tokens(256)
        .expect("token limit");

        assert_eq!(
            engine.decide(&observation).await.expect("action"),
            ProposedAction::Purchase
        );
        server.join().expect("server");
    }

    #[tokio::test]
    async fn active_streams_can_outlive_the_inactivity_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            )
            .expect("headers");
            for delta in ["{\"action\":", "\"purchase\"", "}"] {
                let event = serde_json::json!({"choices": [{"delta": {"content": delta}}]});
                write!(stream, "data: {event}\n\n").expect("event");
                stream.flush().expect("flush");
                thread::sleep(Duration::from_millis(100));
            }
            write!(stream, "data: [DONE]\n\n").expect("done");
        });

        let world = World::briar_glen(42).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let observation = perceive(&world, actor).expect("observation");
        let mut engine = OpenAiDecisionEngine::new(
            &format!("http://{address}/v1"),
            "test-model",
            Duration::from_millis(200),
        )
        .expect("engine");

        assert_eq!(
            engine.decide(&observation).await.expect("action"),
            ProposedAction::Purchase
        );
        server.join().expect("server");
    }

    #[tokio::test]
    async fn inactive_streams_time_out() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request");
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            )
            .expect("headers");
            stream.flush().expect("flush");
            thread::sleep(Duration::from_millis(120));
        });

        let world = World::briar_glen(42).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let observation = perceive(&world, actor).expect("observation");
        let mut engine = OpenAiDecisionEngine::new(
            &format!("http://{address}/v1"),
            "test-model",
            Duration::from_millis(50),
        )
        .expect("engine");

        assert!(matches!(
            engine.decide(&observation).await,
            Err(DecisionError::Http(_))
        ));
        server.join().expect("server");
    }

    #[test]
    fn optional_generation_fields_are_omitted_by_default() {
        let request = ChatRequest {
            model: "model",
            messages: [
                ChatMessage {
                    role: "system",
                    content: "system".into(),
                },
                ChatMessage {
                    role: "user",
                    content: "user".into(),
                },
            ],
            temperature: 0.0,
            stream: true,
            reasoning: None,
            max_completion_tokens: None,
            provider: None,
        };
        let json = serde_json::to_string(&request).expect("request JSON");
        assert!(json.contains(r#""stream":true"#));
        assert!(!json.contains("reasoning"));
        assert!(!json.contains("max_completion_tokens"));
        assert!(!json.contains("provider"));
    }

    #[test]
    fn insecure_remote_endpoints_and_empty_keys_are_rejected() {
        assert!(matches!(
            OpenAiDecisionEngine::new("http://example.com/v1", "model", Duration::from_secs(1)),
            Err(DecisionError::Configuration(_))
        ));
        assert!(
            OpenAiDecisionEngine::new("https://example.com/v1", "model", Duration::from_secs(1))
                .expect("HTTPS endpoint")
                .with_api_key("  ")
                .is_err()
        );
        assert!(
            OpenAiDecisionEngine::new("https://example.com/v1", "model", Duration::from_secs(1))
                .expect("HTTPS endpoint")
                .with_temperature(2.1)
                .is_err()
        );
        assert!(
            OpenAiDecisionEngine::new("https://example.com/v1", "model", Duration::from_secs(1))
                .expect("HTTPS endpoint")
                .with_max_completion_tokens(0)
                .is_err()
        );
        assert!(
            OpenAiDecisionEngine::new("https://example.com/v1", "model", Duration::from_secs(1))
                .expect("HTTPS endpoint")
                .with_provider("  ")
                .is_err()
        );
    }

    fn read_request(stream: &mut impl Read) -> String {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let read = stream.read(&mut buffer).expect("read");
            assert_ne!(read, 0, "request ended before its body");
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let body_start = header_end + 4;
            let headers = std::str::from_utf8(&request[..header_end]).expect("headers");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .expect("content length")
                .parse::<usize>()
                .expect("numeric content length");
            if request.len() >= body_start + content_length {
                return String::from_utf8(request).expect("UTF-8 request");
            }
        }
    }
}
