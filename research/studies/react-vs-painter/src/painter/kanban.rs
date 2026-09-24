//! Sprint board, hand-positioned with the Painter.
use cw_applications::desktop_scene::shared::Align;
use cw_applications::desktop_scene::Painter;
use cw_scene::{Color, Rect};

const BG: Color = Color(249, 250, 251, 255);
const WHITE: Color = Color(255, 255, 255, 255);
const BORDER: Color = Color(229, 231, 235, 255);
const INK: Color = Color(17, 24, 39, 255);
const MUTED: Color = Color(107, 114, 128, 255);
const FAINT: Color = Color(156, 163, 175, 255);
const INDIGO: Color = Color(79, 70, 229, 255);
const COLUMN: Color = Color(243, 244, 246, 255);

#[derive(Clone)]
pub struct Card {
    pub id: u32,
    pub title: String,
    pub column: usize,
    pub priority: usize,
    pub labels: Vec<&'static str>,
    pub people: Vec<usize>,
    pub due: Option<&'static str>,
    pub comments: u32,
    pub files: u32,
    pub progress: Option<u32>,
}

const COLUMNS: [(&str, Color); 4] = [
    ("To do", Color(156, 163, 175, 255)),
    ("In progress", Color(14, 165, 233, 255)),
    ("In review", Color(245, 158, 11, 255)),
    ("Done", Color(16, 185, 129, 255)),
];
const PRIORITY: [(&str, Color); 3] = [
    ("Low", Color(107, 114, 128, 255)),
    ("Medium", Color(245, 158, 11, 255)),
    ("High", Color(244, 63, 94, 255)),
];
const PEOPLE: [(&str, Color); 4] = [
    ("AR", Color(236, 72, 153, 255)),
    ("JK", Color(59, 130, 246, 255)),
    ("ML", Color(16, 185, 129, 255)),
    ("DO", Color(168, 85, 247, 255)),
];

fn label_colors(l: &str) -> (Color, Color) {
    match l {
        "Design" => (Color(253, 242, 248, 255), Color(190, 24, 93, 255)),
        "Frontend" => (Color(238, 242, 255, 255), Color(67, 56, 202, 255)),
        "Backend" => (Color(236, 253, 245, 255), Color(4, 120, 87, 255)),
        "Research" => (Color(255, 251, 235, 255), Color(180, 83, 9, 255)),
        _ => (Color(255, 241, 242, 255), Color(190, 18, 60, 255)),
    }
}

pub struct Board {
    pub cards: Vec<Card>,
    pub modal: bool,
    pub draft: String,
    pub priority: usize,
    pub touched: bool,
}

impl Default for Board {
    fn default() -> Self {
        let c = |id, title: &str, column, priority, labels: Vec<&'static str>, people: Vec<usize>, due, comments, files, progress| Card {
            id,
            title: title.into(),
            column,
            priority,
            labels,
            people,
            due,
            comments,
            files,
            progress,
        };
        Self {
            cards: vec![
                c(1, "Audit the pricing page copy", 0, 0, vec!["Research"], vec![2], Some("May 3"), 2, 0, None),
                c(2, "Hero illustration for the relaunch", 0, 1, vec!["Design"], vec![0, 3], Some("May 6"), 5, 3, None),
                c(3, "Checkout crashes on Safari 16 when the coupon field is empty", 0, 2, vec!["Bug", "Frontend"], vec![1], None, 8, 1, None),
                c(4, "Migrate the blog to the new CMS", 1, 1, vec!["Backend"], vec![3], Some("May 9"), 1, 0, Some(60)),
                c(5, "Responsive navigation", 1, 2, vec!["Frontend", "Design"], vec![1, 0], None, 4, 2, Some(35)),
                c(6, "Rate-limit the signup endpoint", 2, 2, vec!["Backend"], vec![2, 1], Some("Apr 30"), 3, 0, None),
                c(7, "Customer interview synthesis", 3, 0, vec!["Research"], vec![0], None, 0, 4, None),
            ],
            modal: false,
            draft: String::new(),
            priority: 1,
            touched: false,
        }
    }
}

impl Board {
    pub fn click(&mut self, target: &str) {
        match target {
            "new-task" => self.modal = true,
            "close" => self.modal = false,
            "create" => {
                self.touched = true;
                if self.draft.trim().len() >= 3 {
                    let id = self.cards.iter().map(|c| c.id).max().unwrap_or(0) + 1;
                    self.cards.push(Card {
                        id,
                        title: self.draft.trim().into(),
                        column: 0,
                        priority: self.priority,
                        labels: vec!["Research"],
                        people: vec![0],
                        due: None,
                        comments: 0,
                        files: 0,
                        progress: None,
                    });
                    self.modal = false;
                    self.draft.clear();
                }
            }
            t if t.starts_with("priority:") => self.priority = t[9..].parse().unwrap_or(1),
            t if t.starts_with("move:") => {
                let id: u32 = t[5..].parse().unwrap_or(0);
                if let Some(c) = self.cards.iter_mut().find(|c| c.id == id) {
                    c.column = (c.column + 1).min(3);
                }
            }
            _ => {}
        }
    }

    pub fn render(&self, p: &mut Painter) {
        p.scene.background = BG;
        // Header
        p.box_(Rect::new(0, 0, 1280, 120), WHITE, 0);
        p.hline(0, 120, 1280, BORDER);
        p.left(24, 16, 300, "Projects  /  Website relaunch", 12, MUTED);
        p.label(24, 36, 300, "Sprint 14", 20, INK, true, Align::Left);
        for (i, (initials, c)) in PEOPLE.iter().enumerate() {
            let cx = 1062 + i as i32 * 24;
            p.circle(cx, 32, 17, WHITE);
            p.circle(cx, 32, 15, *c);
            p.label(cx - 15, 26, 30, initials, 10, WHITE, true, Align::Center);
        }
        p.box_(Rect::new(1146, 16, 114, 36), INDIGO, 8);
        p.region(Rect::new(1146, 16, 114, 36), "new-task", "New task");
        p.symbol("plus", 1160, 26, 16, WHITE);
        p.left(1182, 25, 80, "New task", 14, WHITE);
        p.border(Rect::new(24, 70, 224, 32), WHITE, 6, BORDER);
        p.symbol("search", 34, 78, 16, FAINT);
        p.left(58, 77, 180, "Filter cards", 14, FAINT);
        p.border(Rect::new(260, 70, 86, 32), WHITE, 6, BORDER);
        p.symbol("filters", 270, 78, 16, MUTED);
        p.left(292, 77, 60, "Filters", 14, MUTED);
        let done = self.cards.iter().filter(|c| c.column == 3).count();
        p.right(1060, 77, 196, &format!("{} of {} done", done, self.cards.len()), 14, MUTED);

        for (ci, (title, dot)) in COLUMNS.iter().enumerate() {
            let x = 24 + ci as i32 * 304;
            p.box_(Rect::new(x, 144, 288, 632), COLUMN, 12);
            p.circle(x + 16, 168, 4, *dot);
            p.label(x + 28, 160, 150, title, 14, Color(55, 65, 81, 255), true, Align::Left);
            let n = self.cards.iter().filter(|c| c.column == ci).count();
            let tw = p.measure(title, 14, true) as i32;
            p.box_(Rect::new(x + 36 + tw, 160, 24, 18), WHITE, 9);
            p.center(x + 36 + tw, 161, 24, &n.to_string(), 12, MUTED);
            p.symbol("more", x + 256, 160, 16, FAINT);
            let mut y = 196;
            for card in self.cards.iter().filter(|c| c.column == ci) {
                y += self.card(p, card, x + 8, y) + 8;
            }
            if n == 0 {
                p.border(Rect::new(x + 8, 196, 272, 60), COLUMN, 8, BORDER);
                p.center(x + 8, 216, 272, "Drop cards here", 12, FAINT);
            }
        }

        if self.modal {
            self.dialog(p);
        }
    }

    /// Draws a card at (x, y) and returns its height.
    fn card(&self, p: &mut Painter, c: &Card, x: i32, y: i32) -> i32 {
        let w = 272u32;
        // Title may wrap to two lines.
        let lines = if p.measure(&c.title, 14, true) > w - 24 { 2 } else { 1 };
        let mut h = 16 + 20 + 8 + lines * 20 + 12 + 24 + 12;
        if c.progress.is_some() {
            h += 30;
        }
        let r = Rect::new(x, y, w, h as u32);
        p.drop_shadow(r, 8, 2, 10, 1);
        p.border(r, WHITE, 8, BORDER);
        let mut lx = x + 12;
        for l in &c.labels {
            let (bg, fg) = label_colors(l);
            let lw = p.measure(l, 11, false) + 12;
            p.box_(Rect::new(lx, y + 12, lw, 18), bg, 4);
            p.left(lx + 6, y + 14, lw, l, 11, fg);
            lx += lw as i32 + 4;
        }
        p.symbol("flag", x + 246, y + 12, 14, PRIORITY[c.priority].1);
        let ty = y + 40;
        if lines == 1 {
            p.label(x + 12, ty, w - 24, &c.title, 14, INK, true, Align::Left);
        } else {
            p.paragraph(x + 12, ty, w - 24, &c.title, 14, INK);
        }
        let mut by = ty + lines * 20 + 12;
        if let Some(pct) = c.progress {
            p.left(x + 12, by, 100, "Progress", 11, MUTED);
            p.right(x + 160, by, 100, &format!("{pct}%"), 11, MUTED);
            p.box_(Rect::new(x + 12, by + 18, 248, 6), COLUMN, 3);
            p.box_(Rect::new(x + 12, by + 18, 248 * pct / 100, 6), Color(14, 165, 233, 255), 3);
            by += 30;
        }
        for (i, who) in c.people.iter().enumerate() {
            let cx = x + 24 + i as i32 * 18;
            p.circle(cx, by + 12, 13, WHITE);
            p.circle(cx, by + 12, 11, PEOPLE[*who].1);
            p.label(cx - 11, by + 7, 22, PEOPLE[*who].0, 9, WHITE, true, Align::Center);
        }
        // Meta, right to left.
        let mut mx = x + w as i32 - 12;
        if c.column < 3 {
            mx -= 14;
            p.symbol("arrow-right", mx, by + 5, 14, FAINT);
            p.region(Rect::new(mx - 2, by + 3, 18, 18), &format!("move:{}", c.id), "Move");
            mx -= 12;
        }
        for (icon, value) in [("paperclip", c.files), ("comment", c.comments)] {
            if value > 0 {
                let t = value.to_string();
                let tw = p.measure(&t, 12, false) as i32;
                mx -= tw;
                p.left(mx, by + 5, tw as u32 + 2, &t, 12, FAINT);
                mx -= 18;
                p.symbol(icon, mx, by + 5, 14, FAINT);
                mx -= 12;
            }
        }
        if let Some(due) = c.due {
            let tw = p.measure(due, 12, false) as i32;
            mx -= tw;
            p.left(mx, by + 5, tw as u32 + 2, due, 12, FAINT);
            mx -= 18;
            p.symbol("calendar", mx, by + 5, 14, FAINT);
        }
        h
    }

    fn dialog(&self, p: &mut Painter) {
        p.z += 10;
        p.box_(Rect::new(0, 0, 1280, 800), Color(17, 24, 39, 102), 0);
        p.region(Rect::new(0, 0, 1280, 800), "close", "Close");
        let error = self.touched && self.draft.trim().len() < 3;
        let h = if error { 335 } else { 313 };
        let d = Rect::new(416, (800 - h) / 2, 448, h as u32);
        p.drop_shadow(d, 16, 24, 60, 12);
        p.box_(d, WHITE, 16);
        p.label(440, d.y + 20, 300, "New task", 16, INK, true, Align::Left);
        p.symbol("close", 816, d.y + 20, 20, FAINT);
        p.region(Rect::new(812, d.y + 16, 28, 28), "close", "Close");
        p.hline(416, d.y + 60, 448, Color(243, 244, 246, 255));
        let fy = d.y + 80;
        p.label(440, fy, 200, "Title", 14, Color(55, 65, 81, 255), true, Align::Left);
        p.border(
            Rect::new(440, fy + 26, 400, 38),
            WHITE,
            8,
            if error { Color(253, 164, 175, 255) } else { Color(209, 213, 219, 255) },
        );
        if self.draft.is_empty() {
            p.left(452, fy + 36, 380, "e.g. Draft the onboarding email", 14, FAINT);
        } else {
            p.left(452, fy + 36, 380, &self.draft, 14, INK);
        }
        let mut py = fy + 80;
        if error {
            p.left(440, fy + 70, 400, "Give the task a title of at least 3 characters.", 12, Color(225, 29, 72, 255));
            py += 22;
        }
        p.label(440, py, 200, "Priority", 14, Color(55, 65, 81, 255), true, Align::Left);
        for (i, (name, c)) in PRIORITY.iter().enumerate() {
            let r = Rect::new(440 + i as i32 * 136, py + 26, 128, 38);
            let on = i == self.priority;
            p.border(
                r,
                if on { Color(238, 242, 255, 255) } else { WHITE },
                8,
                if on { Color(99, 102, 241, 255) } else { BORDER },
            );
            p.region(r, &format!("priority:{i}"), name);
            let tw = p.measure(name, 14, false) as i32 + 20;
            let sx = r.x + (128 - tw) / 2;
            p.symbol("flag", sx, r.y + 12, 14, *c);
            p.left(sx + 20, r.y + 10, 80, name, 14, if on { Color(67, 56, 202, 255) } else { Color(75, 85, 99, 255) });
        }
        let fy = d.y + h - 68;
        p.box_(Rect::new(416, fy, 448, 68), BG, 16);
        p.box_(Rect::new(416, fy, 448, 16), BG, 0);
        p.left(658, fy + 24, 60, "Cancel", 14, Color(55, 65, 81, 255));
        p.box_(Rect::new(732, fy + 16, 108, 36), INDIGO, 8);
        p.region(Rect::new(732, fy + 16, 108, 36), "create", "Create task");
        p.center(732, fy + 25, 108, "Create task", 14, WHITE);
        p.z -= 10;
    }
}
