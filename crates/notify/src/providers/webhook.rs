//! Kuma `webhook.js`.

use reqwest::RequestBuilder;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::multipart::Form;

use crate::config::{WebhookConfig, WebhookContentType, WebhookMethod};
use crate::error::SendError;
use crate::event::Notice;
use crate::js::Js;
use crate::providers::additional_headers;
use crate::{Delivered, SendContext, http, template};

pub(crate) async fn send(
    config: &WebhookConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let url = http::secret_url(config.url.as_ref(), "webhook URL")?;
    let extra_headers = additional_headers(config.additional_headers.as_ref())?;
    let body = Js::object([
        ("heartbeat", notice.heartbeat_json.clone()),
        ("monitor", notice.monitor_json.clone()),
        ("msg", notice.msg.as_str().into()),
    ]);
    let to_json = |value: &Js| serde_json::to_string(value).expect("Js always serializes");

    let request = match (config.method, config.content_type) {
        (Some(WebhookMethod::Get), _) => {
            let mut query = vec![("msg", notice.msg.clone())];
            if !notice.heartbeat_json.is_null() {
                query.push(("heartbeat", to_json(&notice.heartbeat_json)));
            }
            if !notice.monitor_json.is_null() {
                query.push(("monitor", to_json(&notice.monitor_json)));
            }
            ctx.http.get(url).query(&query)
        }
        (_, Some(WebhookContentType::FormData)) => ctx
            .http
            .post(url)
            .multipart(Form::new().text("data", to_json(&body))),
        (_, Some(WebhookContentType::Custom)) => {
            let rendered = template::render(
                config.custom_body.as_deref().unwrap_or_default(),
                &notice.msg,
                &notice.monitor_json,
                &notice.heartbeat_json,
            )?;
            // axios posts a string body as a form unless a header says otherwise.
            ctx.http
                .post(url)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(rendered)
        }
        _ => ctx.http.post(url).json(&body),
    };
    http::deliver(with_headers(request, extra_headers)?).await
}

/// Additional headers replace computed ones of the same name, like Kuma's object spread.
fn with_headers(
    request: RequestBuilder,
    headers: Vec<(String, String)>,
) -> Result<RequestBuilder, SendError> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::try_from(name)
            .map_err(|_| SendError::Config("invalid additional header name".into()))?;
        let value = HeaderValue::try_from(value)
            .map_err(|_| SendError::Config(format!("invalid value for header {name}")))?;
        map.insert(name, value);
    }
    Ok(request.headers(map))
}
