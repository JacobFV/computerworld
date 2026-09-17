//! Shared wire helpers, without service domain semantics.
use cw_protocol::{HttpRequest, HttpResponse, Page, PageAction, PageElement, Result, SimError};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub fn load<T: DeserializeOwned + Default>(value: &Value) -> Result<T> {
    if value.is_null() {
        Ok(T::default())
    } else {
        Ok(serde_json::from_value(value.clone())?)
    }
}
pub fn save<T: Serialize>(state: &mut Value, value: &T) -> Result<()> {
    *state = serde_json::to_value(value)?;
    Ok(())
}
pub fn path(req: &HttpRequest) -> String {
    url::Url::parse(&req.url)
        .map(|u| u.path().to_owned())
        .unwrap_or_else(|_| req.url.split('?').next().unwrap_or("/").to_owned())
}
pub fn query(req: &HttpRequest, key: &str) -> Option<String> {
    url::Url::parse(&req.url)
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}
pub fn body(req: &HttpRequest) -> Result<Value> {
    if req.body.is_empty() {
        return Ok(json!({}));
    }
    if req
        .header("content-type")
        .is_some_and(|h| h.starts_with("application/x-www-form-urlencoded"))
    {
        Ok(Value::Object(
            url::form_urlencoded::parse(&req.body)
                .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                .collect(),
        ))
    } else {
        Ok(serde_json::from_slice(&req.body)?)
    }
}
pub fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").into()
}
pub fn number(v: &Value, key: &str) -> Result<u64> {
    v.get(key)
        .and_then(|x| x.as_u64().or_else(|| x.as_str()?.parse().ok()))
        .ok_or_else(|| SimError::invalid(format!("missing or invalid {key}")))
}
pub fn strings(v: &Value, key: &str) -> Vec<String> {
    match v.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => vec![],
    }
}
pub fn error(status: u16, message: impl Into<String>) -> Result<HttpResponse> {
    HttpResponse::json(status, &json!({"error":message.into()}))
}
pub fn domain<T: Serialize>(result: std::result::Result<T, String>) -> Result<HttpResponse> {
    match result {
        Ok(v) => HttpResponse::json(200, &v),
        Err(e) => error(
            if e.contains("unavailable") || e.contains("writable") {
                403
            } else if e.contains("conflict") {
                409
            } else {
                400
            },
            e,
        ),
    }
}
pub fn heading(id: &str, text: impl Into<String>) -> PageElement {
    PageElement::Heading {
        id: id.into(),
        text: text.into(),
        level: 1,
    }
}
pub fn paragraph(id: &str, text: impl Into<String>) -> PageElement {
    PageElement::Text {
        id: id.into(),
        text: text.into(),
    }
}
pub fn link(id: &str, text: impl Into<String>, url: impl Into<String>) -> PageElement {
    PageElement::Link {
        id: id.into(),
        text: text.into(),
        url: url.into(),
    }
}
pub fn form(id: &str, url: &str, fields: &[(&str, &str, &str)]) -> PageElement {
    PageElement::Form {
        id: id.into(),
        action: PageAction {
            method: "POST".into(),
            url: url.into(),
            fields: BTreeMap::new(),
        },
        children: fields
            .iter()
            .map(|(id, label, value)| PageElement::Input {
                id: (*id).into(),
                label: (*label).into(),
                value: (*value).into(),
                placeholder: String::new(),
            })
            .chain(std::iter::once(PageElement::Button {
                id: format!("{id}-submit"),
                text: "Submit".into(),
                action: PageAction {
                    method: "POST".into(),
                    url: url.into(),
                    fields: BTreeMap::new(),
                },
            }))
            .collect(),
    }
}
pub fn page(title: &str, elements: Vec<PageElement>) -> Result<HttpResponse> {
    HttpResponse::page(&Page {
        version: 1,
        title: title.into(),
        elements,
    })
}
/// Explicit textual HTTP links become native navigation controls; no fetch occurs during rendering.
pub fn links(prefix: &str, text: &str) -> Vec<PageElement> {
    text.split_whitespace()
        .enumerate()
        .filter_map(|(i, word)| {
            let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
            let parsed = url::Url::parse(url).ok()?;
            matches!(parsed.scheme(), "http" | "https")
                .then(|| link(&format!("{prefix}-link-{i}"), url, url))
        })
        .collect()
}
