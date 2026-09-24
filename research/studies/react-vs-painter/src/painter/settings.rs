//! Settings / profile form with validation, hand-positioned with the Painter.
use cw_applications::desktop_scene::shared::Align;
use cw_applications::desktop_scene::Painter;
use cw_scene::{Color, Rect};

const BG: Color = Color(249, 250, 251, 255);
const WHITE: Color = Color(255, 255, 255, 255);
const BORDER: Color = Color(209, 213, 219, 255);
const LINE: Color = Color(229, 231, 235, 255);
const INK: Color = Color(17, 24, 39, 255);
const MUTED: Color = Color(107, 114, 128, 255);
const FAINT: Color = Color(156, 163, 175, 255);
const INDIGO: Color = Color(79, 70, 229, 255);
const RED: Color = Color(220, 38, 38, 255);
const RED_LINE: Color = Color(252, 165, 165, 255);

pub struct Profile {
    pub tab: usize,
    pub name: String,
    pub username: String,
    pub email: String,
    pub website: String,
    pub bio: String,
    pub focus: Option<&'static str>,
    pub submitted: bool,
    pub toast: bool,
    pub toggles: [bool; 4],
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            tab: 0,
            name: "Maya Chen".into(),
            username: "mayachen".into(),
            email: "maya@studio.design".into(),
            website: String::new(),
            bio: "Product designer. Previously at Figma and Linear. I like type, trains and tidy spreadsheets.".into(),
            focus: None,
            submitted: false,
            toast: false,
            toggles: [true, true, false, false],
        }
    }
}

impl Profile {
    fn errors(&self) -> Vec<(&'static str, &'static str)> {
        let mut e = Vec::new();
        if self.name.trim().is_empty() {
            e.push(("name", "Your name is required."));
        }
        if !self.username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || self.username.is_empty() {
            e.push(("username", "Usernames can only contain letters, numbers and underscores."));
        }
        let at = self.email.find('@');
        if at.is_none() || !self.email[at.unwrap()..].contains('.') {
            e.push(("email", "Enter a valid email address."));
        }
        if !self.website.is_empty() && !self.website.starts_with("https://") {
            e.push(("website", "Enter a URL that starts with https://"));
        }
        if self.bio.len() > 160 {
            e.push(("bio", "Keep your bio under 160 characters."));
        }
        e
    }

    pub fn click(&mut self, target: &str) {
        match target {
            "save" => {
                self.submitted = true;
                self.focus = None;
                if self.errors().is_empty() {
                    self.toast = true;
                }
            }
            "tab:0" => self.tab = 0,
            "tab:1" => self.tab = 1,
            "tab:2" => self.tab = 2,
            "field:username" => self.focus = Some("username"),
            "field:website" => self.focus = Some("website"),
            "field:email" => self.focus = Some("email"),
            t if t.starts_with("toggle:") => {
                let i: usize = t[7..].parse().unwrap_or(0);
                self.toggles[i] = !self.toggles[i];
            }
            _ => {}
        }
    }

    pub fn type_text(&mut self, text: &str) {
        match self.focus {
            Some("username") => self.username.push_str(text),
            Some("website") => self.website.push_str(text),
            Some("email") => self.email.push_str(text),
            _ => {}
        }
    }

    pub fn render(&self, p: &mut Painter) {
        p.scene.background = BG;
        p.label(160, 32, 400, "Settings", 24, INK, true, Align::Left);
        p.left(160, 66, 500, "Manage your profile, account and notifications.", 14, MUTED);
        for (i, (label, icon)) in [("Profile", "person"), ("Account", "lock"), ("Notifications", "bell")]
            .iter()
            .enumerate()
        {
            let r = Rect::new(160, 114 + i as i32 * 40, 208, 36);
            let on = i == self.tab;
            if on {
                p.border(r, WHITE, 6, LINE);
            }
            p.region(r, &format!("tab:{i}"), label);
            p.symbol(icon, 172, r.y + 10, 16, if on { INDIGO } else { FAINT });
            p.left(200, r.y + 9, 150, label, 14, if on { INDIGO } else { Color(75, 85, 99, 255) });
        }
        match self.tab {
            0 => self.profile(p),
            1 => self.account(p),
            _ => self.notifications(p),
        }
        if self.toast {
            let r = Rect::new(954, 690, 320, 80);
            p.z += 10;
            p.drop_shadow(r, 12, 12, 40, 6);
            p.border(r, WHITE, 12, LINE);
            p.symbol("check", 970, 706, 20, Color(16, 185, 129, 255));
            p.label(1002, 706, 220, "Profile saved", 14, INK, true, Align::Left);
            p.left(1002, 730, 220, "Your changes are live.", 14, MUTED);
            p.symbol("close", 1242, 706, 16, FAINT);
            p.z -= 10;
        }
    }

    fn field(&self, p: &mut Painter, x: i32, y: i32, w: u32, key: &'static str, label: &str, value: &str, icon: Option<&str>) -> i32 {
        let errors = if self.submitted { self.errors() } else { vec![] };
        let error = errors.iter().find(|(k, _)| *k == key).map(|(_, m)| *m);
        p.label(x, y, w, label, 14, INK, true, Align::Left);
        let r = Rect::new(x, y + 28, w, 36);
        p.border(r, WHITE, 6, if error.is_some() { RED_LINE } else { BORDER });
        p.region(r, &format!("field:{key}"), label);
        let tx = if let Some(icon) = icon {
            p.symbol(icon, x + 12, y + 38, 16, FAINT);
            x + 36
        } else {
            x + 12
        };
        p.left(tx, y + 37, w - 48, value, 14, if error.is_some() { Color(127, 29, 29, 255) } else { INK });
        if let Some(m) = error {
            p.symbol("info", x + w as i32 - 28, y + 38, 16, RED);
            p.symbol("info", x, y + 74, 16, RED);
            p.left(x + 22, y + 72, w - 22, m, 14, RED);
            return 100;
        }
        76
    }

    fn profile(&self, p: &mut Painter) {
        let card = Rect::new(408, 114, 712, 686);
        p.border(card, WHITE, 12, LINE);
        p.label(440, 140, 400, "Public profile", 16, INK, true, Align::Left);
        p.left(440, 166, 600, "This information will be shown on your profile and in comments.", 14, MUTED);
        let errors = if self.submitted { self.errors() } else { vec![] };
        let mut y = 206;
        if !errors.is_empty() {
            let h = 56 + errors.len() as u32 * 24;
            p.border(Rect::new(440, y, 648, h), Color(254, 242, 242, 255), 8, Color(254, 202, 202, 255));
            p.symbol("info", 456, y + 16, 20, Color(239, 68, 68, 255));
            let title = if errors.len() == 1 {
                "There is 1 problem with your profile".to_string()
            } else {
                format!("There are {} problems with your profile", errors.len())
            };
            p.label(488, y + 17, 560, &title, 14, Color(153, 27, 27, 255), true, Align::Left);
            for (i, (_, m)) in errors.iter().enumerate() {
                let ly = y + 44 + i as i32 * 24;
                p.circle(500, ly + 9, 2, Color(185, 28, 28, 255));
                p.left(508, ly, 560, m, 14, Color(185, 28, 28, 255));
            }
            y += h as i32 + 24;
        }
        // Avatar row
        p.circle(472, y + 32, 32, Color(168, 85, 247, 255));
        p.label(440, y + 22, 64, "MC", 20, WHITE, true, Align::Center);
        p.circle(494, y + 54, 12, WHITE);
        p.symbol("camera", 487, y + 47, 14, Color(75, 85, 99, 255));
        p.border(Rect::new(524, y + 8, 118, 32), WHITE, 6, BORDER);
        p.center(524, y + 15, 118, "Change photo", 14, INK);
        p.left(666, y + 15, 80, "Remove", 14, Color(75, 85, 99, 255));
        p.left(524, y + 48, 300, "JPG, PNG or GIF. 1 MB max.", 12, MUTED);
        y += 96;

        let h1 = self.field(p, 440, y, 312, "name", "Full name", &self.name, None);
        let h2 = {
            // Username with a fixed prefix box.
            let errors = if self.submitted { self.errors() } else { vec![] };
            let error = errors.iter().find(|(k, _)| *k == "username").map(|(_, m)| *m);
            p.label(776, y, 312, "Username", 14, INK, true, Align::Left);
            p.border(Rect::new(776, y + 28, 120, 36), BG, 6, BORDER);
            p.left(789, y + 37, 110, "studio.design/", 14, MUTED);
            p.border(Rect::new(895, y + 28, 193, 36), WHITE, 6, if error.is_some() { RED_LINE } else { BORDER });
            p.region(Rect::new(895, y + 28, 193, 36), "field:username", "Username");
            p.left(907, y + 37, 170, &self.username, 14, INK);
            if let Some(m) = error {
                p.symbol("info", 776, y + 74, 16, RED);
                p.left(798, y + 72, 290, m, 14, RED);
                100
            } else {
                76
            }
        };
        y += h1.max(h2) + 12;
        let h = self.field(p, 440, y, 424, "email", "Email address", &self.email, Some("at"));
        if h == 76 {
            p.left(440, y + 72, 424, "We'll only use this to send you receipts.", 14, MUTED);
        }
        p.label(888, y, 200, "Time zone", 14, INK, true, Align::Left);
        p.border(Rect::new(888, y + 28, 200, 36), Color(239, 239, 239, 255), 6, BORDER);
        p.left(900, y + 37, 160, "Europe/Lisbon", 14, INK);
        p.symbol("chevron-down", 1064, y + 38, 14, INK);
        y += 100 + 12;
        let website = if self.website.is_empty() { "https://example.com" } else { self.website.as_str() };
        y += self.field(p, 440, y, 648, "website", "Website", website, Some("globe")) + 12;
        p.label(440, y, 648, "About", 14, INK, true, Align::Left);
        p.border(Rect::new(440, y + 28, 648, 76), WHITE, 6, BORDER);
        p.paragraph(452, y + 36, 624, &self.bio, 14, INK);
        p.right(888, y + 108, 200, &format!("{}/160", self.bio.len()), 12, FAINT);
        // Footer, pinned to the window's bottom edge.
        let fy = 731;
        p.z += 5;
        p.box_(Rect::new(409, fy, 710, 69), WHITE, 0);
        p.hline(409, fy, 710, LINE);
        p.left(898, fy + 25, 60, "Cancel", 14, Color(55, 65, 81, 255));
        p.box_(Rect::new(976, fy + 17, 112, 36), INDIGO, 6);
        p.region(Rect::new(976, fy + 17, 112, 36), "save", "Save changes");
        p.center(976, fy + 26, 112, "Save changes", 14, WHITE);
        p.z -= 5;
    }

    fn account(&self, p: &mut Painter) {
        p.border(Rect::new(408, 114, 712, 140), WHITE, 12, LINE);
        p.label(440, 138, 400, "Password", 16, INK, true, Align::Left);
        p.left(440, 164, 400, "Last changed 3 months ago.", 14, MUTED);
        p.border(Rect::new(440, 196, 150, 36), WHITE, 6, BORDER);
        p.center(440, 205, 150, "Change password", 14, INK);
        p.border(Rect::new(408, 278, 712, 160), WHITE, 12, Color(254, 202, 202, 255));
        p.label(440, 302, 400, "Delete account", 16, Color(185, 28, 28, 255), true, Align::Left);
        p.paragraph(440, 328, 648, "Permanently remove your account and all of its content. This cannot be undone.", 14, MUTED);
        p.box_(Rect::new(440, 380, 150, 36), RED, 6);
        p.center(440, 389, 150, "Delete my account", 14, WHITE);
    }

    fn notifications(&self, p: &mut Painter) {
        p.border(Rect::new(408, 114, 712, 380), WHITE, 12, LINE);
        p.label(440, 138, 400, "Email notifications", 16, INK, true, Align::Left);
        p.left(440, 164, 640, "Choose what we email you about. You can unsubscribe at any time.", 14, MUTED);
        let rows = [
            ("Comments", "When someone comments on a file you own."),
            ("Mentions", "When someone @mentions you anywhere."),
            ("Weekly digest", "A summary of activity across your projects, every Monday."),
            ("Product updates", "Occasional news about new features."),
        ];
        for (i, (title, body)) in rows.iter().enumerate() {
            let y = 206 + i as i32 * 72;
            p.hline(409, y, 710, Color(243, 244, 246, 255));
            p.label(440, y + 16, 500, title, 14, INK, true, Align::Left);
            p.left(440, y + 38, 560, body, 14, MUTED);
            let on = self.toggles[i];
            let t = Rect::new(1044, y + 24, 44, 24);
            p.box_(t, if on { INDIGO } else { LINE }, 12);
            p.region(t, &format!("toggle:{i}"), title);
            p.circle(if on { 1076 } else { 1056 }, y + 36, 10, WHITE);
        }
    }
}
