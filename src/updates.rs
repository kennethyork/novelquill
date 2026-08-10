use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/kennethyork/novelquill/releases/latest";

#[derive(Debug)]
pub enum UpdateStatus {
    UpToDate,
    Available { version: String, url: String },
    Error(String),
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

pub struct UpdateChecker {
    sender: Sender<UpdateStatus>,
    receiver: Receiver<UpdateStatus>,
    busy: Arc<AtomicBool>,
}

impl UpdateChecker {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn check(&self) {
        if self.busy.swap(true, Ordering::Relaxed) {
            return;
        }
        let sender = self.sender.clone();
        let busy = Arc::clone(&self.busy);
        std::thread::spawn(move || {
            let result = check_latest_release(env!("CARGO_PKG_VERSION"));
            let _ = sender.send(result);
            busy.store(false, Ordering::Relaxed);
        });
    }

    pub fn try_recv(&self) -> Option<UpdateStatus> {
        self.receiver.try_recv().ok()
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }
}

fn check_latest_release(current: &str) -> UpdateStatus {
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("NovelQuillStudio/{current}"))
        .build()
    {
        Ok(client) => client,
        Err(error) => return UpdateStatus::Error(error.to_string()),
    };
    let response = match client.get(LATEST_RELEASE_API).send() {
        Ok(response) if response.status() == reqwest::StatusCode::NOT_FOUND => {
            return UpdateStatus::UpToDate;
        }
        Ok(response) => response,
        Err(error) => return UpdateStatus::Error(error.to_string()),
    };
    let release = match response
        .error_for_status()
        .and_then(|response| response.json())
    {
        Ok(release) => release,
        Err(error) => return UpdateStatus::Error(error.to_string()),
    };
    let release: GithubRelease = release;
    if version_is_newer(current, &release.tag_name) {
        UpdateStatus::Available {
            version: release.tag_name,
            url: release.html_url,
        }
    } else {
        UpdateStatus::UpToDate
    }
}

fn version_is_newer(current: &str, candidate: &str) -> bool {
    fn parts(version: &str) -> Vec<u64> {
        version
            .trim_start_matches(['v', 'V'])
            .split('.')
            .map(|part| {
                part.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    }
    let mut current = parts(current);
    let mut candidate = parts(candidate);
    let length = current.len().max(candidate.len());
    current.resize(length, 0);
    candidate.resize(length, 0);
    candidate > current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_release_versions_without_lexical_errors() {
        assert!(version_is_newer("0.9.9", "v0.10.0"));
        assert!(version_is_newer("1.0.0", "v1.1.0"));
        assert!(!version_is_newer("1.2.0", "v1.2.0"));
        assert!(!version_is_newer("2.0.0", "v1.99.0"));
    }
}
