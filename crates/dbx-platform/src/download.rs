pub const R2_CDN_BASE: &str = "https://dl.dbxio.com/";
pub const GITHUB_RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/t8y2/dbx/releases/download/";
pub const CNB_RELEASE_DOWNLOAD_PREFIX: &str = "https://cnb.cool/dbxio.com/dbx/-/releases/download/";

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    #[default]
    Official,
    Cnb,
}

impl DownloadSource {
    pub fn download_candidate_urls(self, github_url: &str, r2_path: &str) -> Result<Vec<String>, String> {
        match self {
            Self::Official => Ok(download_candidate_urls(github_url, r2_path)),
            Self::Cnb => Ok(mirror_download_candidate_urls(
                r2_path,
                rewrite_github_release_url(github_url, CNB_RELEASE_DOWNLOAD_PREFIX)?,
            )),
        }
    }
}

fn mirror_download_candidate_urls(r2_path: &str, mirror_url: String) -> Vec<String> {
    let r2_url = format!("{R2_CDN_BASE}{r2_path}");
    vec![mirror_url, r2_url]
}

fn rewrite_github_release_url(url: &str, target_prefix: &str) -> Result<String, String> {
    if url.starts_with(target_prefix) {
        return Ok(url.to_string());
    }
    url.strip_prefix(GITHUB_RELEASE_DOWNLOAD_PREFIX)
        .map(|path| format!("{target_prefix}{path}"))
        .ok_or_else(|| format!("Unsupported DBX release download URL: {url}"))
}

pub fn download_candidate_urls(github_url: &str, r2_path: &str) -> Vec<String> {
    vec![format!("{R2_CDN_BASE}{r2_path}"), github_url.to_string()]
}

use std::pin::Pin;
use std::time::Duration;

type ResponseFuture = Pin<Box<dyn std::future::Future<Output = Result<reqwest::Response, String>> + Send>>;

pub async fn race_download(
    client: &reqwest::Client,
    github_url: &str,
    r2_path: &str,
    user_agent: &str,
) -> Result<reqwest::Response, String> {
    race_download_urls(client, &download_candidate_urls(github_url, r2_path), user_agent).await
}

pub async fn race_download_urls(
    client: &reqwest::Client,
    urls: &[String],
    user_agent: &str,
) -> Result<reqwest::Response, String> {
    use futures::future::select_ok;

    let mut futs: Vec<ResponseFuture> = Vec::with_capacity(urls.len());

    for url in urls {
        let client = client.clone();
        let url = url.clone();
        let ua = user_agent.to_string();
        futs.push(Box::pin(async move {
            client
                .get(url)
                .header(reqwest::header::USER_AGENT, ua)
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("{e}"))
        }) as ResponseFuture);
    }

    match select_ok(futs).await {
        Ok((resp, _)) => Ok(resp),
        Err(last_err) => Err(last_err),
    }
}

const RESILIENT_RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
const RESILIENT_RETRY_MAX_DELAY: Duration = Duration::from_millis(4000);

/// Total attempts made by [`resilient_download_bytes`] for callers without a
/// custom budget.
pub const RESILIENT_DOWNLOAD_ATTEMPTS: usize = 4;

enum DownloadAttemptError {
    /// Transient: connection dropped mid-body, connect/send error, 5xx. Bytes
    /// received so far are kept and the next attempt resumes with a Range
    /// request.
    Retryable(String),
    /// Deterministic: size cap exceeded or a plain 4xx. Retrying cannot help.
    Fatal(String),
}

enum ResumePlan {
    /// Append incoming chunks to the bytes already received.
    Append,
    /// The partial bytes are stale (the server ignored Range and restarted
    /// the body at zero): discard them so the body is rebuilt from zero.
    Restart,
}

fn retry_delay(attempt: usize) -> Duration {
    // attempt is 1-based; the first retry (attempt == 2) waits one base step.
    let steps = 1_u32 << (attempt.saturating_sub(2)).min(16);
    RESILIENT_RETRY_BASE_DELAY.saturating_mul(steps).min(RESILIENT_RETRY_MAX_DELAY)
}

enum ClassifyError {
    /// Deterministic (plain 4xx): retrying cannot help.
    FatalStatus(reqwest::StatusCode),
    /// Transient (5xx, 408, 429): a further attempt may succeed.
    RetryableStatus(reqwest::StatusCode),
    /// Transient, and the buffered bytes no longer line up with the resource
    /// (416 after a ranged request): discard them before the next attempt.
    StaleResume(reqwest::StatusCode),
}

fn classify_response(status: reqwest::StatusCode, resume_from: u64) -> Result<ResumePlan, ClassifyError> {
    if resume_from > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Err(ClassifyError::StaleResume(status));
    }
    if !status.is_success() {
        return Err(match status.as_u16() {
            408 | 429 => ClassifyError::RetryableStatus(status),
            code if (400..500).contains(&code) => ClassifyError::FatalStatus(status),
            _ => ClassifyError::RetryableStatus(status),
        });
    }
    if resume_from > 0 && status == reqwest::StatusCode::OK {
        // Server ignored the Range header and restarted the body at zero.
        return Ok(ResumePlan::Restart);
    }
    Ok(ResumePlan::Append)
}

/// Downloads an HTTP response body into memory, surviving transient transport
/// failures that kill a single-connection download: connect/send errors,
/// mid-body drops ("error decoding response body"), 5xx and 408/429 statuses.
///
/// Bytes received before a mid-body drop are reused: the next attempt sends a
/// Range request from the received offset; a server that answers 206 appends,
/// one that answers plain 200 restarts cleanly, and a 416 discards the partial
/// bytes and retries without a Range header. Resume correctness is guarded
/// by callers that verify a checksum or signature over the finished bytes —
/// every current caller does.
///
/// Error copy matches the previous single-attempt behavior (`Failed to
/// download/read {label} from {url}: …`) so UI strings stay stable; the last
/// retryable failure carries an `attempt N/M` suffix.
pub async fn resilient_download_bytes(
    client: &reqwest::Client,
    url: reqwest::Url,
    label: &str,
    max_bytes: usize,
    attempts: usize,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    let attempts = attempts.max(1);
    let mut bytes: Vec<u8> = Vec::new();
    let mut last_error = String::new();
    for attempt in 1..=attempts {
        if attempt > 1 {
            tokio::time::sleep(retry_delay(attempt)).await;
        }
        match download_attempt(client, &url, label, max_bytes, &mut bytes, &mut on_progress).await {
            Ok(()) => return Ok(bytes),
            Err(DownloadAttemptError::Fatal(error)) => return Err(error),
            Err(DownloadAttemptError::Retryable(error)) => {
                last_error = format!("{error} (download attempt {attempt}/{attempts})");
            }
        }
    }
    Err(last_error)
}

async fn download_attempt(
    client: &reqwest::Client,
    url: &reqwest::Url,
    label: &str,
    max_bytes: usize,
    bytes: &mut Vec<u8>,
    on_progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<(), DownloadAttemptError> {
    use futures::StreamExt;

    let resume_from = bytes.len() as u64;
    let mut request = client.get(url.clone());
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| DownloadAttemptError::Retryable(format!("Failed to download {label} from {url}: {error}")))?;
    let status_error = |status: reqwest::StatusCode| format!("Failed to download {label} from {url}: HTTP {status}");
    match classify_response(response.status(), resume_from) {
        Ok(ResumePlan::Append) => {}
        Ok(ResumePlan::Restart) => bytes.clear(),
        // The buffered bytes no longer line up with the resource: drop them so
        // the next attempt re-requests without a Range header. Never read the
        // 416 body — it is an error document, not content.
        Err(ClassifyError::StaleResume(status)) => {
            bytes.clear();
            return Err(DownloadAttemptError::Retryable(status_error(status)));
        }
        Err(ClassifyError::RetryableStatus(status)) => {
            return Err(DownloadAttemptError::Retryable(status_error(status)));
        }
        Err(ClassifyError::FatalStatus(status)) => {
            return Err(DownloadAttemptError::Fatal(status_error(status)));
        }
    }
    if response.content_length().is_some_and(|length| bytes.len() as u64 + length > max_bytes as u64) {
        return Err(DownloadAttemptError::Fatal(format!("{label} exceeds {max_bytes} bytes")));
    }
    let total = response.content_length().map(|length| bytes.len() as u64 + length);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|error| DownloadAttemptError::Retryable(format!("Failed to read {label} from {url}: {error}")))?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(DownloadAttemptError::Fatal(format!("{label} exceeds {max_bytes} bytes")));
        }
        bytes.extend_from_slice(&chunk);
        on_progress(bytes.len() as u64, total);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{classify_response, retry_delay, ClassifyError, ResumePlan};
    use super::{download_candidate_urls, DownloadSource};
    use std::time::Duration;

    fn status(code: u16) -> reqwest::StatusCode {
        reqwest::StatusCode::from_u16(code).unwrap()
    }

    #[test]
    fn download_candidates_exclude_third_party_github_proxy() {
        let urls = download_candidate_urls(
            "https://github.com/t8y2/dbx/releases/latest/download/latest.json",
            "releases/latest/latest.json",
        );

        assert_eq!(
            urls,
            vec![
                "https://dl.dbxio.com/releases/latest/latest.json",
                "https://github.com/t8y2/dbx/releases/latest/download/latest.json",
            ]
        );
    }

    #[test]
    fn mirror_download_candidates_prefer_selected_source() {
        let github_url = "https://github.com/t8y2/dbx/releases/download/agents-latest/agent-registry.json";
        assert_eq!(
            DownloadSource::Cnb.download_candidate_urls(github_url, "agents/agent-registry.json").unwrap(),
            vec![
                "https://cnb.cool/dbxio.com/dbx/-/releases/download/agents-latest/agent-registry.json",
                "https://dl.dbxio.com/agents/agent-registry.json",
            ]
        );
    }

    #[test]
    fn classify_appends_fresh_and_partial_content_responses() {
        assert!(matches!(classify_response(status(200), 0), Ok(ResumePlan::Append)));
        assert!(matches!(classify_response(status(206), 0), Ok(ResumePlan::Append)));
        assert!(matches!(classify_response(status(206), 1024), Ok(ResumePlan::Append)));
    }

    #[test]
    fn classify_restarts_when_server_ignores_range() {
        assert!(matches!(classify_response(status(200), 1024), Ok(ResumePlan::Restart)));
    }

    #[test]
    fn classify_fails_unsatisfiable_range_as_stale_resume() {
        // 416 is an error status: the attempt must fail (with the buffered
        // bytes dropped by the caller) instead of reading the error body.
        assert!(matches!(classify_response(status(416), 1024), Err(ClassifyError::StaleResume(_))));
        // Without a resume offset a 416 is just a deterministic client error.
        assert!(matches!(classify_response(status(416), 0), Err(ClassifyError::FatalStatus(_))));
    }

    #[test]
    fn classify_fails_deterministic_client_errors_without_retry() {
        assert!(matches!(classify_response(status(404), 0), Err(ClassifyError::FatalStatus(_))));
        assert!(matches!(classify_response(status(403), 0), Err(ClassifyError::FatalStatus(_))));
    }

    #[test]
    fn classify_retries_transient_statuses() {
        assert!(matches!(classify_response(status(408), 0), Err(ClassifyError::RetryableStatus(_))));
        assert!(matches!(classify_response(status(429), 0), Err(ClassifyError::RetryableStatus(_))));
        assert!(matches!(classify_response(status(500), 0), Err(ClassifyError::RetryableStatus(_))));
        assert!(matches!(classify_response(status(502), 0), Err(ClassifyError::RetryableStatus(_))));
        assert!(matches!(classify_response(status(503), 0), Err(ClassifyError::RetryableStatus(_))));
    }

    #[test]
    fn retry_delay_backs_off_then_caps() {
        assert_eq!(retry_delay(2), Duration::from_millis(500));
        assert_eq!(retry_delay(3), Duration::from_millis(1000));
        assert_eq!(retry_delay(4), Duration::from_millis(2000));
        assert_eq!(retry_delay(5), Duration::from_millis(4000));
        assert_eq!(retry_delay(20), Duration::from_millis(4000));
    }
}
