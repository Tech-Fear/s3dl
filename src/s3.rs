use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::config::{AuthMethod, ResolvedConfig};

pub struct ObjectMeta {
    pub content_type: Option<String>,
    pub content_length: Option<i64>,
    pub last_modified: Option<String>,
    pub e_tag: Option<String>,
    pub storage_class: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

pub async fn build_client(resolved: &ResolvedConfig) -> Result<Client> {
    let region = aws_sdk_s3::config::Region::new(resolved.region.clone());

    let aws_config = match &resolved.auth {
        AuthMethod::StaticKeys {
            access_key,
            secret_key,
        } => {
            aws_config::defaults(BehaviorVersion::latest())
                .region(region)
                .credentials_provider(Credentials::new(
                    access_key,
                    secret_key,
                    None,
                    None,
                    "s3dl-config",
                ))
                .load()
                .await
        }
        AuthMethod::Profile(profile) => {
            aws_config::defaults(BehaviorVersion::latest())
                .region(region)
                .profile_name(profile)
                .load()
                .await
        }
        AuthMethod::Default => {
            aws_config::defaults(BehaviorVersion::latest())
                .region(region)
                .load()
                .await
        }
    };

    Ok(Client::new(&aws_config))
}

pub async fn head_object(client: &Client, bucket: &str, key: &str) -> Result<ObjectMeta> {
    let resp = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .with_context(|| format!("failed to fetch metadata for s3://{bucket}/{key}"))?;

    Ok(ObjectMeta {
        content_type: resp.content_type().map(|s| s.to_string()),
        content_length: resp.content_length(),
        last_modified: resp.last_modified().map(|t| t.fmt(aws_sdk_s3::primitives::DateTimeFormat::DateTime).unwrap_or_default()),
        e_tag: resp.e_tag().map(|s| s.to_string()),
        storage_class: resp.storage_class().map(|s| s.as_str().to_string()),
        metadata: resp.metadata().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default(),
    })
}

pub async fn download(
    client: &Client,
    bucket: &str,
    key: &str,
    output: &Path,
    content_length: Option<i64>,
    quiet: bool,
) -> Result<u64> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let resp = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .with_context(|| format!("failed to download s3://{bucket}/{key}"))?;

    let total = content_length.map(|l| l as u64);

    let pb = if quiet {
        ProgressBar::hidden()
    } else {
        match total {
            Some(t) => {
                let pb = ProgressBar::new(t);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template(
                            "  {spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                        )
                        .expect("valid template")
                        .progress_chars("██░"),
                );
                pb
            }
            None => {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template("  {spinner:.green} {bytes} downloaded ({bytes_per_sec})")
                        .expect("valid template"),
                );
                pb
            }
        }
    };

    let mut file = tokio::fs::File::create(output)
        .await
        .with_context(|| format!("failed to create {}", output.display()))?;

    let mut stream = resp.body;
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.try_next().await.context("failed to read S3 stream")? {
        file.write_all(&chunk)
            .await
            .context("failed to write to output file")?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    pb.finish_and_clear();
    Ok(downloaded)
}

pub fn mime_to_extension(content_type: &str) -> Option<&'static str> {
    let mime = content_type.split(';').next().unwrap_or("").trim();
    match mime {
        "application/pdf" => Some("pdf"),
        "application/xml" | "text/xml" => Some("xml"),
        "application/json" => Some("json"),
        "text/plain" => Some("txt"),
        "text/html" => Some("html"),
        "text/csv" => Some("csv"),
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        "image/tiff" => Some("tiff"),
        "image/bmp" => Some("bmp"),
        "application/zip" => Some("zip"),
        "application/gzip" => Some("gz"),
        "application/x-tar" => Some("tar"),
        "application/x-7z-compressed" => Some("7z"),
        "application/x-rar-compressed" => Some("rar"),
        "application/msword" => Some("doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some("docx"),
        "application/vnd.ms-excel" => Some("xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
        _ => None,
    }
}

pub fn detect_extension_from_content(path: &Path) -> Option<&'static str> {
    let buf = std::fs::read(path).ok()?;
    if buf.is_empty() {
        return None;
    }

    // Binary magic bytes (check before text-based detection)
    if buf.starts_with(b"%PDF") {
        return Some("pdf");
    }
    if buf.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if buf.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }
    if buf.starts_with(b"GIF87a") || buf.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if buf.starts_with(b"RIFF") && buf.len() >= 12 && &buf[8..12] == b"WEBP" {
        return Some("webp");
    }
    if buf.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || buf.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
    {
        return Some("tiff");
    }
    if buf.starts_with(b"BM") {
        return Some("bmp");
    }
    if buf.starts_with(b"PK\x03\x04") {
        if is_docx(&buf) {
            return Some("docx");
        }
        if is_xlsx(&buf) {
            return Some("xlsx");
        }
        return Some("zip");
    }
    if buf.starts_with(&[0x1F, 0x8B]) {
        return Some("gz");
    }
    if buf.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some("7z");
    }
    if buf.starts_with(b"Rar!\x1A\x07") {
        return Some("rar");
    }

    // Text-based detection: trim leading whitespace/BOM
    let text = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let trimmed = text.trim_start_matches('\u{FEFF}').trim_start();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Some("json");
    }
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<xml") {
        return Some("xml");
    }
    if trimmed.starts_with("<!DOCTYPE html") || trimmed.starts_with("<html") {
        return Some("html");
    }
    if trimmed.starts_with('<') {
        return Some("xml");
    }
    if looks_like_csv(trimmed) {
        return Some("csv");
    }

    Some("txt")
}

fn looks_like_csv(text: &str) -> bool {
    let mut lines = text.lines().take(5);
    let first = match lines.next() {
        Some(l) => l,
        None => return false,
    };
    let comma_count = first.matches(',').count();
    if comma_count == 0 {
        return false;
    }
    for line in lines {
        if line.matches(',').count() != comma_count {
            return false;
        }
    }
    true
}

fn is_docx(buf: &[u8]) -> bool {
    buf.windows(19)
        .any(|w| w == b"word/document.xml" || w.starts_with(b"word/"))
}

fn is_xlsx(buf: &[u8]) -> bool {
    buf.windows(14)
        .any(|w| w == b"xl/workbook.xml" || w.starts_with(b"xl/"))
}

pub fn resolve_output_path(
    file_key: &str,
    output: Option<&str>,
    detected_ext: Option<&str>,
    default_dir: &str,
) -> PathBuf {
    let base_name = file_key.rsplit('/').next().unwrap_or(file_key);

    let final_name = match detected_ext {
        Some(ext) => {
            if let Some(dot_pos) = base_name.rfind('.') {
                let existing_ext = &base_name[dot_pos + 1..];
                if existing_ext.eq_ignore_ascii_case(ext) {
                    base_name.to_string()
                } else {
                    format!("{}.{ext}", &base_name[..dot_pos])
                }
            } else {
                format!("{base_name}.{ext}")
            }
        }
        None => base_name.to_string(),
    };

    match output {
        Some(p) => {
            let path = PathBuf::from(p);
            if path.is_dir() {
                path.join(&final_name)
            } else {
                path
            }
        }
        None => {
            let dir = shellexpand(default_dir);
            PathBuf::from(dir).join(&final_name)
        }
    }
}

fn shellexpand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}
