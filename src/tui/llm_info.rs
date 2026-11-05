//! Header summary of the LLM configuration, validated like `chat` but offline.

use organizer::llm::provider::{LlmProvider, OpenAiCompatible, ProviderConfig};

#[derive(Clone, Debug)]
pub struct LlmInfo {
    pub available: bool,
    /// Header text, e.g. `gpt-x@api.example.com（远程）` or `未配置`.
    pub label: String,
    pub host: Option<String>,
    pub remote: bool,
    /// `ORGANIZER_LLM_NO_CONTENT=1`: the excerpt option is unavailable.
    pub no_content: bool,
    /// Why drafting with the LLM is unavailable.
    pub reason: Option<String>,
}

fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

pub fn detect() -> LlmInfo {
    let no_content = flag("ORGANIZER_LLM_NO_CONTENT");
    let unavailable = |label: &str, reason: String| LlmInfo {
        available: false,
        label: label.into(),
        host: None,
        remote: false,
        no_content,
        reason: Some(reason),
    };
    let config = match ProviderConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            let error = error.to_string();
            let label = if error.contains("disabled") {
                "已禁用"
            } else if error.contains("not configured") {
                "未配置"
            } else {
                "配置无效"
            };
            return unavailable(label, error);
        }
    };
    let host = url_host(&config.base_url);
    let model = config.model.clone();
    if let Err(error) = OpenAiCompatible::new(config) {
        return unavailable("配置无效", error.to_string());
    }
    let local = host.as_deref().is_some_and(|h| {
        h == "localhost"
            || h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    LlmInfo {
        available: true,
        label: format!(
            "{}@{}（{}）",
            super::text::clean(&model),
            super::text::clean(host.as_deref().unwrap_or("?")),
            if local { "本地" } else { "远程" }
        ),
        host,
        remote: !local,
        no_content,
        reason: None,
    }
}

/// The real provider used by `chat` in the TUI, created on the worker thread.
pub fn provider() -> Result<Box<dyn LlmProvider>, String> {
    let config = ProviderConfig::from_env().map_err(|e| e.to_string())?;
    Ok(Box::new(
        OpenAiCompatible::new(config).map_err(|e| e.to_string())?,
    ))
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split_once(']').map(|(h, _)| format!("{h}]"))?
    } else {
        host.split(':').next()?.to_owned()
    };
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    #[test]
    fn host_extraction() {
        assert_eq!(
            super::url_host("https://api.example.com/v1").as_deref(),
            Some("api.example.com")
        );
        assert_eq!(
            super::url_host("http://127.0.0.1:8000/v1").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            super::url_host("http://[::1]:8000/v1").as_deref(),
            Some("[::1]")
        );
    }
}
