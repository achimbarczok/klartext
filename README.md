<p align="center">
  <img src="assets/klartext_logo_banner.png" alt="Klartext Banner" width="600">
</p>

<h1 align="center">Klartext</h1>

<p align="center">
  <strong>Local Speech-to-Text Transcription</strong><br>
  A desktop app for fast, private audio transcription with AI-powered summarization
</p>

---

## Features

- **Fully local processing** — all data stays on your machine, no cloud services involved
- **Fast transcription** — NVIDIA Parakeet TDT model via ONNX Runtime with GPU acceleration
- **DirectML GPU support** — works with any GPU (NVIDIA, AMD, Intel); falls back to CPU automatically
- **Drag & drop** — drop audio files directly onto the window
- **File queue** — process multiple files sequentially
- **AI summarization** — generate podcast summaries or meeting protocols via Ollama
- **Export** — save transcriptions as TXT or Markdown
- **Long file support** — automatic 4-minute chunking for files of any length
- **Audio format support** — MP3 and WAV via symphonia (pure Rust decoding)

> **Note:** The application GUI is in German. Documentation is in English.

## Requirements

- Windows 10/11 (Linux/macOS possible but untested)
- [Rust toolchain](https://rustup.rs/) (for building from source)
- [Ollama](https://ollama.ai/) (optional, for AI summarization)

## Installation

```bash
git clone https://github.com/YOUR-USERNAME/klartext-rust.git
cd klartext-rust
cargo build --release
```

The compiled binary will be at `target/release/klartext-rust.exe`.

## Model Download

The Parakeet TDT model (int8 quantized, ~670 MB total) is required for transcription:

### PowerShell (Windows)

```powershell
mkdir models\tdt
Invoke-WebRequest -Uri "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/encoder-model.int8.onnx" -OutFile "models\tdt\encoder-model.onnx"
Invoke-WebRequest -Uri "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/encoder-model.int8.onnx.data" -OutFile "models\tdt\encoder-model.onnx.data"
Invoke-WebRequest -Uri "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/decoder_joint-model.int8.onnx" -OutFile "models\tdt\decoder_joint-model.onnx"
Invoke-WebRequest -Uri "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/vocab.txt" -OutFile "models\tdt\vocab.txt"
```

### Bash (Linux/macOS)

```bash
mkdir -p models/tdt
curl -L "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/encoder-model.int8.onnx" -o models/tdt/encoder-model.onnx
curl -L "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/encoder-model.int8.onnx.data" -o models/tdt/encoder-model.onnx.data
curl -L "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/decoder_joint-model.int8.onnx" -o models/tdt/decoder_joint-model.onnx
curl -L "https://huggingface.co/altunenes/parakeet-rs/resolve/main/tdt/vocab.txt" -o models/tdt/vocab.txt
```

## Usage

```powershell
# Set model path (optional, defaults to ./models/tdt)
$env:KLARTEXT_MODEL_PATH = "models\tdt"

# Run the application
cargo run --release
```

1. The model loads on startup (progress indicator shown)
2. Drag an audio file onto the drop zone, or click the file picker button
3. Transcription runs with a progress bar and time estimate
4. Copy the result, or export as TXT/Markdown
5. Optionally summarize the transcript using Ollama

## Summarization (Optional)

AI summarization requires [Ollama](https://ollama.ai/) running locally:

```bash
# Install Ollama from https://ollama.ai/
ollama pull gemma4:e4b
ollama serve
```

After transcription, select a summarization mode (Podcast Summary, Meeting Protocol, or Custom Prompt) and click "Zusammenfassen".

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `KLARTEXT_MODEL_PATH` | `./models/tdt` | Path to the Parakeet TDT model directory |
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API endpoint |
| `OLLAMA_MODEL` | `gemma4:e4b` | LLM model for summarization |
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |

## Architecture

| Component | Technology |
|-----------|-----------|
| Speech-to-text | parakeet-rs (NVIDIA Parakeet TDT via ONNX Runtime) |
| GPU acceleration | DirectML execution provider (any GPU, CPU fallback) |
| Audio decoding | symphonia (pure Rust, MP3 + WAV) |
| GUI | egui / eframe |
| Summarization | Ollama API |
| Error handling | thiserror + anyhow |
| Logging | tracing + tracing-subscriber |

## License

[MIT](LICENSE)
