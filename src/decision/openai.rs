use super::{DecisionEngine, DecisionError};
use crate::{
    cognition::AgentObservation,
    sim::{ObservationTarget, ProposedAction},
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, time::Duration};

const SYSTEM_PROMPT: &str = r#"You choose one action for a simulated character.
The observation is subjective and complete: do not invent people, places, possessions, or facts.
Prioritize urgent needs, then feasible goals whose progress is below 1.0.
Let personality shape choices: openness and impulsiveness favor exploration, agreeableness favors conversation, ambition favors work, and neuroticism favors safety and rest. Mood ranges from -1 (very negative) through 0 (neutral) to 1 (very positive); let it shape fallback choices without overriding urgent needs or feasible goals.
Beliefs are subjective estimates from witnessed behavior; weigh sociability, reliability, and hostility by confidence, never as objective facts.
The observation gives local_time, work_hours, current activities, action_affordances, and route_hints. Visible residents may be occupied; only talk to IDs listed in talk_to. Route hints are immediate legal move_to IDs toward home, work, or food; use them when pursuing those destinations. Move only to a move_to ID, talk only to a talk_to ID, and propose eat, rest, or work only when its can_* value is true. Observe only the current location or a visible agent; wait is always valid.
For talk, choose a tone grounded in the current mood, personality, relationship, and beliefs: friendly, supportive, neutral, or tense. Write natural dialogue grounded only in the current observation and relevant memories. Keep it to one printable line of at most 200 characters.
Return only one JSON object matching exactly one of these forms:
{"action":"move","destination":"location UUID"}
{"action":"talk","target":"agent UUID","tone":"friendly|supportive|neutral|tense","message":"non-empty text"}
{"action":"observe","target":{"target":"agent","id":"agent UUID"}}
{"action":"observe","target":{"target":"location","id":"location UUID"}}
{"action":"eat"}
{"action":"rest"}
{"action":"work"}
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
        let mut endpoint = Url::parse(base_url)
            .map_err(|error| DecisionError::Configuration(error.to_string()))?;
        if endpoint.scheme() != "https" && (endpoint.scheme() != "http" || !is_loopback(&endpoint))
        {
            return Err(DecisionError::Configuration(
                "endpoint must use HTTPS, or HTTP on a loopback address".into(),
            ));
        }
        if !endpoint
            .path()
            .trim_end_matches('/')
            .ends_with("/chat/completions")
        {
            let path = format!("{}/chat/completions", endpoint.path().trim_end_matches('/'));
            endpoint.set_path(&path);
        }
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        let model = model.into();
        if model.trim().is_empty() {
            return Err(DecisionError::Configuration(
                "model name cannot be empty".into(),
            ));
        }

        Ok(Self {
            client: Client::builder().timeout(timeout).build()?,
            endpoint,
            model,
            api_key: None,
            temperature: 0.0,
            reasoning_effort: None,
            max_completion_tokens: None,
            provider: None,
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
        let request = ChatRequest {
            model: &self.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: SYSTEM_PROMPT.into(),
                },
                ChatMessage {
                    role: "user",
                    content: serde_json::to_string(observation)?,
                },
            ],
            temperature: self.temperature,
            reasoning: self.reasoning_effort.map(|effort| Reasoning { effort }),
            max_completion_tokens: self.max_completion_tokens,
            provider: self.provider.as_deref().map(|provider| ProviderRouting {
                order: [provider],
                allow_fallbacks: false,
            }),
        };
        let mut request_builder = self.client.post(self.endpoint.clone());
        if let Some(api_key) = &self.api_key {
            request_builder = request_builder.bearer_auth(api_key);
        }
        let response: ChatResponse = request_builder
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let content = response
            .choices
            .into_iter()
            .next()
            .ok_or(DecisionError::MissingChoice)?
            .message
            .content;
        let action = serde_json::from_str(content.trim())?;
        if !action_is_afforded(observation, &action) {
            return Err(DecisionError::UnavailableAction);
        }
        Ok(action)
    }
}

fn action_is_afforded(observation: &AgentObservation, action: &ProposedAction) -> bool {
    let affordances = &observation.action_affordances;
    match action {
        ProposedAction::Move { destination } => affordances.move_to.contains(destination),
        ProposedAction::Talk { target, .. } => affordances.talk_to.contains(target),
        ProposedAction::Observe {
            target: ObservationTarget::Agent(target),
        } => affordances.talk_to.contains(target),
        ProposedAction::Observe {
            target: ObservationTarget::Location(target),
        } => *target == observation.current_location.id,
        ProposedAction::Eat => affordances.can_eat,
        ProposedAction::Rest => affordances.can_rest,
        ProposedAction::Work => affordances.can_work,
        ProposedAction::Wait => true,
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<Reasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
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

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::{
        ChatMessage, ChatRequest, OpenAiDecisionEngine, ReasoningEffort, action_is_afforded,
    };
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

    #[test]
    fn unavailable_actions_are_rejected_before_execution() {
        let world = World::briar_glen(42).expect("town");
        let actor = *world.agents.keys().next().expect("resident");
        let observation = perceive(&world, actor).expect("observation");

        assert!(action_is_afforded(&observation, &ProposedAction::Wait));
        assert_eq!(
            action_is_afforded(&observation, &ProposedAction::Work),
            observation.action_affordances.can_work
        );
        assert!(!action_is_afforded(
            &observation,
            &ProposedAction::Move {
                destination: observation.current_location.id,
            }
        ));
    }

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
            assert!(request.contains("Route hints are immediate legal move_to IDs"));
            assert!(request.contains("Move only to a move_to ID"));
            assert!(request.contains("when its can_* value is true"));
            assert!(request.contains(r#"\"local_time\":{\"day\":1,\"hour\":7,\"minute\":0}"#));
            assert!(
                request.contains(r#"\"work_hours\":{\"opens_at_hour\":6,\"closes_at_hour\":14}"#)
            );
            assert!(request.contains(r#"\"opening_hours\":"#));
            assert!(request.contains(r#"\"is_open\":"#));
            assert!(request.contains(r#"\"action_affordances\":{\"move_to\":["#));
            assert!(request.contains(r#"\"route_hints\":"#));
            assert!(request.contains(r#"\"can_work\":false"#));
            assert!(request.contains(r#""temperature":0.7"#));
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
            let body = r#"{"choices":[{"message":{"content":"{\"action\":\"eat\"}"}}]}"#;
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

        assert_eq!(action, ProposedAction::Eat);
        assert!(matches!(
            world.execute(actor, action),
            ActionResult::Success(_)
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
            reasoning: None,
            max_completion_tokens: None,
            provider: None,
        };
        let json = serde_json::to_string(&request).expect("request JSON");
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
