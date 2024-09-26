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
use futures_util::TryStreamExt;
use std::time::Duration;

#[derive(Deserialize, Serialize)]
struct TextInput {
    text: String,
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    HttpServer::new(|| {
        App::new()
            .service(home)
            .service(story_page)
            .service(transcribe_page)
            .service(transcribe_audio)
            .service(generate_audio)
            .service(Files::new("/static", "./static"))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
