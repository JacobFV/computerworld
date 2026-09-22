//! Renders every native OS shell in representative states to PNG files.
//! Usage: cargo run --release --example os-gallery -- [output-dir] [theme,theme...]
//! The default output directory is `target/gallery`: these are pictures to look at
//! once, not something the repository keeps.
use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, World};
use serde_json::{json, Value};
use std::{fs::File, io::BufWriter, path::Path};

const MACHINE: &str = "alice-mac";
struct Gallery {
    world: World,
    session: String,
    width: u32,
    height: u32,
    out: String,
    theme: String,
}
impl Gallery {
    fn new(theme: &str, out: &str) -> Self {
        let mut definition = reference_world();
        definition.metadata["desktop_themes"] = json!({MACHINE: format!("virtual-{theme}")});
        // Native applications, as in the live site world: `url` is the service each
        // one talks to, not a page the browser is sent to.
        definition.metadata["desktop_apps"] = json!([
            {"id":"mail","label":"Mail","kind":"native","url":"http://mail.internal/","icon":"mail"},
            {"id":"calendar","label":"Calendar","kind":"native","url":"http://calendar.internal/","icon":"calendar"},
            {"id":"messages","label":"Messages","kind":"native","url":"http://messages.internal/","icon":"messages"},
            {"id":"docs","label":"Documents","kind":"native","url":"http://docs.internal/","icon":"docs"},
            {"id":"contacts","label":"Contacts","kind":"native","url":"http://mail.internal/|http://messages.internal/","icon":"contacts"},
            {"id":"notes","label":"Notes","kind":"native","url":"","icon":"notes"},
            {"id":"settings","label":"Settings","kind":"native","url":"","icon":"settings"},
            {"id":"calculator","label":"Calculator","kind":"native","url":"","icon":"calculator"},
            {"id":"clock","label":"Clock","kind":"native","url":"","icon":"clock"}
        ]);
        if let Some(computer) = definition.computers.iter_mut().find(|c| c.id == MACHINE) {
            for app in [
                "terminal",
                "editor",
                "browser",
                "files",
                "mail",
                "calendar",
                "messages",
                "docs",
                "contacts",
                "notes",
                "settings",
                "calculator",
                "clock",
            ] {
                if !computer.installed_apps.iter().any(|a| a == app) {
                    computer.installed_apps.push(app.into());
                }
            }
        }
        let mut world = World::new(definition, 7).expect("reference world");
        let session = world
            .environment(EnvironmentConfig::desktop("alice", MACHINE))
            .expect("desktop session");
        let (width, height) = if matches!(theme, "ios" | "android") {
            (390, 780)
        } else {
            (1280, 800)
        };
        Self {
            world,
            session,
            width,
            height,
            out: out.into(),
            theme: theme.into(),
        }
    }
    fn act(&mut self, family: &str, op: &str, payload: Value) -> bool {
        let result = self
            .world
            .step(
                &self.session,
                vec![ActionEnvelope::new(family, op, MACHINE, payload)],
            )
            .expect("step");
        result.outcomes[0].success
    }
    fn click(&mut self, target: &str) -> bool {
        let scene = self
            .world
            .scene(&self.session, self.width, self.height)
            .expect("scene");
        let Some(node) = scene
            .nodes
            .iter()
            .rev()
            .find(|n| n.interaction.as_deref() == Some(target))
        else {
            return false;
        };
        let b = node.transform.bounds(node.bounds);
        let (x, y) = (b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
        let (width, height) = (self.width, self.height);
        self.act(
            "pointer.v1",
            "click",
            json!({"x":x,"y":y,"width":width,"height":height}),
        )
    }
    fn hover(&mut self, target: &str) {
        let scene = self
            .world
            .scene(&self.session, self.width, self.height)
            .expect("scene");
        if let Some(node) = scene
            .nodes
            .iter()
            .rev()
            .find(|n| n.interaction.as_deref() == Some(target))
        {
            let b = node.transform.bounds(node.bounds);
            let (x, y) = (b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
            let (width, height) = (self.width, self.height);
            self.act(
                "pointer.v1",
                "move",
                json!({"x":x,"y":y,"width":width,"height":height}),
            );
        }
    }
    fn shot(&mut self, name: &str) {
        let frame = self
            .world
            .render(&self.session, self.width, self.height)
            .expect("render");
        let path = Path::new(&self.out).join(format!("{}-{name}.png", self.theme));
        let mut encoder = png::Encoder::new(
            BufWriter::new(File::create(&path).expect("create png")),
            frame.width,
            frame.height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .and_then(|mut w| w.write_image_data(&frame.rgba))
            .expect("encode png");
        println!("{}", path.display());
    }
}
fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "target/gallery".into());
    let themes = args
        .next()
        .unwrap_or_else(|| "macos,windows,ubuntu,ios,android".into());
    std::fs::create_dir_all(&out).expect("output directory");
    for theme in themes.split(',') {
        let mobile = matches!(theme, "ios" | "android");
        let mut g = Gallery::new(theme, &out);
        g.shot("home");
        if g.click("shell:launcher") || g.click("shell:search") {
            g.shot("launcher");
            g.act("application.v1", "home", json!({}));
        }
        for panel in [
            "shell:panel:control",
            "shell:panel:quick",
            "shell:quick-settings",
            "shell:panel:notifications",
            "shell:notifications",
            "shell:panel:calendar",
            "shell:panel:spotlight",
            "shell:overview",
        ] {
            if g.click(panel) {
                g.shot(&format!("panel-{}", panel.rsplit(':').next().unwrap()));
                g.act("application.v1", "home", json!({}));
            }
        }
        g.act("application.v1", "launch", json!({"kind":"files"}));
        g.shot("files");
        if mobile {
            g.act("application.v1", "home", json!({}));
        }
        g.act("application.v1", "launch", json!({"kind":"terminal"}));
        g.act("keyboard.v1", "type", json!({"text":"ls -la"}));
        g.act("keyboard.v1", "key", json!({"key":"Enter"}));
        g.shot("terminal");
        if mobile {
            g.act("application.v1", "home", json!({}));
        }
        g.act("application.v1", "launch", json!({"kind":"editor"}));
        g.act(
            "keyboard.v1",
            "type",
            json!({"text":"Launch checklist\nConfirm the release owner signs off."}),
        );
        g.shot("editor");
        if mobile {
            g.act("application.v1", "home", json!({}));
        }
        g.act("application.v1", "launch", json!({"kind":"browser"}));
        g.act(
            "browser.v1",
            "navigate",
            json!({"url":"http://intranet.internal/"}),
        );
        g.shot("browser");
        if !mobile {
            g.hover("shell:launch:files");
            g.shot("multiwindow");
        }
        g.act("application.v1", "launch", json!({"kind":"mail"}));
        g.shot("mail");
        g.act("application.v1", "launch", json!({"kind":"calendar"}));
        g.shot("calendar");
        if g.click("shell:overview") {
            g.shot("overview");
        }
    }
}
