use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct ExtractParams {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct StreamParams {
    pub url: Option<String>,
    pub id: Option<String>,
    pub ss: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct CoverParams {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub audio_url: String,
    pub cover_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractResponse {
    pub playlist_title: Option<String>,
    pub tracks: Vec<SearchTrack>,
}
