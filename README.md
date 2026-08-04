# Audio Format Converter (Batch)

A fast batch audio converter that wraps `ffmpeg`. Feed it a directory (or single file), pick a target format and bitrate, and let it convert everything in parallel with a live progress bar.

## Features

- Six supported formats: **MP3**, **WAV**, **FLAC**, **OGG**, **M4A**, **AAC**.
- Parallel conversion using a fixed-size thread pool (defaults to your CPU count).
- Live ASCII progress bar and per-file success / failure reporting.
- Recursive directory traversal (opt-in with `--recursive`).
- Filter source files by extension with `--from`.
- Safe by default: refuses to overwrite existing files unless `--overwrite` is given.
- Dry-run mode to preview conversions without touching ffmpeg.
- Clear error message when `ffmpeg` is missing from `PATH`.

## Prerequisites

You must have `ffmpeg` installed and available on your `PATH`.

- **Windows**: `winget install Gyan.FFmpeg` or download from <https://ffmpeg.org/download.html>.
- **macOS**: `brew install ffmpeg`.
- **Linux (Debian/Ubuntu)**: `sudo apt install ffmpeg`.

Verify with:

```bash
ffmpeg -version
```

## Installation

```bash
cd Audio-Format-Converter-Batch
cargo build --release
./target/release/audioconv --help
```

## Usage

### Basic

```bash
audioconv ./music --to mp3 --bitrate 320k --output ./converted
```

### Only convert WAV sources

```bash
audioconv ./music --from wav --to flac --output ./flac_out
```

### Recurse into subfolders and overwrite existing files

```bash
audioconv ./library --to m4a --bitrate 256k --recursive --overwrite --output ./library_m4a
```

### Preview without converting

```bash
audioconv ./music --to ogg --dry-run
```

### Convert a single file

```bash
audioconv ./song.wav --to mp3 --bitrate 320k --output ./converted
```

## Options

| Flag | Description | Default |
|------|-------------|---------|
| `<INPUT>` | File or directory to scan (positional) | required |
| `--to <FORMAT>` | Target format (`mp3`, `wav`, `flac`, `ogg`, `m4a`, `aac`) | required |
| `--from <FORMAT>` | Only convert files with this source format | any supported |
| `--bitrate <RATE>` | ffmpeg `-b:a` bitrate (e.g. `128k`, `192k`, `320k`) | `192k` |
| `--output <DIR>` | Output directory (created if missing) | `./converted` |
| `--jobs <N>` | Number of parallel ffmpeg workers | CPU count |
| `--recursive` | Recurse into subdirectories | off |
| `--overwrite` | Overwrite output files if they exist | off |
| `--dry-run` | Print planned operations without running ffmpeg | off |

## Exit codes

- `0`: all files converted successfully.
- `1`: `ffmpeg` missing, invalid arguments, or one or more files failed to convert.

## License

Released under the MIT License.
