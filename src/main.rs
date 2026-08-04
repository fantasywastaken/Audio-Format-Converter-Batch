use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use colored::Colorize;

#[derive(Parser, Debug)]
#[command(
    name = "audioconv",
    version,
    about = "Batch audio format converter powered by ffmpeg"
)]
struct Cli {
    #[arg(help = "Input file or directory to scan for audio files")]
    input: PathBuf,
    #[arg(long, value_enum, help = "Restrict scanning to a single source format")]
    from: Option<Format>,
    #[arg(long, value_enum, help = "Target output format")]
    to: Format,
    #[arg(long, default_value = "192k", help = "Audio bitrate passed to ffmpeg (-b:a)")]
    bitrate: String,
    #[arg(long, default_value = "./converted", help = "Output directory (created if missing)")]
    output: PathBuf,
    #[arg(long, help = "Number of parallel ffmpeg workers (defaults to CPU count)")]
    jobs: Option<usize>,
    #[arg(long, help = "Recursively descend into subdirectories")]
    recursive: bool,
    #[arg(long, help = "Overwrite output files if they already exist")]
    overwrite: bool,
    #[arg(long = "dry-run", help = "List the operations that would be performed without running ffmpeg")]
    dry_run: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq)]
enum Format {
    Mp3,
    Wav,
    Flac,
    Ogg,
    M4a,
    Aac,
}

impl Format {
    fn extension(&self) -> &'static str {
        match self {
            Format::Mp3 => "mp3",
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Ogg => "ogg",
            Format::M4a => "m4a",
            Format::Aac => "aac",
        }
    }
    fn from_ext(s: &str) -> Option<Format> {
        match s.to_lowercase().as_str() {
            "mp3" => Some(Format::Mp3),
            "wav" => Some(Format::Wav),
            "flac" => Some(Format::Flac),
            "ogg" => Some(Format::Ogg),
            "m4a" => Some(Format::M4a),
            "aac" => Some(Format::Aac),
            _ => None,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Err(e) = check_ffmpeg() {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    let files = collect_files(&cli.input, cli.from, cli.recursive)?;
    if files.is_empty() {
        println!("{}", "No matching audio files found.".yellow());
        return Ok(());
    }
    let root = if cli.input.is_dir() { cli.input.clone() } else {
        cli.input.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
    };
    fs::create_dir_all(&cli.output).context("could not create output directory")?;
    println!(
        "{} {} file(s) queued -> target: {}, bitrate: {}, workers: {}",
        "→".cyan().bold(),
        files.len(),
        cli.to.extension().to_uppercase().cyan(),
        cli.bitrate.cyan(),
        cli.jobs.unwrap_or_else(default_workers).to_string().cyan()
    );
    if cli.dry_run {
        for f in &files {
            let dest = output_path(f, &root, &cli.output, cli.to);
            println!("  {} -> {}", f.display(), dest.display());
        }
        return Ok(());
    }
    convert_all(files, &cli, &root)?;
    Ok(())
}

fn check_ffmpeg() -> Result<()> {
    let output = Command::new("ffmpeg").arg("-version").output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => bail!(
            "{}\n{}",
            "ffmpeg was not found in PATH.".red().bold(),
            "Install ffmpeg from https://ffmpeg.org/download.html and ensure the ffmpeg binary is on your PATH.".dimmed()
        ),
    }
}

fn collect_files(root: &Path, from: Option<Format>, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
        return Ok(out);
    }
    if !root.exists() {
        bail!("input path does not exist: {}", root.display());
    }
    walk(root, recursive, &mut |p| {
        if !p.is_file() {
            return;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        let file_fmt = Format::from_ext(ext);
        match (from, file_fmt) {
            (Some(f), Some(x)) if f == x => out.push(p.to_path_buf()),
            (None, Some(_)) => out.push(p.to_path_buf()),
            _ => {}
        }
    })?;
    Ok(out)
}

fn walk(root: &Path, recursive: bool, cb: &mut dyn FnMut(&Path)) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                walk(&path, recursive, cb)?;
            }
        } else {
            cb(&path);
        }
    }
    Ok(())
}

fn output_path(input: &Path, root: &Path, out_root: &Path, to: Format) -> PathBuf {
    let rel = if input.starts_with(root) {
        input.strip_prefix(root).unwrap_or(input).to_path_buf()
    } else {
        input
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    let mut dest = out_root.join(rel);
    dest.set_extension(to.extension());
    dest
}

struct JobResult {
    file: PathBuf,
    result: Result<PathBuf>,
}

fn convert_all(files: Vec<PathBuf>, cli: &Cli, root: &Path) -> Result<()> {
    let total = files.len();
    let workers = cli.jobs.unwrap_or_else(default_workers).max(1);
    let (tx_job, rx_job) = mpsc::channel::<PathBuf>();
    let rx_job = Arc::new(Mutex::new(rx_job));
    let (tx_res, rx_res) = mpsc::channel::<JobResult>();
    let bitrate = cli.bitrate.clone();
    let overwrite = cli.overwrite;
    let to = cli.to;
    let input_root = root.to_path_buf();
    let out_root = cli.output.clone();

    let mut handles = Vec::new();
    for _ in 0..workers {
        let rx_job = Arc::clone(&rx_job);
        let tx_res = tx_res.clone();
        let bitrate = bitrate.clone();
        let input_root = input_root.clone();
        let out_root = out_root.clone();
        handles.push(thread::spawn(move || loop {
            let job = {
                let guard = rx_job.lock().unwrap();
                guard.recv()
            };
            match job {
                Ok(path) => {
                    let dest = output_path(&path, &input_root, &out_root, to);
                    if let Some(p) = dest.parent() {
                        let _ = fs::create_dir_all(p);
                    }
                    let result = convert_one(&path, &dest, &bitrate, overwrite).map(|_| dest);
                    if tx_res.send(JobResult { file: path, result }).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }));
    }
    drop(tx_res);
    for f in files {
        tx_job.send(f).unwrap();
    }
    drop(tx_job);

    let mut done = 0usize;
    let mut ok = 0usize;
    let mut fail = 0usize;
    let mut stdout = std::io::stdout().lock();
    while let Ok(msg) = rx_res.recv() {
        done += 1;
        let _ = write!(stdout, "\r\x1b[K");
        match &msg.result {
            Ok(dest) => {
                ok += 1;
                let _ = writeln!(
                    stdout,
                    "{} {} -> {}",
                    "OK".green().bold(),
                    msg.file.display(),
                    dest.display()
                );
            }
            Err(e) => {
                fail += 1;
                let _ = writeln!(
                    stdout,
                    "{} {} — {}",
                    "!!".red().bold(),
                    msg.file.display(),
                    e
                );
            }
        }
        draw_progress(&mut stdout, done, total);
    }
    for h in handles {
        let _ = h.join();
    }
    let _ = writeln!(stdout);
    println!(
        "{} converted: {}   {} failed: {}",
        "OK".green().bold(),
        ok,
        "!!".red().bold(),
        fail
    );
    if fail > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn draw_progress<W: Write>(w: &mut W, done: usize, total: usize) {
    let ratio = if total == 0 { 0.0 } else { done as f64 / total as f64 };
    let width = 30usize;
    let filled = (ratio * width as f64) as usize;
    let filled_capped = filled.min(width);
    let empty = width - filled_capped;
    let bar = format!("{}{}", "█".repeat(filled_capped), "░".repeat(empty));
    let percent = (ratio * 100.0) as u32;
    let _ = write!(w, "\r[{}] {}/{} ({}%)", bar.cyan(), done, total, percent);
    let _ = w.flush();
}

fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn convert_one(input: &Path, dest: &Path, bitrate: &str, overwrite: bool) -> Result<()> {
    let mut cmd = Command::new("ffmpeg");
    if overwrite {
        cmd.arg("-y");
    } else {
        cmd.arg("-n");
    }
    cmd.arg("-hide_banner");
    cmd.arg("-loglevel").arg("error");
    cmd.arg("-i").arg(input);
    cmd.arg("-b:a").arg(bitrate);
    cmd.arg("-vn");
    cmd.arg(dest);
    let output = cmd.output().context("failed to invoke ffmpeg")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if err.is_empty() {
            bail!("ffmpeg exited with status {}", output.status);
        } else {
            bail!("ffmpeg: {}", err);
        }
    }
    Ok(())
}
