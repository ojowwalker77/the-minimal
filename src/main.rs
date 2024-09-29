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
use futures_util::TryStreamExt;
use std::time::Duration;
use std::fs;
use std::path::Path;

use lopdf::Document;







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
#[derive(Clone)]
struct AppState {
    db_context: Arc<Mutex<String>>,        // Static "database" context
    input_context: Arc<Mutex<String>>,     // User input context (dynamic)
    uploaded_files: Arc<Mutex<Vec<UploadedFile>>>,
    knowledge_net: Arc<Mutex<Vec<String>>>,
}

// Struct to track uploaded files and their status
#[derive(Clone, Debug)]
struct UploadedFile {
    filename: String,        // Name of the uploaded file
    purpose: String,         // Purpose (e.g., knowledge, style, etc.)
    state: String,           // State (e.g., processing, complete)
}

#[post("/upload-file")]
async fn upload_file(
    mut payload: Multipart,
    data: web::Data<AppState>,
    web::Query(info): web::Query<FilePurpose>
) -> impl Responder {
    log::info!("Received file upload request");

    let mut file_data = vec![];
    let mut filename = None;

    // Collect file data and capture filename
    while let Ok(Some(mut field)) = payload.try_next().await {
        log::info!("Reading file chunks");

        // Extract the filename from the content disposition
        let content_disposition = field.content_disposition();
        if let Some(name) = content_disposition.get_filename() {
            filename = Some(name.to_string());
            log::info!("File name: {}", name);
        }

        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => {
                    log::error!("Error processing file upload: {:?}", e);
                    return HttpResponse::InternalServerError().body("Error processing file upload.");
                }
            };
            file_data.extend_from_slice(&data);
        }
    }

    // Use the filename or default if none found
    let filename = filename.unwrap_or_else(|| format!("file_{}.pdf", Uuid::new_v4()));
    let file_path = format!("./static/{}", filename);  // Use relative path

    // Check if the "static" directory exists, and create it if not
    let static_dir = Path::new("./static");
    if !static_dir.exists() {
        match fs::create_dir_all(static_dir) {
            Ok(_) => log::info!("Created static directory"),
            Err(e) => {
                log::error!("Failed to create static directory: {:?}", e);
                return HttpResponse::InternalServerError().body("Failed to create directory.");
            }
        }
    }

    log::info!("Saving file to: {}", file_path);

    // Save file to disk
    if let Err(e) = std::fs::write(&file_path, &file_data) {
        log::error!("Failed to save file: {:?}", e);
        return HttpResponse::InternalServerError().body(format!("Failed to save file: {}", e));
    }

    // Set the file state to "processing"
    let mut uploaded_files = data.uploaded_files.lock().unwrap();
    uploaded_files.push(UploadedFile {
        filename: filename.clone(),
        purpose: info.purpose.clone(),
        state: "processing".to_string(),
    });
    log::info!("File state set to 'processing'");

    let response = HttpResponse::Ok().body(format!("File '{}' uploaded, processing started.", filename));

    let data_clone = data.clone();
    let filename_clone = filename.clone();

    actix_web::rt::spawn(async move {
            log::info!("Starting summarization for file: {}", filename_clone);
            let summary = process_file_and_summarize(&file_path).await;

            if !summary.is_empty() {
                log::info!("Summarization successful for file: {}", filename_clone);

                // Update contexts
                let mut db_context = data_clone.db_context.lock().unwrap();
                *db_context = format!("{}\n{}", *db_context, summary);

                log::info!("Updated db_context with new summary");

                // Mark file as complete
                let mut uploaded_files = data_clone.uploaded_files.lock().unwrap();
                if let Some(file) = uploaded_files.iter_mut().find(|f| f.filename == filename_clone) {
                    file.state = "complete".to_string();
                }
                log::info!("File state set to 'complete' for file: {}", filename_clone);
            } else {
                log::error!("Summarization failed for file: {}", filename_clone);
            }
        });

    response
}







fn extract_text_from_pdf(file_path: &str) -> String {
    let doc = Document::load(file_path).expect("Failed to open PDF file");
    let mut extracted_text = String::new();

    for page_id in doc.get_pages().keys() {
        if let Ok(content) = doc.extract_text(&[*page_id]) {
            extracted_text.push_str(&content);
        }
    }

    extracted_text
}

async fn process_file_and_summarize(file_path: &str) -> String {
    log::info!("Extracting text from PDF: {}", file_path);
    let extracted_text = extract_text_from_pdf(file_path);

    if extracted_text.is_empty() {
        log::error!("Failed to extract text from the PDF");
        return String::new();
    }
    log::info!("Text extracted from PDF: {:?}", extracted_text);

    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    let client = Client::new();

    let messages = vec![
        serde_json::json!({
            "role": "system",
            "content": "You are an assistant tasked with summarizing large documents. Extract key points and provide a concise summary."
        }),
        serde_json::json!({
            "role": "user",
            "content": format!("The document content is as follows:\n\n{}", extracted_text),
        })
    ];

    let payload = serde_json::json!({
        "model": "gpt-4",
        "messages": messages,
        "max_tokens": 300,
        "temperature": 0.7,
    });

    log::info!("Sending summarization request to OpenAI API");
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await;

    match response {
        Ok(res) => match res.json::<serde_json::Value>().await {
            Ok(result) => {
                let summary = result["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("No summary available.")
                    .trim()
                    .to_string();
                log::info!("Summarization result: {}", summary);
                summary
            }
            Err(e) => {
                log::error!("Error parsing OpenAI response: {:?}", e);
                "Error parsing response".to_string()
            }
        },
        Err(e) => {
            log::error!("Error with OpenAI API request: {:?}", e);
            "Error with API request".to_string()
        }
    }
}






// Struct to handle query params (e.g., purpose of file)
#[derive(Deserialize)]
struct FilePurpose {
    purpose: String,
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

#[get("/get-global-context")]
async fn get_global_context(state: web::Data<AppState>) -> impl Responder {
    let db_context = state.db_context.lock().unwrap().clone();
    let input_context = state.input_context.lock().unwrap().clone();

    let global_context = format!("{}\n{}", db_context, input_context);

    if global_context.trim().is_empty() {
        log::info!("No context available.");
        return HttpResponse::Ok().body("");  // Explicitly return empty if nothing is there
    }

    log::info!("Returning global context:\n{}", global_context);
    HttpResponse::Ok().body(global_context)  // Return the full global context
}


#[post("/update-context")]
async fn update_context(state: web::Data<AppState>, form: web::Form<TextInput>) -> impl Responder {
    let mut input_context = state.input_context.lock().unwrap();
    *input_context = form.text.clone();

    log::info!("Updated input context: {:?}", *input_context);

    HttpResponse::Ok().body("Input context updated.")
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
        "max_tokens": 80,
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
    let db_context = state.db_context.lock().unwrap().clone();
    let input_context = state.input_context.lock().unwrap().clone();
    let context = format!("{}\n{}", db_context, input_context);


    if context.is_empty() {
        // Instead of returning a bad request, return an empty string
        return HttpResponse::Ok().body("");
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
        "max_tokens": 50,
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
                let suggestion = result["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("No suggestion available.")
                    .trim()
                    .to_string();
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
    env_logger::init(); // Initialize logging

    log::info!("Starting server...");

    // Initialize AppState outside the closure
    let shared_data = web::Data::new(AppState {
        db_context: Arc::new(Mutex::new(String::new())),  // Initialize as empty or load from a database
        input_context: Arc::new(Mutex::new(String::new())),  // Initialize as empty
        uploaded_files: Arc::new(Mutex::new(Vec::new())),
        knowledge_net: Arc::new(Mutex::new(Vec::new())),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(shared_data.clone())  // Share the same AppState across threads
            .service(home)
            .service(story_page)
            .service(transcribe_page)
            .service(transcribe_audio)
            .service(generate_audio)
            .service(notes_page)
            .service(update_context)
            .service(get_global_context)
            .service(get_ai_results)
            .service(search_ai)
            .service(upload_file)
            .service(Files::new("/static", "./static"))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
