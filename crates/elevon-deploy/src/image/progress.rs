use bollard::models::{BuildInfo, ErrorDetail, ProgressDetail, PushImageInfo};
use futures_util::StreamExt;
use tracing_indicatif::span_ext::IndicatifSpanExt;

#[derive(Debug, Clone, Copy)]
pub enum ProgressMode {
    Build,
    Push,
}

pub trait DockerProgressEvent {
    fn stream_text(&self) -> Option<&str>;
    fn status_text(&self) -> Option<&str>;
    fn progress_detail(&self) -> Option<&ProgressDetail>;
    fn error_detail(&self) -> Option<&ErrorDetail>;
}

impl DockerProgressEvent for BuildInfo {
    fn stream_text(&self) -> Option<&str> {
        self.stream.as_deref()
    }

    fn status_text(&self) -> Option<&str> {
        self.status.as_deref()
    }

    fn progress_detail(&self) -> Option<&ProgressDetail> {
        self.progress_detail.as_ref()
    }

    fn error_detail(&self) -> Option<&ErrorDetail> {
        self.error_detail.as_ref()
    }
}

impl DockerProgressEvent for PushImageInfo {
    fn stream_text(&self) -> Option<&str> {
        None
    }

    fn status_text(&self) -> Option<&str> {
        self.status.as_deref()
    }

    fn progress_detail(&self) -> Option<&ProgressDetail> {
        self.progress_detail.as_ref()
    }

    fn error_detail(&self) -> Option<&ErrorDetail> {
        self.error_detail.as_ref()
    }
}

pub async fn drain_progress_stream<S, T, E>(
    mut stream: S,
    mode: ProgressMode,
    action: &str,
) -> anyhow::Result<()>
where
    S: StreamExt<Item = Result<T, E>> + Unpin,
    T: DockerProgressEvent,
    E: std::fmt::Display,
{
    let mut printer = ProgressPrinter::new(mode);

    while let Some(item) = stream.next().await {
        let info = item.map_err(|e| anyhow::anyhow!("{action} failed: {e}"))?;
        printer.handle(&info, action)?;
    }

    Ok(())
}

pub struct ProgressPrinter {
    mode: ProgressMode,
    // skip repeated Waiting / same % so the terminal isn't a wall of noise
    last_line_key: Option<String>,
}

impl ProgressPrinter {
    pub fn new(mode: ProgressMode) -> Self {
        Self {
            mode,
            last_line_key: None,
        }
    }

    pub fn handle(&mut self, info: &impl DockerProgressEvent, action: &str) -> anyhow::Result<()> {
        if let Some(err) = info.error_detail() {
            anyhow::bail!("{action} failed: {err:?}");
        }

        if let Some(s) = info.stream_text() {
            for line in s.lines() {
                let line = line.trim_end();
                if !line.is_empty() {
                    tracing_indicatif::indicatif_println!("{line}");
                }
            }
        }

        let Some(status) = info.status_text() else {
            return Ok(());
        };

        tracing::Span::current().pb_set_message(status);

        let (current, total) = info
            .progress_detail()
            .map(|d| (d.current, d.total))
            .unwrap_or((None, None));

        match self.mode {
            ProgressMode::Build => {
                tracing::debug!(
                    %status,
                    current = current.unwrap_or(0),
                    total = total.unwrap_or(0),
                    "build progress"
                );
            }
            ProgressMode::Push => {
                self.print_push_status(status, current, total);
            }
        }

        Ok(())
    }

    fn print_push_status(&mut self, status: &str, current: Option<i64>, total: Option<i64>) {
        // Docker fires these per-layer; keep them on the spinner only.
        if status == "Waiting" || status == "Layer already exists" || status == "Preparing" {
            return;
        }

        let status = prettify_status(status);

        let line = match (current, total) {
            // done / full layer — one size is enough
            (Some(cur), Some(tot)) if tot > 0 && cur >= tot => {
                format!("{status}  {}", format_bytes(tot as u64))
            }
            (Some(cur), Some(tot)) if tot > 0 => {
                format!(
                    "{status}  {} / {} ({:.0}%)",
                    format_bytes(cur as u64),
                    format_bytes(tot as u64),
                    (cur as f64 / tot as f64) * 100.0
                )
            }
            (Some(cur), _) if cur > 0 => {
                format!("{status}  {}", format_bytes(cur as u64))
            }
            _ => status.clone(),
        };

        // only re-print every ~5% when we've got bytes, else once per status
        let key = match (current, total) {
            (Some(cur), Some(tot)) if tot > 0 => {
                let pct = ((cur as f64 / tot as f64) * 20.0).floor() as i64;
                format!("{status}:{pct}")
            }
            _ => status,
        };

        if self.last_line_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.last_line_key = Some(key);

        tracing_indicatif::indicatif_println!("{line}");
    }
}

// rewrites trailing `size: 1234` (raw bytes) from docker
fn prettify_status(status: &str) -> String {
    let Some((prefix, size_str)) = status.rsplit_once(" size: ") else {
        return status.to_string();
    };
    let Ok(bytes) = size_str.trim().parse::<u64>() else {
        return status.to_string();
    };
    format!("{prefix} size: {}", format_bytes(bytes))
}

// TODO: This doesnt seem to be correct for some reason
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn print_success(message: &str) {
    tracing_indicatif::indicatif_println!();
    tracing_indicatif::indicatif_println!("  \x1b[32m✓\x1b[0m {message}");
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, prettify_status};

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(12 * 1024 * 1024), "12 MB");
    }

    #[test]
    fn prettifies_digest_size() {
        let s = "latest: digest: sha256:abc size: 2479";
        assert_eq!(
            prettify_status(s),
            "latest: digest: sha256:abc size: 2.4 KB"
        );
    }
}
