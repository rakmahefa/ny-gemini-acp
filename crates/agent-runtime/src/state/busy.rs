//! Inter-process `busy` sentinel management.

use super::Store;

impl Store {
    pub(crate) fn busy_path(&self, id: &str) -> std::path::PathBuf {
        self.dir.join(format!("{id}.busy"))
    }

    pub(crate) async fn acquire_busy(&self, id: &str) -> anyhow::Result<()> {
        let path = self.busy_path(id);
        let content = format_busy_content();

        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(_) => {
                if let Err(error) = tokio::fs::write(&path, &content).await {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(error.into());
                }
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if stale_busy_sentinel(&path).await {
                    let _ = tokio::fs::remove_file(&path).await;
                    let file = tokio::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .await?;
                    drop(file);
                    if let Err(error) = tokio::fs::write(&path, &content).await {
                        let _ = tokio::fs::remove_file(&path).await;
                        return Err(error.into());
                    }
                    Ok(())
                } else {
                    anyhow::bail!("session {id} already busy")
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) async fn release_busy(&self, id: &str) {
        let _ = tokio::fs::remove_file(self.busy_path(id)).await;
    }
}

fn format_busy_content() -> String {
    match process_start_time(std::process::id()) {
        Some(start_time) => format!(
            "pid={} start_time={} ts={}\n",
            std::process::id(),
            start_time,
            crate::time::now_unix()
        ),
        None => format!("pid={} ts={}\n", std::process::id(), crate::time::now_unix()),
    }
}

/// Returns true only when a well-formed busy sentinel references a dead PID,
/// or a reused PID whose recorded process start time no longer matches.
pub(crate) async fn stale_busy_sentinel(path: &std::path::Path) -> bool {
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return false;
    };
    let Some(pid) = parse_busy_pid(&raw) else {
        return false;
    };

    let Some(current_start) = process_start_time(pid) else {
        return true;
    };
    match parse_busy_start_time(&raw) {
        Some(recorded_start) => recorded_start != current_start,
        None => false,
    }
}

/// Startup recovery may also remove malformed sentinels left by a crashed writer.
pub(crate) async fn recoverable_busy_sentinel(path: &std::path::Path) -> bool {
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return true;
    };
    let Some(pid) = parse_busy_pid(&raw) else {
        return true;
    };

    let Some(current_start) = process_start_time(pid) else {
        return true;
    };
    match parse_busy_start_time(&raw) {
        Some(recorded_start) => recorded_start != current_start,
        None => false,
    }
}

fn parse_busy_pid(raw: &str) -> Option<u32> {
    raw.lines()
        .find_map(|line| line.strip_prefix("pid=")?.split_whitespace().next())
        .and_then(|value| value.parse::<u32>().ok())
}

fn parse_busy_start_time(raw: &str) -> Option<u64> {
    raw.lines()
        .find_map(|line| line.strip_prefix("start_time=")?.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
}

#[cfg(target_os = "linux")]
fn process_start_time(pid: u32) -> Option<u64> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, after_comm) = raw.rsplit_once(") ")?;
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn process_start_time(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_busy_pid, parse_busy_start_time, process_start_time};

    #[test]
    fn parses_busy_identity() {
        let raw = "pid=123 start_time=456 ts=789\n";
        assert_eq!(parse_busy_pid(raw), Some(123));
        assert_eq!(parse_busy_start_time(raw), Some(456));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn current_process_has_a_stable_start_time() {
        assert!(process_start_time(std::process::id()).is_some());
    }
}
