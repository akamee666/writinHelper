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
use std::io::ErrorKind;
use std::{
    collections::HashMap,
    net::SocketAddr,
    process::{Child, Command},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

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

    println!("Starting Writing Assistant...");
    let verbose = args.verbose;

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
        println!("Available Writing-Focused Models:");
        println!("=================================");
        let models_map = available_models.lock().unwrap();
        for (_, model) in models_map.iter() {
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
            eprintln!(
                "Error: Model '{}' is not in the list of available models.",
                args.model
            );
            eprintln!("Run with --list-models to see available options.");
            std::process::exit(1);
        }
    }

    // Start Ollama in the background (unless --no-start-ollama is specified)
    let ollama_process = if !args.no_start_ollama {
        start_ollama(verbose)
    } else {
        if verbose {
            println!("Skipping Ollama startup (--no-start-ollama flag provided)");
        }
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
        println!(
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
            println!("Model '{}' not found locally. Downloading...", args.model);
            println!(
                "This may take a while depending on your internet connection and the model size."
            );

            match Command::new("ollama").arg("pull").arg(&args.model).status() {
                Ok(status) => {
                    if status.success() {
                        println!("Successfully downloaded model '{}'", args.model);

                        // Update model status
                        update_model_status(&client, available_models.clone(), verbose).await;
                    } else {
                        eprintln!("Failed to download model. Status: {}", status);
                        eprintln!("Continuing anyway, but the application may not work correctly.");
                    }
                }
                Err(e) => {
                    eprintln!("Error executing ollama pull: {}", e);
                    eprintln!("Continuing anyway, but the application may not work correctly.");
                }
            }
        } else {
            println!("Model '{}' is already downloaded.", args.model);
        }
    }

    // Get current directory for static files
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let frontend_path = current_dir.join("frontend");

    if verbose {
        println!("Serving static files from: {}", frontend_path.display());
    }

    // Check if frontend directory exists
    if !frontend_path.exists() {
        eprintln!(
            "Error: Frontend directory not found at: {}",
            frontend_path.display()
        );
        eprintln!("Make sure the 'frontend' directory exists in the current working directory.");
        std::process::exit(1);
    }

    // Print selected model information
    println!("Using model: {}", args.model);

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
    println!("Listening on http://{}", addr);

    // Create a TCP listener
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("Failed to bind to address {}: {}", addr, e);
            eprintln!("Is another service already running on port {}?", args.port);
            std::process::exit(1);
        }
    };

    println!("Server started successfully!");
    println!(
        "Open your browser and navigate to http://localhost:{}",
        args.port
    );

    // Use axum::serve with the listener and app
    if let Err(e) = serve(listener, app.into_make_service()).await {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }
}

// Start Ollama as a child process
fn start_ollama(_verbose: bool) -> Option<Child> {
    println!("Starting Ollama server...");

    let ollama_path = if cfg!(target_os = "windows") {
        "ollama.exe"
    } else {
        "ollama"
    };

    match Command::new(ollama_path).arg("serve").spawn() {
        Ok(child) => {
            println!("Ollama server started successfully");
            Some(child)
        }
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                eprintln!("Ollama not found in PATH. Please make sure Ollama is installed.");
                eprintln!("You can install it from https://ollama.com/download");
            } else {
                eprintln!("Failed to start Ollama: {:?}", e);
                eprintln!(
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
    verbose: bool,
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

                    // Update downloaded status
                    for (_, model_info) in models_map.iter_mut() {
                        model_info.downloaded = downloaded_models.contains(&model_info.name);
                    }

                    if verbose {
                        println!(
                            "Updated model status. Downloaded models: {:?}",
                            downloaded_models
                        );
                    }
                }
            }
        }
        Err(e) => {
            if verbose {
                eprintln!("Failed to get model list: {:?}", e);
                eprintln!("Is Ollama running and accessible at http://localhost:11434?");
            }
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
            eprintln!("Error reading index.html: {}", e);
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
    let selected_model = &state.selected_model;
    let verbose = state.verbose;

    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        if verbose {
            println!("Received text for analysis");
        }

        // Deserialize the incoming message
        match serde_json::from_str::<AnalysisRequest>(&text) {
            Ok(mut input) => {
                // Override the model with the one selected via command line
                input.model = selected_model.clone();

                // Check if model exists and is downloaded
                let model_available = {
                    let models = state.available_models.lock().unwrap();
                    models
                        .get(&input.model)
                        .map(|info| info.downloaded)
                        .unwrap_or(false)
                };

                let analysis = if model_available {
                    analyze_text_with_local_model(client, input, verbose).await
                } else {
                    // Return error if model not available
                    AnalysisResponse {
                        suggestions: vec![],
                        error: Some(format!(
                            "Model '{}' is not available or not downloaded. Try running with --download-model flag.",
                            input.model
                        )),
                    }
                };

                match serde_json::to_string(&analysis) {
                    Ok(response) => {
                        if socket.send(Message::Text(response.into())).await.is_err() {
                            if verbose {
                                eprintln!("Error sending response to websocket");
                            }
                            break;
                        }
                    }
                    Err(e) => {
                        if verbose {
                            eprintln!("Error serializing analysis response: {}", e);
                        }
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
                if verbose {
                    eprintln!("Failed to parse request: {}", e);
                }
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

    if verbose {
        println!("WebSocket connection closed");
    }
}

#[derive(Serialize, Deserialize)]
struct AnalysisRequest {
    text: String,
    focus: String, // "grammar", "flow", "conciseness"
    model: String, // The model to use for analysis
}

#[derive(Serialize, Deserialize)]
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
    input: AnalysisRequest,
    verbose: bool,
) -> AnalysisResponse {
    // Return early if text is empty
    if input.text.trim().is_empty() {
        return AnalysisResponse {
            suggestions: vec![],
            error: Some("Please provide text to analyze".to_string()),
        };
    }

    // Create focus-specific prompts
    let focus_prompt = match input.focus.as_str() {
        "grammar" => "Focus primarily on grammatical issues, punctuation, and syntax errors.",
        "flow" => "Focus on improving sentence flow, transitional phrases, and paragraph cohesion.",
        "conciseness" => "Focus on eliminating wordiness, redundancies, and tightening language.",
        _ => "Provide balanced feedback on grammar, clarity, and style.",
    };

    // Model-specific system prompts
    let base_system_prompt = format!(
        "You are a non-generative writing assistant that provides context-aware suggestions.
        Your task is to analyze the text and identify specific areas that could be improved.
        {focus_prompt}
        
        Only suggest changes to existing text - NEVER generate new content or ideas.
        For each suggestion, provide:
        1. The exact text span that needs improvement (with character positions)
        2. A suggested revision for that exact span
        3. A brief reason for the suggestion
        4. A category (grammar, clarity, flow, word-choice, or style)
        5. A severity level (critical, suggestion, or optional)
        
        Format your response as a JSON array of suggestion objects, with this exact structure:
        {{
            \"suggestions\": [
                {{
                    \"start\": 0,
                    \"end\": 10,
                    \"original\": \"example text\",
                    \"suggestion\": \"better text\",
                    \"reason\": \"Explanation for the change\",
                    \"category\": \"grammar\",
                    \"severity\": \"critical\"
                }},
                ...
            ]
        }}"
    );

    // Customize system prompt for specific models if needed
    let system_prompt = match input.model.as_str() {
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

    if verbose {
        println!("Using model: {}", input.model);
        println!("Focus: {}", input.focus);
        println!("Text length: {} characters", input.text.len());
    }

    // Create the request for Ollama
    let ollama_request = OllamaRequest {
        model: input.model,
        prompt: input.text,
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
                                if verbose {
                                    eprintln!(
                                        "Unexpected JSON structure: {}",
                                        ollama_response.response
                                    );
                                }

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
                            if verbose {
                                eprintln!("Failed to parse JSON response: {}", e);
                                eprintln!("Raw response: {}", ollama_response.response);
                            }

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
                    if verbose {
                        eprintln!("Failed to parse Ollama response: {}", err);
                    }
                    AnalysisResponse {
                        suggestions: vec![],
                        error: Some("Failed to parse response from Ollama".to_string()),
                    }
                }
            }
        }
        Err(err) => {
            if verbose {
                eprintln!("Ollama API error: {:?}", err);
            }
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
            if self.verbose {
                println!("Shutting down Ollama process...");
            }

            match child.kill() {
                Ok(_) => {
                    if self.verbose {
                        println!("Ollama process terminated successfully.");
                    }
                }
                Err(e) => {
                    if self.verbose {
                        eprintln!("Failed to kill Ollama process: {}", e);
                    }
                }
            }
        }
    }
}

