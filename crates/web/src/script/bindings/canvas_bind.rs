//! The `<canvas>` natives: one `canvasOp(node, op, ...args)` entry over
//! `canvas::CanvasState`, plus state get/set, `measureText`, image data and
//! `toDataURL`.

use cw_jsvm::value::{Args, JsResult, Obj, TypedKind, Value};
use cw_jsvm::vm::Vm;

use super::{arg_node, arg_num, arg_str, array_values, inner, node_of, string_val};
use crate::dom::NodeId;
use crate::paint::RgbaImage;
use crate::script::canvas::{color_string, parse_css_color, parse_font, CanvasState, Paint};

fn with_canvas<R>(vm: &mut Vm, n: NodeId, f: impl FnOnce(&mut CanvasState) -> R) -> R {
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    if !i.canvases.contains_key(&n) {
        let w = i.doc.attr(n, "width").and_then(|v| v.trim().parse().ok()).unwrap_or(300);
        let h = i.doc.attr(n, "height").and_then(|v| v.trim().parse().ok()).unwrap_or(150);
        i.canvases.insert(n, CanvasState::new(w, h));
    }
    let r = f(i.canvases.get_mut(&n).unwrap());
    i.touch();
    r
}

fn num(vm: &mut Vm, a: &Args, i: usize) -> JsResult<f64> {
    let v = arg_num(vm, a, i)?;
    Ok(if v.is_finite() { v } else { 0.0 })
}

fn canvas_op(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let op = arg_str(vm, a, 1)?;
    let f = |vm: &mut Vm, i: usize| num(vm, a, i + 2);
    match op.as_str() {
        "fillRect" | "strokeRect" | "clearRect" => {
            let (x, y, w, h) = (f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?);
            with_canvas(vm, n, |c| match op.as_str() {
                "fillRect" => c.fill_rect(x, y, w, h),
                "strokeRect" => c.stroke_rect(x, y, w, h),
                _ => c.clear_rect(x, y, w, h),
            });
        }
        "beginPath" => with_canvas(vm, n, |c| c.begin_path()),
        "closePath" => with_canvas(vm, n, |c| c.close_path()),
        "moveTo" | "lineTo" => {
            let (x, y) = (f(vm, 0)?, f(vm, 1)?);
            with_canvas(vm, n, |c| if op == "moveTo" { c.move_to(x, y) } else { c.line_to(x, y) });
        }
        "rect" => {
            let (x, y, w, h) = (f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?);
            with_canvas(vm, n, |c| c.rect(x, y, w, h));
        }
        "arc" => {
            let (x, y, r, s, e) = (f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?, f(vm, 4)?);
            let anti = a.arg(7).truthy();
            with_canvas(vm, n, |c| c.arc(x, y, r, s, e, anti));
        }
        "ellipse" => {
            let (x, y, rx, ry, rot, s, e) = (f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?, f(vm, 4)?, f(vm, 5)?, f(vm, 6)?);
            let anti = a.arg(9).truthy();
            with_canvas(vm, n, |c| c.ellipse(x, y, rx, ry, rot, s, e, anti));
        }
        "arcTo" => {
            let (x1, y1, x2, y2, r) = (f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?, f(vm, 4)?);
            with_canvas(vm, n, |c| c.arc_to(x1, y1, x2, y2, r));
        }
        "quadraticCurveTo" => {
            let (cx, cy, x, y) = (f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?);
            with_canvas(vm, n, |c| c.quadratic_curve_to(cx, cy, x, y));
        }
        "bezierCurveTo" => {
            let (a1, b1, a2, b2, x, y) = (f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?, f(vm, 4)?, f(vm, 5)?);
            with_canvas(vm, n, |c| c.bezier_curve_to(a1, b1, a2, b2, x, y));
        }
        "fill" => {
            let even_odd = matches!(&a.arg(2), Value::Str(s) if s.as_str() == "evenodd");
            with_canvas(vm, n, |c| c.fill(even_odd));
        }
        "stroke" => with_canvas(vm, n, |c| c.stroke()),
        "clip" => with_canvas(vm, n, |c| c.clip()),
        "save" => with_canvas(vm, n, |c| c.save()),
        "restore" => with_canvas(vm, n, |c| c.restore()),
        "translate" => {
            let (x, y) = (f(vm, 0)?, f(vm, 1)?);
            with_canvas(vm, n, |c| c.translate(x, y));
        }
        "scale" => {
            let (x, y) = (f(vm, 0)?, f(vm, 1)?);
            with_canvas(vm, n, |c| c.scale(x, y));
        }
        "rotate" => {
            let r = f(vm, 0)?;
            with_canvas(vm, n, |c| c.rotate(r));
        }
        "transform" | "setTransform" => {
            let m = [f(vm, 0)?, f(vm, 1)?, f(vm, 2)?, f(vm, 3)?, f(vm, 4)?, f(vm, 5)?];
            with_canvas(vm, n, |c| if op == "transform" { c.transform(m) } else { c.set_transform(m) });
        }
        "resetTransform" => with_canvas(vm, n, |c| c.set_transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])),
        "fillText" | "strokeText" => {
            let text = arg_str(vm, a, 2)?;
            let (x, y) = (f(vm, 1)?, f(vm, 2)?);
            let max = if a.arg(5).is_undefined() { None } else { Some(f(vm, 3)?) };
            with_canvas(vm, n, |c| c.fill_text(&text, x, y, max, op == "strokeText"));
        }
        "measureText" => {
            let text = arg_str(vm, a, 2)?;
            let w = with_canvas(vm, n, |c| c.measure_text(&text));
            return Ok(Value::Num(w));
        }
        "isPointInPath" => {
            let (x, y) = (f(vm, 0)?, f(vm, 1)?);
            let eo = matches!(&a.arg(4), Value::Str(s) if s.as_str() == "evenodd");
            let r = with_canvas(vm, n, |c| c.is_point_in_path(x, y, eo));
            return Ok(Value::Bool(r));
        }
        "resize" => {
            let (w, h) = (f(vm, 0)? as u32, f(vm, 1)? as u32);
            with_canvas(vm, n, |c| c.resize(w, h));
        }
        "drawImage" => {
            // (source node, sx, sy, sw, sh, dx, dy, dw, dh) with the source rect
            // already resolved by the prelude.
            let src = node_of(&a.arg(2));
            let vals: Vec<f64> = (3..11).map(|i| arg_num(vm, a, i).unwrap_or(0.0)).collect();
            let img = match src {
                Some(s) => inner(vm).borrow().canvases.get(&s).map(|c| c.image()),
                None => None,
            };
            if let Some(img) = img {
                with_canvas(vm, n, |c| c.draw_image(&img, vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6], vals[7]));
            }
        }
        "toDataURL" => {
            let s = with_canvas(vm, n, |c| c.to_data_url());
            return Ok(string_val(s));
        }
        "dirty" => {
            let d = inner(vm).borrow().canvases.get(&n).map(|c| c.dirty).unwrap_or(false);
            return Ok(Value::Bool(d));
        }
        _ => {}
    }
    Ok(Value::Undefined)
}

/// `W.canvasState(node, prop)` / `W.canvasSetState(node, prop, value)`.
fn canvas_state(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let prop = arg_str(vm, a, 1)?;
    let v = with_canvas(vm, n, |c| {
        let s = &c.state;
        match prop.as_str() {
            "fillStyle" => Value::string(s.fill_text.clone()),
            "strokeStyle" => Value::string(s.stroke_text.clone()),
            "lineWidth" => Value::Num(s.line_width),
            "globalAlpha" => Value::Num(s.global_alpha),
            "font" => Value::string(s.font.clone()),
            "textAlign" => Value::string(s.text_align.clone()),
            "textBaseline" => Value::string(s.text_baseline.clone()),
            "lineCap" => Value::string(s.line_cap.clone()),
            "lineJoin" => Value::string(s.line_join.clone()),
            "globalCompositeOperation" => Value::string(s.composite.clone()),
            "imageSmoothingEnabled" => Value::Bool(s.image_smoothing),
            "miterLimit" => Value::Num(s.miter_limit),
            "shadowBlur" => Value::Num(s.shadow_blur),
            "shadowColor" => Value::string(s.shadow_color.clone()),
            "width" => Value::Num(c.width as f64),
            "height" => Value::Num(c.height as f64),
            _ => Value::Undefined,
        }
    });
    Ok(v)
}

fn canvas_set_state(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let prop = arg_str(vm, a, 1)?;
    let v = a.arg(2);
    match prop.as_str() {
        "fillStyle" | "strokeStyle" => {
            // A string colour or a gradient object made by `createGradient`.
            let paint = match &v {
                Value::Obj(o) if o.own_value("%gradient").is_some() => gradient_of(vm, o)?,
                _ => {
                    let s = vm.to_string(&v)?.to_string();
                    parse_css_color(&s).map(|c| (Paint::Solid(c), color_string(c)))
                }
            };
            if let Some((p, text)) = paint {
                with_canvas(vm, n, |c| {
                    if prop == "fillStyle" {
                        c.state.fill = p;
                        c.state.fill_text = text;
                    } else {
                        c.state.stroke = p;
                        c.state.stroke_text = text;
                    }
                });
            }
        }
        "font" => {
            let s = vm.to_string(&v)?.to_string();
            if let Some((size, bold, italic, family)) = parse_font(&s) {
                with_canvas(vm, n, |c| {
                    c.state.font = s.clone();
                    c.state.font_size = size;
                    c.state.font_bold = bold;
                    c.state.font_italic = italic;
                    c.state.font_family = family;
                });
            }
        }
        "lineWidth" | "globalAlpha" | "miterLimit" | "shadowBlur" => {
            let x = vm.to_number(&v)?;
            if x.is_finite() {
                with_canvas(vm, n, |c| match prop.as_str() {
                    "lineWidth" => c.state.line_width = x,
                    "globalAlpha" => c.state.global_alpha = x.clamp(0.0, 1.0),
                    "miterLimit" => c.state.miter_limit = x,
                    _ => c.state.shadow_blur = x,
                });
            }
        }
        "imageSmoothingEnabled" => with_canvas(vm, n, |c| c.state.image_smoothing = v.truthy()),
        _ => {
            let s = vm.to_string(&v)?.to_string();
            with_canvas(vm, n, |c| match prop.as_str() {
                "textAlign" => {
                    if matches!(s.as_str(), "start" | "end" | "left" | "right" | "center") {
                        c.state.text_align = s.clone()
                    }
                }
                "textBaseline" => {
                    if matches!(s.as_str(), "top" | "hanging" | "middle" | "alphabetic" | "ideographic" | "bottom") {
                        c.state.text_baseline = s.clone()
                    }
                }
                "lineCap" => c.state.line_cap = s.clone(),
                "lineJoin" => c.state.line_join = s.clone(),
                "globalCompositeOperation" => c.state.composite = s.clone(),
                "shadowColor" => c.state.shadow_color = s.clone(),
                _ => {}
            });
        }
    }
    Ok(Value::Undefined)
}

/// A gradient object made by the prelude: `{%gradient: 'linear'|'radial',
/// coords: [...], stops: [[offset, color]...]}`.
fn gradient_of(vm: &mut Vm, o: &Obj) -> JsResult<Option<(Paint, String)>> {
    let kind = vm.get_str(&Value::Obj(o.clone()), "%gradient")?;
    let coords = vm.get_str(&Value::Obj(o.clone()), "coords")?;
    let coords: Vec<f64> = array_values(vm, &coords)?.iter().map(|v| match v {
        Value::Num(n) => *n,
        _ => 0.0,
    }).collect();
    let stops_v = vm.get_str(&Value::Obj(o.clone()), "stops")?;
    let mut stops = Vec::new();
    for s in array_values(vm, &stops_v)? {
        let pair = array_values(vm, &s)?;
        if pair.len() == 2 {
            let off = vm.to_number(&pair[0])?;
            let col = vm.to_string(&pair[1])?.to_string();
            if let Some(c) = parse_css_color(&col) {
                stops.push((off, c));
            }
        }
    }
    stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let is_radial = matches!(&kind, Value::Str(s) if s.as_str() == "radial");
    let paint = if is_radial && coords.len() >= 6 {
        Paint::Radial { x: coords[3], y: coords[4], r: coords[5], stops }
    } else if coords.len() >= 4 {
        Paint::Linear { x0: coords[0], y0: coords[1], x1: coords[2], y1: coords[3], stops }
    } else {
        return Ok(None);
    };
    Ok(Some((paint, "[object CanvasGradient]".into())))
}

fn canvas_get_image_data(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let x = arg_num(vm, a, 1)? as i64;
    let y = arg_num(vm, a, 2)? as i64;
    let w = (arg_num(vm, a, 3)?.max(0.0) as u32).min(16384);
    let h = (arg_num(vm, a, 4)?.max(0.0) as u32).min(16384);
    let img = with_canvas(vm, n, |c| c.get_image_data(x, y, w, h));
    Ok(Value::Obj(vm.new_typed(TypedKind::Uint8Clamped, img.rgba, None)))
}

fn canvas_put_image_data(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let Value::Obj(data) = a.arg(1) else { return Ok(Value::Undefined) };
    let w = arg_num(vm, a, 2)? as u32;
    let h = arg_num(vm, a, 3)? as u32;
    let x = arg_num(vm, a, 4)? as i64;
    let y = arg_num(vm, a, 5)? as i64;
    let bytes = vm.typed_bytes(&data).unwrap_or_default();
    if bytes.len() as u64 != w as u64 * h as u64 * 4 {
        return Ok(Value::Undefined);
    }
    let img = RgbaImage::new(w, h, bytes);
    with_canvas(vm, n, |c| c.put_image_data(&img, x, y));
    Ok(Value::Undefined)
}

pub fn install(vm: &mut Vm, w: &Obj) {
    vm.method(w, "canvasOp", 10, canvas_op);
    vm.method(w, "canvasState", 2, canvas_state);
    vm.method(w, "canvasSetState", 3, canvas_set_state);
    vm.method(w, "canvasGetImageData", 5, canvas_get_image_data);
    vm.method(w, "canvasPutImageData", 6, canvas_put_image_data);
}
