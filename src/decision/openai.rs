use super::{DecisionEngine, DecisionError};
use crate::{cognition::AgentObservation, sim::ProposedAction};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, time::Duration};

const SYSTEM_PROMPT: &str = r#"You choose one action for a simulated character.
The observation is subjective and complete: do not invent people, places, possessions, or facts.
Return only one JSON object matching exactly one of these forms:
{"action":"move","destination":"location UUID"}
{"action":"talk","target":"agent UUID","message":"non-empty text"}
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
            temperature: 0,
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
        Ok(serde_json::from_str(content.trim())?)
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
    temperature: u8,
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
    use super::OpenAiDecisionEngine;
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
        .expect("API key");
        let action = engine.decide(&observation).await.expect("action");

        assert_eq!(action, ProposedAction::Eat);
        assert!(matches!(
            world.execute(actor, action),
            ActionResult::Success(_)
        ));
        server.join().expect("server");
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
