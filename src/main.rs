use actix_multipart::Multipart;
use actix_files::Files;
use futures_util::stream::StreamExt;
use async_stream::stream;
use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::env;
use dotenv::dotenv;
use reqwest::Client;
use uuid::Uuid;
use std::sync::{Arc, Mutex};
use comrak::{markdown_to_html, ComrakOptions};

use futures_util::TryStreamExt;
use std::time::Duration;

#[derive(Deserialize, Serialize)]
struct TextInput {
    text: String,
}

#[derive(Clone)]
struct AppState {
    global_context: Arc<Mutex<String>>,
}

#[derive(Deserialize, Serialize, Clone)]
struct FormData {
    job_title: String,
    task_title: String,
    task_description: String,
    additional_data: Option<String>,
    mood: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct BugHistoryEntry {
    job_title: String,
    task_title: String,
    task_description: String,
}

const TIMEOUT_DURATION: Duration = Duration::from_secs(30);

#[post("/transcribe_audio")]
async fn transcribe_audio(mut payload: Multipart) -> impl Responder {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");

    let mut file_data = vec![];
    while let Ok(Some(mut field)) = payload.try_next().await {
        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(_) => return HttpResponse::InternalServerError().body("Error processing file upload."),
            };
            file_data.extend_from_slice(&data);
        }
    }

    let client = Client::builder()
        .timeout(TIMEOUT_DURATION)
        .build()
        .expect("Failed to build HTTP client");

    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(
            reqwest::multipart::Form::new()
                .text("model", "whisper-1")
                .part("file", reqwest::multipart::Part::bytes(file_data)
                    .file_name(format!("{}.mp3", Uuid::new_v4()))
                    .mime_str("audio/mpeg")
                    .unwrap())
        )
        .send()
        .await;

    match response {
        Ok(res) => match res.json::<serde_json::Value>().await {
            Ok(transcription_json) => {
                let transcription = transcription_json["text"].as_str().unwrap_or("No transcription available.");
                HttpResponse::Ok().json(serde_json::json!({
                    "transcription": transcription
                }))
            }
            Err(_) => HttpResponse::InternalServerError().body("Error parsing transcription response."),
        },
        Err(_) => HttpResponse::InternalServerError().body("Error sending transcription request."),
    }
}

#[post("/generate_audio")]
async fn generate_audio(text_input: web::Json<TextInput>) -> impl Responder {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");

    let client = Client::new();
    let api_url = "https://api.openai.com/v1/audio/speech";

    let response = client
        .post(api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "model": "tts-1",
            "voice": "alloy",
            "input": text_input.text
        }))
        .send()
        .await;

    if response.is_err() {
        return HttpResponse::InternalServerError().body("Error generating audio");
    }

    let mut response = response.unwrap();
    let audio_stream = stream! {
        while let Some(chunk) = response.chunk().await.unwrap() {
            yield Ok::<_, actix_web::Error>(web::Bytes::from(chunk));
        }
    };

    HttpResponse::Ok()
        .content_type("audio/mpeg")
        .streaming(audio_stream)
}

#[post("/update-context")]
async fn update_context(state: web::Data<AppState>, form: web::Form<TextInput>) -> impl Responder {
    let mut context = state.global_context.lock().unwrap();
    *context = form.text.clone();

    println!("Updated global context: {:?}", form.text);

    HttpResponse::Ok().body("Context updated.")
}

#[post("/search-ai")]
async fn search_ai(state: web::Data<AppState>, form: web::Json<TextInput>) -> impl Responder {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");

    let client = Client::new();
    let search_query = form.text.clone();
    let messages = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are an AI assistant that provides quick, concise, and factual information based on the user's query."
        }),
        serde_json::json!({
            "role": "user",
            "content": format!("Please provide a concise explanation or relevant information about: {}", search_query),
        })
    ];

    let payload = serde_json::json!({
        "model": "gpt-4",
        "messages": messages,
        "max_tokens": 80,  // Limit the response to a short summary
        "temperature": 0.7,
    });

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await;

    match response {
        Ok(res) => match res.json::<serde_json::Value>().await {
            Ok(result) => {
                let search_result = result["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("No relevant information found.")
                    .trim()
                    .to_string();

                HttpResponse::Ok().json(serde_json::json!({ "result": search_result }))
            }
            Err(_) => HttpResponse::InternalServerError().body("Error parsing response from GPT-4."),
        },
        Err(_) => HttpResponse::InternalServerError().body("Error calling GPT-4 API."),
    }
}


#[get("/get-ai-results")]
async fn get_ai_results(state: web::Data<AppState>) -> impl Responder {
    let context = state.global_context.lock().unwrap().clone();

    if context.is_empty() {
        return HttpResponse::BadRequest().body("Context is empty.");
    }
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    let client = Client::new();
    let messages = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are a writing assistant that provides suggestions for the next line of a document."
        }),
        serde_json::json!({
            "role": "user",
            "content": format!("The document is as follows:\n\n{}\n\nPlease suggest the next line.", context),
        })
    ];

    let payload = serde_json::json!({
        "model": "gpt-4",
        "messages": messages,
        "max_tokens": 50,  // Limit response to only a single suggestion
        "temperature": 1.0,
    });
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await;

    match response {
        Ok(res) => match res.json::<serde_json::Value>().await {
            Ok(result) => {
                // Extract the suggestion from the GPT-4 response
                let suggestion = result["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("No suggestion available.")
                    .trim()
                    .to_string();

                // Return the suggestion as plain text
                HttpResponse::Ok().body(suggestion)
            }
            Err(_) => HttpResponse::InternalServerError().body("Error parsing response from GPT-4."),
        },
        Err(_) => HttpResponse::InternalServerError().body("Error calling GPT-4 API."),
    }
}

#[get("/")]
async fn home() -> impl Responder {
    let html = include_str!("templates/home.html");
    HttpResponse::Ok()
        .content_type("text/html")
        .body(html)
}

#[get("/speech")]
async fn story_page() -> impl Responder {
    let html = include_str!("templates/speech.html");
    HttpResponse::Ok().content_type("text/html").body(html)
}

#[get("/transcribe")]
async fn transcribe_page() -> impl Responder {
    let html = include_str!("templates/transcribe.html");
    HttpResponse::Ok().content_type("text/html").body(html)
}

#[get("/notes")]
async fn notes_page() -> impl Responder {
    let html = include_str!("templates/notes.html");
    HttpResponse::Ok().content_type("text/html").body(html)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    HttpServer::new(|| {
        App::new()
            .data(AppState {
                global_context: Arc::new(Mutex::new(String::new())),
            })
            .service(home)
            .service(story_page)
            .service(transcribe_page)
            .service(transcribe_audio)
            .service(generate_audio)
            .service(notes_page)
            .service(update_context)
            .service(get_ai_results)
            .service(search_ai)
            .service(Files::new("/static", "./static"))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
