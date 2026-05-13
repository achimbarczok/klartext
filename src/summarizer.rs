//! Ollama-based summarization with configurable prompts.
//! Prompts can be customized via a `prompts.toml` file in the working directory.

use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use tracing::{error, info, warn};

/// Prompt configuration loaded from prompts.toml
#[derive(Debug, Clone, Deserialize, Default)]
struct PromptConfig {
    summary: Option<PromptEntry>,
    protocol: Option<PromptEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct PromptEntry {
    prompt: String,
}

/// Load custom prompts from prompts.toml if it exists.
fn load_prompt_config() -> PromptConfig {
    let path = std::env::var("KLARTEXT_PROMPTS_FILE")
        .unwrap_or_else(|_| "prompts.toml".to_string());

    match fs::read_to_string(&path) {
        Ok(content) => {
            match toml::from_str::<PromptConfig>(&content) {
                Ok(config) => {
                    info!("Loaded custom prompts from: {}", path);
                    config
                }
                Err(e) => {
                    warn!("Failed to parse {}: {}. Using default prompts.", path, e);
                    PromptConfig::default()
                }
            }
        }
        Err(_) => {
            info!("No prompts.toml found, using default prompts");
            PromptConfig::default()
        }
    }
}

/// Available summarization modes with pre-configured prompts.
#[derive(Debug, Clone, PartialEq)]
pub enum SummaryMode {
    /// Summarize a podcast episode
    PodcastSummary,
    /// Create a meeting protocol with action items
    MeetingProtocol,
    /// Custom user-defined prompt
    Custom(String),
}

impl SummaryMode {
    /// Get the display name for the UI.
    pub fn label(&self) -> &str {
        match self {
            Self::PodcastSummary => "Zusammenfassung",
            Self::MeetingProtocol => "Protokoll mit Todos",
            Self::Custom(_) => "Eigener Prompt",
        }
    }

    /// Get the system prompt for this mode.
    /// Loads from prompts.toml if available, otherwise uses built-in defaults.
    pub fn system_prompt(&self) -> String {
        let config = load_prompt_config();

        match self {
            Self::PodcastSummary => {
                if let Some(entry) = config.summary {
                    return entry.prompt;
                }
                "Du bist ein hilfreicher Assistent. \
                 Antworte direkt ohne langes Nachdenken oder Analyse. \
                 Erstelle eine kurze Zusammenfassung des folgenden Textes. \
                 Format: Eine Titelzeile (als Markdown-Heading), dann zwei kurze Abs\u{00E4}tze die den Inhalt zusammenfassen. \
                 Nicht mehr. Keine Aufz\u{00E4}hlungen, keine Bullet Points. \
                 Antworte auf Deutsch.".to_string()
            }
            Self::MeetingProtocol => {
                if let Some(entry) = config.protocol {
                    return entry.prompt;
                }
                "Du bist ein hilfreicher Assistent, der Meeting-Protokolle erstellt. \
                 Antworte direkt ohne langes Nachdenken oder Analyse. \
                 Erstelle ein strukturiertes Protokoll mit folgenden Abschnitten: \
                 1. Datum und Teilnehmer (falls erkennbar) \
                 2. Besprochene Themen (kurz zusammengefasst) \
                 3. Beschl\u{00FC}sse und Ergebnisse \
                 4. Am Ende ein klar abgetrennter Block '## Todos' mit einer Tabelle: \
                    | Wer | Was | Bis wann | \
                    Trage dort alle erkennbaren Aufgaben ein mit Verantwortlichem und Frist (falls genannt). \
                 Formatiere als Markdown. Antworte auf Deutsch.".to_string()
            }
            Self::Custom(prompt) => prompt.clone(),
        }
    }
}

/// Command sent to the summarizer thread.
pub enum SummaryCommand {
    /// Summarize the given text with the specified mode.
    Summarize {
        text: String,
        mode: SummaryMode,
        model: String,
    },
    /// Shut down the summarizer thread.
    Shutdown,
}

/// Status updates from the summarizer thread.
#[derive(Debug, Clone)]
pub enum SummaryStatus {
    /// Summarization is in progress.
    InProgress,
    /// Summarization completed successfully.
    Complete(String),
    /// Summarization failed.
    Error(String),
}

/// Ollama API request body.
#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    system: String,
    stream: bool,
    /// Disable thinking/analyzing mode for faster responses
    think: bool,
}

/// Ollama API response body.
#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

/// Spawn a summarizer thread that communicates via channels.
pub fn spawn_summarizer(
    command_rx: Receiver<SummaryCommand>,
    status_tx: Sender<SummaryStatus>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let ollama_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());

        loop {
            let command = match command_rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => break,
            };

            match command {
                SummaryCommand::Summarize { text, mode, model } => {
                    let _ = status_tx.send(SummaryStatus::InProgress);
                    info!("Starting summarization with model '{}', mode: {:?}", model, mode.label());

                    let result = call_ollama(&ollama_url, &text, &mode, &model);
                    match result {
                        Ok(summary) => {
                            info!("Summarization complete");
                            let _ = status_tx.send(SummaryStatus::Complete(summary));
                        }
                        Err(e) => {
                            error!("Summarization failed: {}", e);
                            let _ = status_tx.send(SummaryStatus::Error(e));
                        }
                    }
                }
                SummaryCommand::Shutdown => {
                    info!("Summarizer shutting down");
                    break;
                }
            }
        }
    })
}

/// Call the Ollama API to generate a summary.
fn call_ollama(
    base_url: &str,
    transcript: &str,
    mode: &SummaryMode,
    model: &str,
) -> Result<String, String> {
    let url = format!("{}/api/generate", base_url);

    let prompt = format!(
        "Hier ist eine Transkription. Bitte verarbeite sie gem\u{00E4}\u{00DF} den Anweisungen:\n\n---\n{}\n---",
        transcript
    );

    let request_body = OllamaRequest {
        model: model.to_string(),
        prompt,
        system: mode.system_prompt(),
        stream: false,
        think: false,
    };

    let body_json = serde_json::to_string(&request_body)
        .map_err(|e| format!("JSON serialization failed: {}", e))?;

    let mut response = ureq::post(&url)
        .header("Content-Type", "application/json")
        .send(body_json.as_bytes())
        .map_err(|e| format!("Ollama nicht erreichbar: {}. Ist Ollama gestartet?", e))?;

    let response_body: OllamaResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("Ung\u{00FC}ltige Antwort von Ollama: {}", e))?;

    Ok(response_body.response)
}
