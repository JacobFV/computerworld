//! Optional website: authored files (HTML, CSS, JavaScript, pictures) served with
//! their media types, `Page` seeds served as HTML through the `cw_web::page`
//! converter (or as Page JSON when the site sets `"format": "page"`), and a small
//! shared JSON record API.
use cw_protocol::{HttpRequest, HttpResponse, Page, Result, SimError};
use cw_sdk::{Registry, Service, ServiceContext};
use cw_service_common as wire;
use cw_service_common::html::{el, Document as HtmlDocument};
use serde_json::{json, Value};
/// How `pages` are served: `html` (the default) converts each seed through
/// `cw_web::page::to_document`, so the site renders through the web engine with its
/// current look; `page` keeps the native Page media type for anything that must stay
/// JSON (a client that reads the element tree directly).
const FORMATS: &[&str] = &["html", "page"];
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
        for key in ["pages", "records", "assets", "files"] {
            if initial.get(key).is_none() {
                initial[key] = json!({});
            }
            if !initial[key].is_object() {
                return Err(SimError::invalid(format!("{key} must be an object")));
            }
        }
        wire::variant(&initial, "format", FORMATS)?;
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
        for (path, file) in initial["files"].as_object().unwrap() {
            if !path.starts_with('/') || path.ends_with('/') {
                return Err(SimError::invalid("file paths must be absolute and name a file"));
            }
            let ok = match file {
                Value::String(_) => true,
                Value::Object(o) => {
                    o.get("text").is_some_and(Value::is_string)
                        || o.get("bytes").is_some_and(|b| serde_json::from_value::<Vec<u8>>(b.clone()).is_ok())
                }
                _ => false,
            };
            if !ok {
                return Err(SimError::invalid(
                    "files are a string, or an object with text or bytes",
                ));
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
            let records = state["records"].as_object();
            let list = el("dl").id("records-list").each(records.into_iter().flatten(), |(k, v)| {
                cw_service_common::html::fragment([
                    el("dt").id(format!("record-{k}")).text(k.as_str()),
                    el("dd").id(format!("record-{k}-value")).text(v.to_string()),
                ])
            });
            let doc = HtmlDocument::new("Records")
                .lang("en")
                .stylesheet(RECORDS_CSS)
                .body([el("h1").id("records").text("Shared records"), list]);
            return match wire::variant(state, "format", FORMATS)?.as_str() {
                "page" => {
                    let mut elements = vec![wire::heading("records", "Shared records")];
                    for (k, v) in records.into_iter().flatten() {
                        elements.push(wire::paragraph(&format!("record-{k}"), format!("{k}: {v}")));
                    }
                    wire::page("Records", elements)
                }
                _ => wire::html::page(&doc),
            };
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
        if let Some(v) = state["pages"].get(&path) {
            let page = serde_json::from_value::<Page>(v.clone())?;
            return match wire::variant(state, "format", FORMATS)?.as_str() {
                "page" => HttpResponse::page(&page),
                _ => Ok(cw_service_common::html::HtmlResponse::new(200, cw_web::page::to_document(&page)).into()),
            };
        }
        if let Some(response) = serve_file(state, &path)? {
            return Ok(response);
        }
        wire::error(404, "page not found")
    }
}
/// The `/records` page's stylesheet: a plain list, readable, nothing to fetch.
const RECORDS_CSS: &str = "body { font-family: sans-serif; margin: 16px; color: #202124 } h1 { font-size: 22px } dt { font-weight: bold; margin-top: 8px } dd { margin: 0; font-family: monospace; white-space: pre-wrap }";
/// An authored file: `path` itself, `index.html` for a directory path, and a redirect
/// to the directory form for a directory named without its slash, as web servers do.
fn serve_file(state: &Value, path: &str) -> Result<Option<HttpResponse>> {
    let files = state["files"].as_object();
    let Some(files) = files else { return Ok(None) };
    let candidate = if path.ends_with('/') { format!("{path}index.html") } else { path.to_owned() };
    if let Some(file) = files.get(&candidate) {
        let (body, declared) = match file {
            Value::String(text) => (text.as_bytes().to_vec(), None),
            Value::Object(o) => {
                let body = match (o.get("text"), o.get("bytes")) {
                    (Some(Value::String(t)), _) => t.as_bytes().to_vec(),
                    (_, Some(b)) => serde_json::from_value::<Vec<u8>>(b.clone())?,
                    _ => Vec::new(),
                };
                (body, o.get("content_type").and_then(Value::as_str).map(str::to_owned))
            }
            _ => return Ok(None),
        };
        let content_type = declared.unwrap_or_else(|| content_type_of(&candidate).to_owned());
        return Ok(Some(HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::from([("content-type".to_owned(), content_type)]),
            body,
        }));
    }
    if !path.ends_with('/') && files.contains_key(&format!("{path}/index.html")) {
        return Ok(Some(HttpResponse {
            status: 301,
            headers: std::collections::BTreeMap::from([("location".to_owned(), format!("{path}/"))]),
            body: Vec::new(),
        }));
    }
    Ok(None)
}
/// The media type a file extension is served with.
pub fn content_type_of(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
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
mod files_tests {
    use super::*;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "a".into(),
            source: "pc".into(),
            tick: 0,
            seed: 1,
            instance: "site".into(),
        }
    }
    fn get(state: &mut Value, path: &str) -> HttpResponse {
        StaticSite
            .handle(
                state,
                &ctx(),
                &HttpRequest {
                    method: "GET".into(),
                    url: format!("http://site.test{path}"),
                    headers: Default::default(),
                    body: vec![],
                },
            )
            .unwrap()
    }
    #[test]
    fn html_directories_are_served_with_their_media_types() {
        let mut state = StaticSite
            .initialize(
                json!({"files":{
                    "/index.html":"<h1>Home</h1>",
                    "/docs/index.html":{"text":"<p>Docs</p>"},
                    "/style.css":"h1{color:red}",
                    "/app.js":"console.log(1)",
                    "/logo.png":{"bytes":[137,80,78,71]},
                    "/notes.txt":{"text":"plain","content_type":"text/x-notes"}
                }}),
                &ctx(),
            )
            .unwrap();
        let home = get(&mut state, "/");
        assert_eq!(home.header("content-type"), Some("text/html; charset=utf-8"));
        assert_eq!(home.body, b"<h1>Home</h1>");
        assert_eq!(get(&mut state, "/docs/").body, b"<p>Docs</p>");
        let redirect = get(&mut state, "/docs");
        assert_eq!((redirect.status, redirect.header("location")), (301, Some("/docs/")));
        assert_eq!(get(&mut state, "/style.css").header("content-type"), Some("text/css; charset=utf-8"));
        assert_eq!(get(&mut state, "/app.js").header("content-type"), Some("text/javascript; charset=utf-8"));
        let logo = get(&mut state, "/logo.png");
        assert_eq!(logo.header("content-type"), Some("image/png"));
        assert_eq!(logo.body, vec![137, 80, 78, 71]);
        assert_eq!(get(&mut state, "/notes.txt").header("content-type"), Some("text/x-notes"));
        assert_eq!(get(&mut state, "/missing.html").status, 404);
        assert!(StaticSite.initialize(json!({"files":{"relative.html":"x"}}), &ctx()).is_err());
        assert!(StaticSite.initialize(json!({"files":{"/x.html":7}}), &ctx()).is_err());
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

#[cfg(test)]
mod page_format_tests {
    use super::*;
    use cw_service_common::html::validate_strict;
    fn ctx() -> ServiceContext {
        ServiceContext {
            actor: "a".into(),
            source: "pc".into(),
            tick: 0,
            seed: 1,
            instance: "site".into(),
        }
    }
    fn get(state: &mut Value, path: &str) -> HttpResponse {
        StaticSite
            .handle(state, &ctx(), &HttpRequest::get(format!("http://site.test{path}")))
            .unwrap()
    }
    fn seed() -> Value {
        json!({"pages":{"/":{"version":1,"title":"Atlas","theme":{"accent":"#c00","content_width":700},"elements":[
            {"kind":"heading","id":"h","text":"Welcome","level":1},
            {"kind":"text","id":"intro","text":"A synthetic site"},
            {"kind":"link","id":"docs","text":"Docs","url":"/docs"},
            {"kind":"form","id":"search","action":{"method":"GET","url":"/find","fields":{"q":"$q"}},
             "children":[{"kind":"input","id":"q","label":"Query","value":"","placeholder":""},
                         {"kind":"button","id":"go","text":"Go","action":{"method":"GET","url":"/find","fields":{"q":"$q"}}}]}
        ]}}})
    }
    /// Page seeds are HTML by default: the converter keeps every id, the form and the link,
    /// and the result passes the strict validator, so every existing site renders through the
    /// engine unchanged.
    #[test]
    fn page_seeds_are_served_as_strict_html_with_their_ids() {
        let mut state = StaticSite.initialize(seed(), &ctx()).unwrap();
        let home = get(&mut state, "/");
        assert_eq!(home.status, 200);
        assert_eq!(home.header("content-type"), Some("text/html; charset=utf-8"));
        let html = String::from_utf8(home.body).unwrap();
        validate_strict(&html).unwrap();
        let dom = cw_web::html::parse(&html);
        for id in ["h", "intro", "docs", "search", "q", "go"] {
            assert_eq!(dom.by_id(id).len(), 1, "#{id}");
        }
        let form = dom.by_id("search")[0];
        assert_eq!(dom.attr(form, "action"), Some("/find"));
        assert_eq!(dom.attr(dom.by_id("docs")[0], "href"), Some("/docs"));
        assert!(dom.is(dom.by_id("q")[0], "input"));
        let records = get(&mut state, "/records");
        assert_eq!(records.header("content-type"), Some("text/html; charset=utf-8"));
        validate_strict(&String::from_utf8(records.body).unwrap()).unwrap();
    }
    /// `"format": "page"` is the escape hatch: the native media type, byte for byte the seed.
    #[test]
    fn the_page_format_keeps_the_native_media_type() {
        let mut seed = seed();
        seed["format"] = json!("page");
        let mut state = StaticSite.initialize(seed, &ctx()).unwrap();
        let home = get(&mut state, "/");
        assert_eq!(home.header("content-type"), Some(cw_protocol::PAGE_MEDIA_TYPE));
        let page: Page = serde_json::from_slice(&home.body).unwrap();
        assert_eq!(page.title, "Atlas");
        assert_eq!(get(&mut state, "/records").header("content-type"), Some(cw_protocol::PAGE_MEDIA_TYPE));
        let mut bad = json!({"format": "xml"});
        bad["pages"] = json!({});
        assert!(StaticSite.initialize(bad, &ctx()).is_err());
    }
}
