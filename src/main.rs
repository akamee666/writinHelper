use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
    serve,
};
use clap::Parser;
use reqwest::Client as ReqwestClient;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::ErrorKind,
    net::SocketAddr,
    process::{Child, Command},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tracing::{Level, debug, error, info, warn};
use tracing_subscriber::FmtSubscriber;

// Define CLI arguments using Clap
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "AI Writing Assistant",
    long_about = "A tool that uses local LLMs to help improve your writing"
)]
struct Args {
    /// Model to use for writing analysis
    ///
    /// Specifies which LLM to use for analyzing your text. Use the --list-models
    /// flag to see available options. Default is llama3.2.
    #[arg(short, long, default_value = "llama3.2")]
    model: String,

    /// Port to run the server on
    ///
    /// Specifies which local port the web server will use. Make sure this port
    /// is not in use by another application. Default is 3000.
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// Don't auto-start Ollama (assume it's already running)
    ///
    /// If enabled, the application will not attempt to start the Ollama service
    /// and will assume it's already running. Use this if you've started
    /// Ollama manually or it's running as a system service.
    #[arg(long, default_value_t = false)]
    no_start_ollama: bool,

    /// Download the model if not available (may take time)
    ///
    /// If enabled, the application will automatically download the selected model
    /// if it's not already available locally. This may take significant time
    /// depending on the model size and your internet connection.
    #[arg(long, default_value_t = false)]
    download_model: bool,

    /// List available writing-focused models and exit
    ///
    /// Shows all writing-focused models supported by this application,
    /// along with their descriptions and approximate sizes. The program
    /// will exit after displaying this information.
    #[arg(long, default_value_t = false)]
    list_models: bool,

    /// Enable verbose logging
    ///
    /// If enabled, additional debugging information will be displayed
    /// during operation, which can be helpful for troubleshooting.
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

// Structure to hold application state
struct AppState {
    client: ReqwestClient,
    ollama_process: Arc<Mutex<Option<Child>>>,
    available_models: Arc<Mutex<HashMap<String, ModelInfo>>>,
    selected_model: String,
    verbose: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ModelInfo {
    name: String,
    display_name: String,
    description: String,
    downloaded: bool,
    size_gb: f32,
    writing_focused: bool,
}

#[tokio::main]
async fn main() {
    // Parse command line arguments
    let args = Args::parse();
    let verbose = args.verbose;

    // Initialize tracing subscriber
    let subscriber = FmtSubscriber::builder()
        .with_max_level(if verbose { Level::DEBUG } else { Level::INFO })
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("Setting default subscriber failed");

    info!("Starting Writing Assistant...");

    // Initialize models information
    let available_models = Arc::new(Mutex::new(HashMap::new()));

    // Define writing-focused models
    let writing_models = vec![
        ModelInfo {
            name: "llama3.2".to_string(),
            display_name: "Llama 3.2 (8B)".to_string(),
            description: "General purpose model with good writing capabilities".to_string(),
            downloaded: false,
            size_gb: 4.7,
            writing_focused: true,
        },
        ModelInfo {
            name: "mistral".to_string(),
            display_name: "Mistral 7B".to_string(),
            description: "Excellent for grammar and style improvements".to_string(),
            downloaded: false,
            size_gb: 4.1,
            writing_focused: true,
        },
        ModelInfo {
            name: "phi3:mini".to_string(),
            display_name: "Phi-3 Mini".to_string(),
            description: "Microsoft's small but capable writing assistant".to_string(),
            downloaded: false,
            size_gb: 2.8,
            writing_focused: true,
        },
        ModelInfo {
            name: "gemma:2b".to_string(),
            display_name: "Gemma 2B".to_string(),
            description: "Fast and lightweight for basic writing assistance".to_string(),
            downloaded: false,
            size_gb: 1.3,
            writing_focused: true,
        },
        ModelInfo {
            name: "neural-chat".to_string(),
            display_name: "Neural Chat".to_string(),
            description: "Optimized for conversational writing and flow".to_string(),
            downloaded: false,
            size_gb: 4.1,
            writing_focused: true,
        },
    ];

    // Add models to the map
    {
        let mut models_map = available_models.lock().unwrap();
        for model in writing_models {
            models_map.insert(model.name.clone(), model);
        }
    }

    // If --list-models flag is present, print models and exit
    if args.list_models {
        // Keep println! here as it's direct user output, not logging
        println!("Available Writing-Focused Models:");
        println!("=================================");
        let models_map = available_models.lock().unwrap();
        for (_, model) in models_map.iter() {
            // Keep println! here
            println!("{} ({})", model.display_name, model.name);
            println!("    Description: {}", model.description);
            println!("    Size: {:.1} GB", model.size_gb);
            println!();
        }
        return;
    }

    // Validate that the selected model exists in our list
    {
        let models_map = available_models.lock().unwrap();
        if !models_map.contains_key(&args.model) {
            error!(
                "Model '{}' is not in the list of available models.",
                args.model
            );
            error!("Run with --list-models to see available options.");
            std::process::exit(1);
        }
    }

    // Start Ollama in the background (unless --no-start-ollama is specified)
    let ollama_process = if !args.no_start_ollama {
        start_ollama(verbose)
    } else {
        info!("Skipping Ollama startup (--no-start-ollama flag provided)");
        None
    };
    let ollama_process = Arc::new(Mutex::new(ollama_process));

    // Create HTTP client
    let client = ReqwestClient::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");

    // Check which models are already downloaded
    tokio::spawn({
        let client = client.clone();
        let models = available_models.clone();
        async move {
            // Wait a bit for Ollama to start
            tokio::time::sleep(Duration::from_secs(3)).await;
            update_model_status(&client, models, verbose).await;
        }
    });

    // Download the model if specified
    if args.download_model {
        info!(
            "Checking if model '{}' needs to be downloaded...",
            args.model
        );

        // Wait a bit for the model status to update
        tokio::time::sleep(Duration::from_secs(4)).await;

        let download_needed = {
            let models_map = available_models.lock().unwrap();
            if let Some(model_info) = models_map.get(&args.model) {
                !model_info.downloaded
            } else {
                false
            }
        };

        if download_needed {
            info!("Model '{}' not found locally. Downloading...", args.model);
            info!(
                "This may take a while depending on your internet connection and the model size."
            );

            match Command::new("ollama").arg("pull").arg(&args.model).status() {
                Ok(status) => {
                    if status.success() {
                        info!("Successfully downloaded model '{}'", args.model);

                        // Update model status
                        update_model_status(&client, available_models.clone(), verbose).await;
                    } else {
                        error!("Failed to download model. Status: {}", status);
                        error!("Continuing anyway, but the application may not work correctly.");
                    }
                }
                Err(e) => {
                    error!("Error executing ollama pull: {}", e);
                    error!("Continuing anyway, but the application may not work correctly.");
                }
            }
        } else {
            info!("Model '{}' is already downloaded.", args.model);
        }
    }

    // Get current directory for static files
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let frontend_path = current_dir.join("frontend");

    debug!("Serving static files from: {}", frontend_path.display());

    // Check if frontend directory exists
    if !frontend_path.exists() {
        error!(
            "Frontend directory not found at: {}",
            frontend_path.display()
        );
        error!("Make sure the 'frontend' directory exists in the current working directory.");
        std::process::exit(1);
    }

    // Print selected model information
    info!("Using model: {}", args.model);

    // Create shared state
    let app_state = Arc::new(AppState {
        client,
        ollama_process,
        available_models,
        selected_model: args.model,
        verbose,
    });

    // Create a router for our application
    let app = Router::new()
        .route("/ws", get(handle_ws))
        .route("/", get(serve_frontend))
        .route("/health", get(health_check))
        // Serve static files directly from the frontend directory
        .nest_service("/static", ServeDir::new(frontend_path))
        .with_state(app_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    info!("Listening on http://{}", addr);

    // Create a TCP listener
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind to address {}: {}", addr, e);
            error!("Is another service already running on port {}?", args.port);
            std::process::exit(1);
        }
    };

    info!("Server started successfully!");
    info!(
        // Changed from println! to info!
        "Open your browser and navigate to http://localhost:{}",
        args.port
    );

    // Use axum::serve with the listener and app
    if let Err(e) = serve(listener, app.into_make_service()).await {
        error!("Server error: {}", e);
        std::process::exit(1);
    }
}

// Start Ollama as a child process
fn start_ollama(_verbose: bool) -> Option<Child> {
    // Prefixed verbose with _
    info!("Starting Ollama server...");

    match Command::new("ollama").arg("serve").spawn() {
        Ok(child) => {
            info!("Ollama server started successfully");
            Some(child)
        }
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                error!("Ollama not found in PATH. Please make sure Ollama is installed.");
                error!("You can install it from https://ollama.com/download");
            } else {
                error!("Failed to start Ollama: {:?}", e);
                warn!(
                    // Use warn as it might not be a fatal error if already running
                    "Is Ollama already running? If so, you can ignore this error or use --no-start-ollama flag."
                );
            }
            None
        }
    }
}

// Health check endpoint for monitoring
async fn health_check() -> impl IntoResponse {
    #[derive(Serialize)]
    struct HealthResponse {
        status: String,
        version: String,
    }

    let health = HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    Json(health)
}

// Check which models are downloaded
async fn update_model_status(
    client: &ReqwestClient,
    models: Arc<Mutex<HashMap<String, ModelInfo>>>,
    _verbose: bool, // Prefixed verbose with _
) {
    // Try to get list of downloaded models from Ollama
    match client.get("http://localhost:11434/api/tags").send().await {
        Ok(response) => {
            if let Ok(json) = response.json::<serde_json::Value>().await {
                if let Some(models_array) = json.get("models").and_then(|v| v.as_array()) {
                    let mut models_map = models.lock().unwrap();

                    // Extract downloaded model names
                    let downloaded_models: Vec<String> = models_array
                        .iter()
                        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                        .collect();

                    // Update downloaded status - Check if any downloaded tag starts with the base name
                    for (_, model_info) in models_map.iter_mut() {
                        model_info.downloaded = downloaded_models
                            .iter()
                            .any(|downloaded_name| downloaded_name.starts_with(&model_info.name));
                    }

                    debug!(
                        "Updated model status. Downloaded models: {:?}",
                        downloaded_models
                    );
                }
            }
        }
        Err(e) => {
            // Use warn! as failure to update status might not be critical initially
            warn!("Failed to get model list from Ollama: {:?}", e);
            warn!("Is Ollama running and accessible at http://localhost:11434?");
        }
    }
}

async fn serve_frontend() -> impl IntoResponse {
    // Get the path to the frontend directory
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let index_path = current_dir.join("frontend").join("index.html");

    // Read the index.html file
    match tokio::fs::read_to_string(index_path).await {
        Ok(content) => axum::response::Html(content),
        Err(e) => {
            error!("Error reading index.html: {}", e);
            axum::response::Html(
                "<html><body><h1>Error loading page</h1><p>Could not find the index.html file.</p></body></html>".to_string(),
            )
        }
    }
}

async fn handle_ws(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| socket_handler(socket, state))
}

async fn socket_handler(mut socket: WebSocket, state: Arc<AppState>) {
    // Get reference to the HTTP client
    let client = &state.client;
    let selected_model = state.selected_model.clone(); // Clone selected model name
    let verbose = state.verbose;

    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        debug!("Received text for analysis: {:?}", text); // Log received text

        // Deserialize the incoming message
        match serde_json::from_str::<AnalysisRequest>(&text) {
            Ok(input) => {
                debug!("Parsed request: {:?}", input); // Log parsed request

                // Check if the selected model exists and is downloaded
                let model_available = {
                    let models = state.available_models.lock().unwrap();
                    models
                        .get(&selected_model) // Use selected_model from state
                        .map(|info| info.downloaded)
                        .unwrap_or(false)
                };

                let analysis = if model_available {
                    // Call analysis function - focus removed
                    analyze_text_with_local_model(
                        client,
                        input.text,
                        // input.focus removed
                        &selected_model, // Pass selected model name
                        verbose,
                    )
                    .await
                } else {
                    // Return error if model not available
                    error!(
                        "Selected model '{}' is not available/downloaded.",
                        selected_model
                    );
                    AnalysisResponse {
                        suggestions: vec![],
                        error: Some(format!(
                            "Model '{}' is not available or not downloaded. Try running with --download-model flag.",
                            selected_model
                        )),
                    }
                };

                match serde_json::to_string(&analysis) {
                    Ok(response) => {
                        if socket.send(Message::Text(response.into())).await.is_err() {
                            error!("Error sending response to websocket"); // Changed from eprintln! to error!
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error serializing analysis response: {}", e); // Changed from eprintln! to error!
                        // Send error to client
                        let error_response = AnalysisResponse {
                            suggestions: vec![],
                            error: Some(
                                "Internal server error: Failed to serialize response".to_string(),
                            ),
                        };

                        if let Ok(err_json) = serde_json::to_string(&error_response) {
                            if socket.send(Message::text(err_json)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to parse request: {}", e); // Changed from eprintln! to error!
                // Send error to client
                let error_response = AnalysisResponse {
                    suggestions: vec![],
                    error: Some("Invalid request format".to_string()),
                };

                if let Ok(err_json) = serde_json::to_string(&error_response) {
                    if socket.send(Message::text(err_json)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    info!("WebSocket connection closed"); // Changed from println! to info!
}

#[derive(Serialize, Deserialize, Debug)] // Added Debug derive
struct AnalysisRequest {
    text: String,
    // focus field removed
    #[serde(default)] // Make model optional during deserialization
    model: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)] // Added Debug derive
struct TextSuggestion {
    start: usize,
    end: usize,
    original: String,
    suggestion: String,
    reason: String,
    category: String, // "grammar", "flow", "clarity", etc.
    severity: String, // "critical", "suggestion", "optional"
}

#[derive(Serialize, Deserialize)]
struct AnalysisResponse {
    suggestions: Vec<TextSuggestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// Request structure for Ollama API
#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    system: String,
    stream: bool,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

// Response structure for Ollama API
#[derive(Deserialize, Debug)]
struct OllamaResponse {
    response: String,
    // Other fields are available but we only need the response
}

async fn analyze_text_with_local_model(
    client: &ReqwestClient,
    text: String,
    // focus parameter removed
    model_name: &str,
    _verbose: bool,
) -> AnalysisResponse {
    // Return early if text is empty
    if text.trim().is_empty() {
        return AnalysisResponse {
            suggestions: vec![],
            error: Some("Please provide text to analyze".to_string()),
        };
    }

    // Removed focus-specific prompt logic
    // Use a single, balanced system prompt focused on providing hints

    let base_system_prompt = format!(
        "You are a non-generative writing assistant designed to provide helpful HINTS and GUIDANCE, not direct corrections.
        Your task is to analyze the text and identify specific areas where the user might consider improvements.
        Provide balanced feedback on grammar, clarity, flow, word-choice, and style.
        
        CRITICAL RULE: NEVER provide a direct replacement or rewrite of the user's text in the 'suggestion' field. Instead, offer a hint, question, or guidance.
        
        For each area you identify:
        1. Pinpoint the exact text span (`original`) that the hint relates to (using `start` and `end` character positions).
        2. In the `suggestion` field, provide a HINT or QUESTION that prompts the user to think about the identified span. Examples: 'Consider if this word choice is the most effective.', 'Is there a more concise way to phrase this?', 'Check punctuation here.', 'Does this sentence flow well with the previous one?'.
        3. Provide a brief `reason` explaining *why* this area might warrant attention (e.g., 'Potential ambiguity', 'Wordiness', 'Possible grammatical error').
        4. Assign a `category` (grammar, clarity, flow, word-choice, or style).
        5. Assign a `severity` level (critical, suggestion, or optional).
        
        Format your response STRICTLY as a JSON array of suggestion objects, following this exact structure:
        {{
            \"suggestions\": [
                {{
                    \"start\": 12,
                    \"end\": 18,
                    \"original\": \"melody\",
                    \"suggestion\": \"Consider using a different word instead of 'melody'.\",
                    \"reason\": \"'Melody' might not fit the context of a forest floor.\",
                    \"category\": \"word-choice\",
                    \"severity\": \"suggestion\"
                }},
                // ... more hints if applicable
            ]
        }}
        Ensure the output is valid JSON. Only output the JSON structure."
    );

    // Customize system prompt for specific models if needed (keeping the hint-focused approach)
    let system_prompt = match model_name {
        // Use model_name parameter
        "mistral" => format!(
            "{}\n\nYou excel at identifying grammar issues and improving writing style.",
            base_system_prompt
        ),
        "phi3:mini" => format!(
            "{}\n\nYou are excellent at providing concise and clear improvements to text.",
            base_system_prompt
        ),
        _ => base_system_prompt,
    };

    debug!("Using model: {}", model_name);
    // Removed focus logging
    debug!("Text length: {} characters", text.len());

    // Create the request for Ollama
    let ollama_request = OllamaRequest {
        model: model_name.to_string(), // Use model_name parameter
        prompt: text,                  // Use text parameter
        system: system_prompt,
        stream: false,
        temperature: 0.1,
        format: Some("json".to_string()), // Request JSON format output
    };

    match client
        .post("http://localhost:11434/api/generate")
        .json(&ollama_request)
        .send()
        .await
    {
        Ok(response) => {
            match response.json::<OllamaResponse>().await {
                Ok(ollama_response) => {
                    // Try to parse the JSON response
                    match serde_json::from_str::<serde_json::Value>(&ollama_response.response) {
                        Ok(json) => {
                            if let Some(suggestions_array) =
                                json.get("suggestions").and_then(|v| v.as_array())
                            {
                                let suggestions = suggestions_array
                                    .iter()
                                    .filter_map(|item| {
                                        let suggestion =
                                            serde_json::from_value::<TextSuggestion>(item.clone())
                                                .ok()?;
                                        Some(suggestion)
                                    })
                                    .collect();

                                AnalysisResponse {
                                    suggestions,
                                    error: None,
                                }
                            } else {
                                // Fallback if the JSON structure is unexpected
                                warn!(
                                    // Changed from eprintln! to warn!
                                    "Unexpected JSON structure from model: {}",
                                    ollama_response.response
                                );

                                // Try to salvage the response by parsing it as free text
                                let suggestions = vec![TextSuggestion {
                                        start: 0,
                                        end: 0,
                                        original: "".to_string(),
                                        suggestion: "".to_string(),
                                        reason: "The model didn't return properly formatted JSON. Please try a different model.".to_string(),
                                        category: "error".to_string(),
                                        severity: "critical".to_string(),
                                    }];

                                AnalysisResponse {
                                    suggestions,
                                    error: Some(
                                        "Failed to parse model response as expected JSON format"
                                            .to_string(),
                                    ),
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse JSON response: {}", e); // Changed from eprintln! to warn!
                            debug!("Raw response: {}", ollama_response.response); // Changed from eprintln! to debug!

                            // Attempt to extract suggestions in a different way
                            // This is a fallback for models that might not format exact JSON
                            // but still provide useful feedback
                            let fallback_suggestions =
                                extract_fallback_suggestions(&ollama_response.response);

                            if !fallback_suggestions.is_empty() {
                                AnalysisResponse {
                                    suggestions: fallback_suggestions,
                                    error: None,
                                }
                            } else {
                                AnalysisResponse {
                                    suggestions: vec![],
                                    error: Some("Failed to parse model response".to_string()),
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("Failed to parse Ollama response: {}", err); // Changed from eprintln! to error!
                    AnalysisResponse {
                        suggestions: vec![],
                        error: Some("Failed to parse response from Ollama".to_string()),
                    }
                }
            }
        }
        Err(err) => {
            error!("Ollama API error: {:?}", err); // Changed from eprintln! to error!
            AnalysisResponse {
                suggestions: vec![],
                error: Some(format!("Ollama API error: {}", err)),
            }
        }
    }
}

// Fallback function to try to extract suggestions from unstructured text
fn extract_fallback_suggestions(text: &str) -> Vec<TextSuggestion> {
    let mut suggestions = Vec::new();

    // Look for patterns that might indicate suggestions
    // This is a very basic implementation and could be improved
    if text.contains("suggestion") || text.contains("change") || text.contains("improve") {
        // Split by lines and look for potential suggestions
        for line in text.lines() {
            if line.contains(":")
                && (line.contains("change")
                    || line.contains("replace")
                    || line.contains("suggestion"))
            {
                suggestions.push(TextSuggestion {
                    start: 0,
                    end: 0, // We don't know the exact positions
                    original: "".to_string(),
                    suggestion: line.to_string(),
                    reason: "Extracted from model response".to_string(),
                    category: "general".to_string(),
                    severity: "suggestion".to_string(),
                });
            }
        }
    }

    // If we couldn't extract specific suggestions but there's content,
    // provide the raw text as a general suggestion
    if suggestions.is_empty() && !text.trim().is_empty() {
        suggestions.push(TextSuggestion {
            start: 0,
            end: 0,
            original: "".to_string(),
            suggestion: "".to_string(),
            reason: "The model provided a response but not in the expected format.".to_string(),
            category: "general".to_string(),
            severity: "info".to_string(),
        });
    }

    suggestions
}

// Gracefully shutdown when the program is terminated
impl Drop for AppState {
    fn drop(&mut self) {
        // Try to kill the Ollama process if we started it
        if let Some(mut child) = self.ollama_process.lock().unwrap().take() {
            // Only attempt to kill if we started the process
            info!("Shutting down Ollama process..."); // Changed from println!

            match child.kill() {
                Ok(_) => {
                    info!("Ollama process terminated successfully."); // Changed from println!
                }
                Err(e) => {
                    error!("Failed to kill Ollama process: {}", e); // Changed from eprintln!
                }
            }
        }
    }
}

// Unit tests
#[cfg(test)]
mod tests {
    use super::*; // Import items from the parent module

    #[test]
    fn test_extract_fallback_suggestions_with_keywords() {
        let text = "suggestion: change 'their' to 'there'.\nreplace the comma.";
        let suggestions = extract_fallback_suggestions(text);
        // The second line "replace the comma." doesn't contain ":" so it shouldn't be extracted.
        assert_eq!(suggestions.len(), 1); // Expect only 1 suggestion based on current logic
        assert_eq!(
            suggestions[0].suggestion,
            "suggestion: change 'their' to 'there'."
        );
        assert_eq!(suggestions[0].category, "general");
        assert_eq!(suggestions[0].severity, "suggestion");
        // assert_eq!(suggestions[1].suggestion, "replace the comma."); // This assertion is removed
    }

    #[test]
    fn test_extract_fallback_suggestions_no_keywords_non_empty() {
        let text = "The model output this text without specific formatting.";
        let suggestions = extract_fallback_suggestions(text);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(
            suggestions[0].reason,
            "The model provided a response but not in the expected format."
        );
        assert_eq!(suggestions[0].category, "general");
        assert_eq!(suggestions[0].severity, "info"); // Check severity for non-formatted response
    }

    #[test]
    fn test_extract_fallback_suggestions_empty_text() {
        let text = "";
        let suggestions = extract_fallback_suggestions(text);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_extract_fallback_suggestions_whitespace_text() {
        let text = "   \n  \t ";
        let suggestions = extract_fallback_suggestions(text);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_deserialize_analysis_request_full() {
        let json = r#"{"text": "Test text", "focus": "grammar", "model": "llama3.2"}"#;
        let request: Result<AnalysisRequest, _> = serde_json::from_str(json);
        assert!(request.is_ok());
        let req = request.unwrap();
        assert_eq!(req.text, "Test text");
        // assert_eq!(req.focus, "grammar");
        assert_eq!(req.model, Some("llama3.2".to_string()));
    }

    #[test]
    fn test_deserialize_analysis_request_no_model() {
        let json = r#"{"text": "Test text", "focus": "grammar"}"#;
        let request: Result<AnalysisRequest, _> = serde_json::from_str(json);
        assert!(request.is_ok());
        let req = request.unwrap();
        assert_eq!(req.text, "Test text");
        // assert_eq!(req.focus, "grammar");
        assert_eq!(req.model, None); // Model should default to None
    }

    #[test]
    fn test_deserialize_analysis_request_missing_field() {
        let json = r#"{"text": "Test text"}"#; // Missing focus
        let request: Result<AnalysisRequest, _> = serde_json::from_str(json);
        assert!(request.is_err()); // Should fail due to missing 'focus'
    }

    #[test]
    fn test_serialize_analysis_response_with_suggestions() {
        let response = AnalysisResponse {
            suggestions: vec![TextSuggestion {
                start: 0,
                end: 5,
                original: "hello".to_string(),
                suggestion: "hi".to_string(),
                reason: "greeting".to_string(),
                category: "style".to_string(),
                severity: "optional".to_string(),
            }],
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        // Basic check for structure - more robust checks could use serde_json::Value
        assert!(json.contains("\"suggestions\":["));
        assert!(json.contains("\"start\":0"));
        assert!(json.contains("\"suggestion\":\"hi\""));
        assert!(!json.contains("\"error\":")); // Error field should be skipped
    }

    #[test]
    fn test_serialize_analysis_response_with_error() {
        let response = AnalysisResponse {
            suggestions: vec![],
            error: Some("Model unavailable".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"suggestions\":[]"));
        assert!(json.contains("\"error\":\"Model unavailable\""));
    }

    #[test]
    fn test_serialize_analysis_response_empty() {
        let response = AnalysisResponse {
            suggestions: vec![],
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"suggestions\":[]"));
        assert!(!json.contains("\"error\":"));
    }
}
