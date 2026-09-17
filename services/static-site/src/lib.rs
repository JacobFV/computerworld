//! Optional native-page website plus a small shared JSON record API.
use cw_protocol::{HttpRequest, HttpResponse, Page, Result, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as wire;
use serde_json::{json, Value};
pub struct StaticSite;
pub fn register(registry: &mut Registry) -> Result<()> {
    registry.register(StaticSite)
}
impl Service for StaticSite {
    fn kind(&self) -> &str {
        "static-site"
    }
    fn initialize(&self, mut initial: Value, _: &ServiceContext) -> Result<Value> {
        if initial.is_null() {
            initial = json!({});
        }
        if !initial.is_object() {
            return Err(SimError::invalid("static-site state must be an object"));
        }
        for key in ["pages", "records", "assets"] {
            if initial.get(key).is_none() {
                initial[key] = json!({});
            }
            if !initial[key].is_object() {
                return Err(SimError::invalid(format!("{key} must be an object")));
            }
        }
        for (path, page) in initial["pages"].as_object().unwrap() {
            if !path.starts_with('/') {
                return Err(SimError::invalid("page paths must be absolute"));
            }
            let p: Page = serde_json::from_value(page.clone())?;
            if p.version != 1 {
                return Err(SimError::invalid("unsupported native page version"));
            }
        }
        for (path, asset) in initial["assets"].as_object().unwrap() {
            if !path.starts_with('/')
                || asset["content_type"].as_str().is_none()
                || (asset.get("json").is_none() && asset.get("bytes").is_none())
            {
                return Err(SimError::invalid(
                    "assets need absolute paths, content_type and json or bytes",
                ));
            }
            if let Some(bytes) = asset.get("bytes") {
                let _: Vec<u8> = serde_json::from_value(bytes.clone())?;
            }
        }
        Ok(initial)
    }
    fn handle(
        &self,
        state: &mut Value,
        ctx: &ServiceContext,
        req: &HttpRequest,
    ) -> Result<HttpResponse> {
        let path = wire::path(req);
        if !allowed(state, "readers", &ctx.actor) {
            return wire::error(403, "site read denied");
        }
        if path == "/api/records" {
            if req.method != "GET" {
                return wire::error(405, "method not allowed");
            }
            return HttpResponse::json(200, &state["records"]);
        }
        if let Some(key) = path.strip_prefix("/api/records/") {
            if key.is_empty() || key.contains('/') {
                return wire::error(404, "record not found");
            }
            if req.method != "GET" && !allowed(state, "writers", &ctx.actor) {
                return wire::error(403, "site write denied");
            }
            let records = state["records"]
                .as_object_mut()
                .ok_or_else(|| SimError::invalid("site records must be an object"))?;
            return match req.method.as_str() {
                "GET" => match records.get(key) {
                    Some(v) => HttpResponse::json(200, v),
                    None => wire::error(404, "record not found"),
                },
                "PUT" | "POST" => {
                    let value = match wire::body(req) {
                        Ok(v) => v,
                        Err(_) => return wire::error(400, "malformed request body"),
                    };
                    let status = if records.contains_key(key) { 200 } else { 201 };
                    records.insert(key.into(), value.clone());
                    HttpResponse::json(status, &value)
                }
                "DELETE" => {
                    if records.remove(key).is_some() {
                        Ok(HttpResponse::text(204, ""))
                    } else {
                        wire::error(404, "record not found")
                    }
                }
                _ => wire::error(405, "method not allowed"),
            };
        }
        if req.method != "GET" {
            return wire::error(405, "method not allowed");
        }
        if path == "/records" {
            let mut elements = vec![wire::heading("records", "Shared records")];
            if let Some(records) = state["records"].as_object() {
                for (k, v) in records {
                    elements.push(wire::paragraph(&format!("record-{k}"), format!("{k}: {v}")));
                }
            }
            return wire::page("Records", elements);
        }
        if let Some(asset) = state["assets"].get(&path) {
            let body = if let Some(bytes) = asset.get("bytes") {
                serde_json::from_value::<Vec<u8>>(bytes.clone())?
            } else {
                serde_json::to_vec(&asset["json"])?
            };
            return Ok(HttpResponse {
                status: 200,
                headers: std::collections::BTreeMap::from([(
                    "content-type".into(),
                    asset["content_type"]
                        .as_str()
                        .unwrap_or("application/octet-stream")
                        .into(),
                )]),
                body,
            });
        }
        match state["pages"].get(&path) {
            Some(v) => HttpResponse::page(&serde_json::from_value::<Page>(v.clone())?),
            None => wire::error(404, "page not found"),
        }
    }
}
fn allowed(state: &Value, field: &str, actor: &str) -> bool {
    state
        .get(field)
        .and_then(Value::as_array)
        .is_none_or(|a| a.is_empty() || a.iter().any(|v| v.as_str() == Some(actor)))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn context(actor: &str) -> ServiceContext {
        ServiceContext {
            actor: actor.into(),
            source: actor.into(),
            tick: 0,
            seed: 0,
            instance: "site".into(),
        }
    }
    fn request(method: &str, path: &str, body: Value) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            url: format!("http://site.test{path}"),
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        }
    }
    #[test]
    fn records_are_shared_and_rendered_from_same_state() {
        let service = StaticSite;
        let mut state = service
            .initialize(json!({"writers":["alice"]}), &context("alice"))
            .unwrap();
        assert_eq!(
            service
                .handle(
                    &mut state,
                    &context("bob"),
                    &request("PUT", "/api/records/key", json!("x"))
                )
                .unwrap()
                .status,
            403
        );
        assert_eq!(
            service
                .handle(
                    &mut state,
                    &context("alice"),
                    &request("PUT", "/api/records/key", json!("unique-marker"))
                )
                .unwrap()
                .status,
            201
        );
        let response = service
            .handle(
                &mut state,
                &context("bob"),
                &request("GET", "/records", Value::Null),
            )
            .unwrap();
        assert!(String::from_utf8(response.body)
            .unwrap()
            .contains("unique-marker"));
        assert_eq!(
            service
                .handle(
                    &mut state,
                    &context("bob"),
                    &request("GET", "/missing", Value::Null)
                )
                .unwrap()
                .status,
            404
        );
    }
}

#[cfg(test)]
mod assets_tests {
    use super::*;
    #[test]
    fn native_image_response_is_served_over_http() {
        let ctx = ServiceContext {
            actor: "a".into(),
            source: "pc".into(),
            tick: 0,
            seed: 1,
            instance: "site".into(),
        };
        let mut state=StaticSite.initialize(json!({"assets":{"/pixel":{"content_type":"application/vnd.computerworld.rgba+json","json":{"width":1,"height":1,"rgba":[255,0,0,255]}}}}),&ctx).unwrap();
        let response = StaticSite
            .handle(
                &mut state,
                &ctx,
                &HttpRequest {
                    method: "GET".into(),
                    url: "http://site.test/pixel".into(),
                    headers: Default::default(),
                    body: vec![],
                },
            )
            .unwrap();
        assert_eq!(
            response.header("content-type"),
            Some("application/vnd.computerworld.rgba+json")
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&response.body).unwrap()["rgba"],
            json!([255, 0, 0, 255])
        );
    }
}
