use axum::Json;
use once_cell::sync::OnceCell;
use screenpipe_core::model::EmbeddingModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

static EMBEDDING_MODEL: OnceCell<Arc<Mutex<EmbeddingModel>>> = OnceCell::new();

// OpenAI-like request/response types
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct EmbeddingRequest {
    model: String,
    input: EmbeddingInput,
    #[serde(default = "default_encoding")]
    encoding_format: String,
    #[serde(default)]
    user: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    #[serde(rename = "single")]
    Single(String),
    #[serde(rename = "multiple")]
    Multiple(Vec<String>),
}

#[derive(Serialize)]
pub struct EmbeddingResponse {
    object: String,
    data: Vec<EmbeddingData>,
    model: String,
    usage: Usage,
}

#[derive(Serialize)]
pub struct EmbeddingData {
    object: String,
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Serialize)]
pub struct Usage {
    prompt_tokens: usize,
    total_tokens: usize,
}

fn default_encoding() -> String {
    "float".to_string()
}

pub async fn get_or_initialize_model() -> anyhow::Result<Arc<Mutex<EmbeddingModel>>> {
    if let Some(model) = EMBEDDING_MODEL.get() {
        return Ok(model.clone());
    }

    let model = EmbeddingModel::new(None, None)?;
    EMBEDDING_MODEL
        .set(Arc::new(Mutex::new(model)))
        .map_err(|_| anyhow::anyhow!("failed to set global embedding model"))?;
    info!("embedding model initialized");

    EMBEDDING_MODEL
        .get()
        .ok_or_else(|| anyhow::anyhow!("model initialization failed"))
        .map(|model| model.clone())
}

pub async fn create_embeddings(
    Json(request): Json<EmbeddingRequest>,
) -> Result<Json<EmbeddingResponse>, (axum::http::StatusCode, String)> {
    let model = get_or_initialize_model()
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let model = model.lock().await;

    let (texts, _is_single) = match request.input {
        EmbeddingInput::Single(text) => (vec![text], true),
        EmbeddingInput::Multiple(texts) => (texts, false),
    };

    // Generate embeddings
    let embeddings = if texts.len() == 1 {
        vec![model
            .generate_embedding(&texts[0])
            .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?]
    } else {
        model
            .generate_batch_embeddings(&texts)
            .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    // Create response
    let data = embeddings
        .into_iter()
        .enumerate()
        .map(|(i, embedding)| EmbeddingData {
            object: "embedding".to_string(),
            embedding,
            index: i,
        })
        .collect();

    let response = EmbeddingResponse {
        object: "list".to_string(),
        data,
        model: request.model,
        usage: Usage {
            prompt_tokens: texts.iter().map(|t| t.len()).sum(),
            total_tokens: texts.iter().map(|t| t.len()).sum(),
        },
    };

    Ok(Json(response))
}
