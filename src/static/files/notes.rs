use actix_files::Files;
use actix_multipart::Multipart;
use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
struct TextInput {
    text: String,
}

#[derive(Clone)]
struct AppState {
    db_context: Arc<Mutex<HashMap<String, String>>>,
    input_context: Arc<Mutex<HashMap<String, String>>>,
    uploaded_files: Arc<Mutex<HashMap<String, Vec<UploadedFile>>>>,
}

#[derive(Clone, Debug)]
struct UploadedFile {
    filename: String,
    purpose: String,
    state: String,
}

#[derive(Deserialize)]
struct FilePurpose {
    purpose: String,
}

#[post("/upload-file")]
async fn upload_file(
    mut payload: Multipart,
    data: web::Data<AppState>,
    web::Query(info): web::Query<FilePurpose>,
    req: HttpRequest,
) -> impl Responder {
    let mut file_data = vec![];
    let mut filename = None;

    while let Ok(Some(mut field)) = payload.try_next().await {
        if let Some(name) = field.content_disposition().get_filename() {
            filename = Some(name.to_string());
        }

        while let Some(chunk) = field.next().await {
            let data = chunk
                .map_err(|_| HttpResponse::InternalServerError().body("File processing error"))?;
            file_data.extend_from_slice(&data);
        }
    }

    let filename = filename.unwrap_or_else(|| format!("file_{}.pdf", Uuid::new_v4()));
    let file_path = format!("./static/{}", filename);
    let static_dir = Path::new("./static");
    if !static_dir.exists() {
        fs::create_dir_all(static_dir)
            .map_err(|_| HttpResponse::InternalServerError().body("Failed to create directory"))?;
    }

    fs::write(&file_path, &file_data)
        .map_err(|_| HttpResponse::InternalServerError().body("Failed to save file"))?;

    let session_id = get_session_id(&req)
        .ok_or_else(|| HttpResponse::BadRequest().body("Missing session ID"))?;

    let mut uploaded_files = data.uploaded_files.lock().await;
    uploaded_files
        .entry(session_id.clone())
        .or_insert_with(Vec::new)
        .push(UploadedFile {
            filename: filename.clone(),
            purpose: info.purpose.clone(),
            state: "ready".to_string(),
        });

    HttpResponse::Ok().body(format!("File '{}' uploaded successfully.", filename))
}

#[post("/update-context")]
async fn update_context(
    state: web::Data<AppState>,
    form: web::Form<TextInput>,
    req: HttpRequest,
) -> impl Responder {
    let session_id = get_session_id(&req)
        .ok_or_else(|| HttpResponse::BadRequest().body("Missing session ID"))?;

    let mut input_context = state.input_context.lock().await;
    input_context.insert(session_id, form.text.clone());

    HttpResponse::Ok().body("Context updated.")
}

#[get("/get-global-context")]
async fn get_global_context(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    let session_id = get_session_id(&req)
        .ok_or_else(|| HttpResponse::BadRequest().body("Missing session ID"))?;

    let db_context = state.db_context.lock().await;
    let input_context = state.input_context.lock().await;

    let global_context = format!(
        "{}\n{}",
        db_context.get(&session_id).unwrap_or(&"".to_string()),
        input_context.get(&session_id).unwrap_or(&"".to_string()),
    );

    HttpResponse::Ok().body(global_context)
}

#[get("/get-ai-results")]
async fn get_ai_results(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    let session_id = get_session_id(&req)
        .ok_or_else(|| HttpResponse::BadRequest().body("Missing session ID"))?;

    let db_context = state.db_context.lock().await;
    let input_context = state.input_context.lock().await;

    let context = format!(
        "{}\n{}",
        db_context.get(&session_id).unwrap_or(&"".to_string()),
        input_context.get(&session_id).unwrap_or(&"".to_string()),
    );

    if context.trim().is_empty() {
        return HttpResponse::Ok().body("No context available.");
    }

    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    let client = reqwest::Client::new();

    let messages = vec![
        serde_json::json!({"role": "system", "content": "Continue the user's notes logically based on their previous writing."}),
        serde_json::json!({"role": "user", "content": format!("Given the following context:\n{}\n\nContinue logically:", context)}),
    ];

    let payload = serde_json::json!({
        "model": "gpt-4",
        "messages": messages,
        "max_tokens": 100,
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
                let suggestion = result["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("No suggestion available.")
                    .trim()
                    .to_string();
                HttpResponse::Ok().body(suggestion)
            }
            Err(_) => HttpResponse::InternalServerError().body("Error parsing response"),
        },
        Err(_) => HttpResponse::InternalServerError().body("Error calling GPT-4 API"),
    }
}

#[post("/close_session")]
async fn close_session(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    let session_id = get_session_id(&req)
        .ok_or_else(|| HttpResponse::BadRequest().body("Missing session ID"))?;

    {
        let mut db_context = state.db_context.lock().await;
        db_context.remove(&session_id);
    }
    {
        let mut input_context = state.input_context.lock().await;
        input_context.remove(&session_id);
    }
    {
        let mut uploaded_files = state.uploaded_files.lock().await;
        uploaded_files.remove(&session_id);
    }

    HttpResponse::Ok().body("Session closed.")
}

#[get("/notes")]
async fn notes_page() -> impl Responder {
    let html = include_str!("files/notes.html");
    HttpResponse::Ok().content_type("text/html").body(html)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port)
        .parse()
        .expect("Invalid address format");

    let shared_data = web::Data::new(AppState {
        db_context: Arc::new(Mutex::new(HashMap::new())),
        input_context: Arc::new(Mutex::new(HashMap::new())),
        uploaded_files: Arc::new(Mutex::new(HashMap::new())),
    });

    println!("Starting Notes Server on port: {}", port);

    HttpServer::new(move || {
        App::new()
            .app_data(shared_data.clone())
            .service(notes_page)
            .service(upload_file)
            .service(update_context)
            .service(get_global_context)
            .service(get_ai_results)
            .service(close_session)
            .service(Files::new("/static/files", "./src/files"))
    })
    .bind(addr)?
    .run()
    .await
}

fn get_session_id(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("X-Session-ID")?
        .to_str()
        .ok()
        .map(|s| s.to_string())
}
