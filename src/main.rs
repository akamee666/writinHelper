use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestSystemMessageContent, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, CreateChatCompletionRequest,
};
use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    serve,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    // Get current directory to ensure we have the correct path
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let frontend_path = current_dir.join("frontend");

    println!("Serving static files from: {}", frontend_path.display());

    // Create a router for our application
    let app = Router::new()
        .route("/ws", get(handle_ws))
        .route("/", get(serve_frontend))
        // Serve static files directly from the frontend directory
        .nest_service("/static", ServeDir::new(frontend_path));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on http://{}", addr);

    // Create a TCP listener
    let listener = TcpListener::bind(addr).await.unwrap();

    // Use axum::serve with the listener and app
    serve(listener, app.into_make_service()).await.unwrap();
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
                "<html><body><h1>Error loading page</h1></body></html>".to_string(),
            )
        }
    }
}

async fn handle_ws(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(socket_handler)
}

async fn socket_handler(mut socket: WebSocket) {
    let client = Client::<OpenAIConfig>::new();

    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        println!("Received text for analysis");

        // Deserialize the incoming message
        if let Ok(input) = serde_json::from_str::<AnalysisRequest>(&text) {
            let analysis = analyze_text(&client, input).await;

            if let Ok(response) = serde_json::to_string(&analysis) {
                if socket.send(Message::Text(response.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AnalysisRequest {
    text: String,
    focus: String, // "grammar", "flow", "conciseness"
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
}

async fn analyze_text(client: &Client<OpenAIConfig>, input: AnalysisRequest) -> AnalysisResponse {
    // Create focus-specific prompts
    let focus_prompt = match input.focus.as_str() {
        "grammar" => "Focus primarily on grammatical issues, punctuation, and syntax errors.",
        "flow" => "Focus on improving sentence flow, transitional phrases, and paragraph cohesion.",
        "conciseness" => "Focus on eliminating wordiness, redundancies, and tightening language.",
        _ => "Provide balanced feedback on grammar, clarity, and style.",
    };

    let system_prompt = format!(
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
        
        Format each suggestion as a JSON object. Return an array of these objects."
    );

    let request = CreateChatCompletionRequest {
        model: "gpt-3.5-turbo-0125".to_string(),
        messages: vec![
            ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                content: ChatCompletionRequestSystemMessageContent::Text(system_prompt),
                name: None,
            }),
            ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                content: ChatCompletionRequestUserMessageContent::Text(input.text),
                name: None,
            }),
        ],
        temperature: Some(0.1),
        ..Default::default()
    };

    match client.chat().create(request).await {
        Ok(response) => {
            let content = response.choices[0]
                .message
                .content
                .clone()
                .unwrap_or_default();

            // Try to parse the JSON response
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => {
                    if let Some(suggestions_array) =
                        json.get("suggestions").and_then(|v| v.as_array())
                    {
                        let suggestions = suggestions_array
                            .iter()
                            .filter_map(|item| {
                                let suggestion =
                                    serde_json::from_value::<TextSuggestion>(item.clone()).ok()?;
                                Some(suggestion)
                            })
                            .collect();

                        AnalysisResponse { suggestions }
                    } else {
                        // Fallback if the JSON structure is unexpected
                        AnalysisResponse {
                            suggestions: vec![],
                        }
                    }
                }
                Err(_) => {
                    println!("Failed to parse JSON response");
                    AnalysisResponse {
                        suggestions: vec![],
                    }
                }
            }
        }
        Err(err) => {
            println!("OpenAI API error: {:?}", err);
            AnalysisResponse {
                suggestions: vec![],
            }
        }
    }
}

