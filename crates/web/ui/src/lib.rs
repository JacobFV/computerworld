//! `cw-ui`: runs React-syntax apps compiled by `cw-tsx` without a JS VM. This
//! commit holds the IR (`ir`) the compiler targets; the runtime follows.

pub mod ir;

/// The DOM attribute React writes for a host-element prop (`className` → `class`).
/// Names React passes through keep their spelling; the document lower-cases HTML
/// attribute names itself.
pub fn dom_attr_name(prop: &str) -> String {
    match prop {
        "className" => "class".into(),
        "htmlFor" => "for".into(),
        "acceptCharset" => "accept-charset".into(),
        "httpEquiv" => "http-equiv".into(),
        // SVG presentation attributes React spells in camelCase.
        "strokeWidth" => "stroke-width".into(),
        "strokeLinecap" => "stroke-linecap".into(),
        "strokeLinejoin" => "stroke-linejoin".into(),
        "strokeDasharray" => "stroke-dasharray".into(),
        "strokeDashoffset" => "stroke-dashoffset".into(),
        "strokeOpacity" => "stroke-opacity".into(),
        "fillOpacity" => "fill-opacity".into(),
        "fillRule" => "fill-rule".into(),
        "clipRule" => "clip-rule".into(),
        "clipPath" => "clip-path".into(),
        "fontSize" => "font-size".into(),
        "fontFamily" => "font-family".into(),
        "fontWeight" => "font-weight".into(),
        "textAnchor" => "text-anchor".into(),
        "dominantBaseline" => "dominant-baseline".into(),
        "stopColor" => "stop-color".into(),
        "stopOpacity" => "stop-opacity".into(),
        "xlinkHref" => "xlink:href".into(),
        n => n.to_owned(),
    }
}
