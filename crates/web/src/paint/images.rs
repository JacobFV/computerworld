//! Images a document carries inline: every `data:` URL an `<img src>`, `<input
//! type=image src>`, `<object data>`, `background-image: url()` or `content: url()`
//! names is decoded here, so a page like Acid2 renders without a network. A host with
//! a fetch merges what it downloaded into the same [`ImageMap`]; layout reads the
//! sizes through [`ImageSizes`] and paint the pixels through [`ImageCache`].

use std::collections::BTreeMap;

use super::{data_url, png, ImageCache, ImageMap, RgbaImage};
use crate::dom::{Document, NodeId};
use crate::layout::ImageSizes;
use crate::style::computed::{BackgroundImage, ComputedStyle, Content, ContentItem, StyleSet};

impl ImageSizes for ImageMap {
    fn size(&self, src: &str) -> Option<(u32, u32)> {
        self.0.get(src).map(|i| (i.width, i.height))
    }
}

impl ImageMap {
    /// Decodes every `data:` image URL the document and its computed styles refer
    /// to. Unknown formats and malformed data decode to nothing, so the element falls
    /// back as it would for a broken image.
    pub fn from_document(doc: &Document, styles: &StyleSet) -> ImageMap {
        let mut map = ImageMap::default();
        for url in document_image_urls(doc, styles) {
            map.insert_data_url(&url);
        }
        map
    }

    /// Adds a decoded image.
    pub fn insert(&mut self, url: &str, image: RgbaImage) {
        self.0.insert(url.to_owned(), image);
    }

    /// Decodes `url` if it is a `data:` URL of a supported image format and records
    /// it under the URL as written. Returns whether an image was added.
    pub fn insert_data_url(&mut self, url: &str) -> bool {
        if self.0.contains_key(url) {
            return true;
        }
        match decode_data_image(url) {
            Some(img) => {
                self.0.insert(url.to_owned(), img);
                true
            }
            None => false,
        }
    }
}

/// Decodes a `data:` URL holding a PNG. Other formats are not decoded (yet).
pub fn decode_data_image(url: &str) -> Option<RgbaImage> {
    let (_mime, bytes) = data_url::decode(url)?;
    // Sniff rather than trust the media type: browsers do, and a `data:image/png`
    // that holds nothing decodable is a broken image either way.
    if png::is_png(&bytes) {
        png::decode_png(&bytes)
    } else {
        None
    }
}

/// Every image URL the document names, in document order, deduplicated: element
/// sources first, then the computed styles of each element and its pseudo-elements.
pub fn document_image_urls(doc: &Document, styles: &StyleSet) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |u: &str| {
        let u = u.trim();
        if !u.is_empty() && !out.iter().any(|x| x == u) {
            out.push(u.to_owned());
        }
    };
    for node in doc.descendants(Document::ROOT) {
        if let Some(src) = element_image_source(doc, node) {
            push(src);
        }
        for style in [styles.get(node), styles.before(node), styles.after(node), styles.marker(node)].into_iter().flatten() {
            for u in style_image_urls(style) {
                push(u);
            }
        }
    }
    out
}

/// The URL an element loads as its replaced content: `<img src>`, `<input
/// type=image src>`, `<object data>`, `<embed src>`.
pub fn element_image_source(doc: &Document, node: NodeId) -> Option<&str> {
    let tag = doc.tag(node)?;
    match tag {
        "img" | "embed" => doc.attr(node, "src"),
        "input" if doc.attr(node, "type").is_some_and(|t| t.eq_ignore_ascii_case("image")) => doc.attr(node, "src"),
        "object" => doc.attr(node, "data"),
        _ => None,
    }
}

/// The `url()` images a computed style paints: background layers and `content`.
pub fn style_image_urls(style: &ComputedStyle) -> impl Iterator<Item = &str> {
    let backgrounds = style.background.iter().filter_map(|l| match &l.image {
        BackgroundImage::Url(u) => Some(u.as_str()),
        _ => None,
    });
    let content: Box<dyn Iterator<Item = &str>> = match &style.content {
        Content::Items(items) => Box::new(items.iter().filter_map(|i| match i {
            ContentItem::Url(u) => Some(u.as_str()),
            _ => None,
        })),
        _ => Box::new(std::iter::empty()),
    };
    backgrounds.chain(content)
}

/// An image cache over several maps: the first that knows a URL answers.
pub struct Layered<'a>(pub Vec<&'a dyn ImageCache>);

impl ImageCache for Layered<'_> {
    fn image(&self, url: &str) -> Option<&RgbaImage> {
        self.0.iter().find_map(|c| c.image(url))
    }
}

impl From<BTreeMap<String, RgbaImage>> for ImageMap {
    fn from(m: BTreeMap<String, RgbaImage>) -> ImageMap {
        ImageMap(m)
    }
}
