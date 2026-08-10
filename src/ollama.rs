use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};

#[derive(Debug)]
pub enum OllamaRequest {
    ListModels { base_url: String },
    Generate(GenerateRequest),
}

#[derive(Debug)]
pub enum OllamaResponse {
    Models(Vec<String>),
    GeneratedChunk(String),
    Finished,
    Cancelled,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct GenerateRequest {
    pub base_url: String,
    pub model: String,
    pub system: String,
    pub prompt: String,
    pub temperature: f32,
    pub top_p: f32,
    pub repeat_penalty: f32,
    pub context_length: u32,
    pub max_output_tokens: i32,
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    name: String,
}

#[derive(Serialize)]
struct ApiGenerateRequest<'a> {
    model: &'a str,
    system: &'a str,
    prompt: &'a str,
    stream: bool,
    options: GenerateOptions,
}

#[derive(Serialize)]
struct GenerateOptions {
    temperature: f32,
    top_p: f32,
    repeat_penalty: f32,
    num_ctx: u32,
    num_predict: i32,
}

#[derive(Deserialize)]
struct ApiGenerateResponse {
    response: String,
    #[serde(default)]
    done: bool,
    error: Option<String>,
}

pub struct OllamaClient {
    sender: Sender<OllamaRequest>,
    receiver: Receiver<OllamaResponse>,
    cancelled: Arc<AtomicBool>,
}

impl OllamaClient {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<OllamaRequest>();
        let (response_tx, response_rx) = mpsc::channel::<OllamaResponse>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);

        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build();
            let Ok(client) = client else {
                let _ =
                    response_tx.send(OllamaResponse::Error("Could not create HTTP client".into()));
                return;
            };
            while let Ok(request) = request_rx.recv() {
                match request {
                    OllamaRequest::ListModels { base_url } => {
                        let result = list_models(&client, &base_url);
                        let _ = response_tx.send(result.unwrap_or_else(OllamaResponse::Error));
                    }
                    OllamaRequest::Generate(request) => {
                        if let Err(error) =
                            generate(&client, request, &response_tx, &worker_cancelled)
                        {
                            let _ = response_tx.send(OllamaResponse::Error(error));
                        }
                    }
                }
            }
        });

        Self {
            sender: request_tx,
            receiver: response_rx,
            cancelled,
        }
    }

    pub fn send(&self, request: OllamaRequest) {
        if matches!(&request, OllamaRequest::Generate(_)) {
            self.cancelled.store(false, Ordering::Relaxed);
        }
        let _ = self.sender.send(request);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn try_recv(&self) -> Option<OllamaResponse> {
        self.receiver.try_recv().ok()
    }
}

fn list_models(
    client: &reqwest::blocking::Client,
    base_url: &str,
) -> Result<OllamaResponse, String> {
    let response = client
        .get(format!("{}/api/tags", base_url.trim_end_matches('/')))
        .send()
        .map_err(connection_error)?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<TagsResponse>()
        .map_err(|e| e.to_string())?;
    Ok(OllamaResponse::Models(
        response.models.into_iter().map(|m| m.name).collect(),
    ))
}

fn generate(
    client: &reqwest::blocking::Client,
    request: GenerateRequest,
    sender: &Sender<OllamaResponse>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let body = ApiGenerateRequest {
        model: &request.model,
        system: &request.system,
        prompt: &request.prompt,
        stream: true,
        options: GenerateOptions {
            temperature: request.temperature,
            top_p: request.top_p,
            repeat_penalty: request.repeat_penalty,
            num_ctx: request.context_length,
            num_predict: request.max_output_tokens,
        },
    };
    let response = client
        .post(format!(
            "{}/api/generate",
            request.base_url.trim_end_matches('/')
        ))
        .json(&body)
        .send()
        .map_err(connection_error)?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    for line in BufReader::new(response).lines() {
        if cancelled.load(Ordering::Relaxed) {
            let _ = sender.send(OllamaResponse::Cancelled);
            return Ok(());
        }
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let chunk: ApiGenerateResponse =
            serde_json::from_str(&line).map_err(|error| error.to_string())?;
        if let Some(error) = chunk.error {
            return Err(error);
        }
        if !chunk.response.is_empty() {
            let _ = sender.send(OllamaResponse::GeneratedChunk(chunk.response));
        }
        if chunk.done {
            let _ = sender.send(OllamaResponse::Finished);
            return Ok(());
        }
    }
    let _ = sender.send(OllamaResponse::Finished);
    Ok(())
}

fn connection_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Ollama took too long to respond.".into()
    } else {
        format!(
            "Could not reach Ollama. Start it with `ollama serve` and check the server URL. ({error})"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn advanced_generation_options_match_ollama_api_names() {
        let options = GenerateOptions {
            temperature: 0.8,
            top_p: 0.92,
            repeat_penalty: 1.15,
            num_ctx: 32_768,
            num_predict: 2_048,
        };
        let json = serde_json::to_value(options).unwrap();
        assert_eq!(json["num_ctx"], 32_768);
        assert_eq!(json["num_predict"], 2_048);
        assert!((json["top_p"].as_f64().unwrap() - 0.92).abs() < 0.000_001);
        assert!((json["repeat_penalty"].as_f64().unwrap() - 1.15).abs() < 0.000_001);
    }

    #[test]
    fn model_listing_handles_a_real_http_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2_048];
            let read = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).contains("GET /api/tags"));
            let body = r#"{"models":[{"name":"qwen-test:latest"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let response = list_models(&client, &format!("http://{address}")).unwrap();
        match response {
            OllamaResponse::Models(models) => assert_eq!(models, ["qwen-test:latest"]),
            _ => panic!("expected model response"),
        }
        server.join().unwrap();
    }

    #[test]
    fn model_listing_reports_connection_failure_without_panicking() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let error = list_models(&client, &format!("http://{address}")).unwrap_err();
        assert!(error.contains("Could not reach Ollama"));
    }
}
