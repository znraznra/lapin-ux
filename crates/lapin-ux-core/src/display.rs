// SPDX-License-Identifier: GPL-3.0-only
//! Displays, as `cosmic-randr list --kdl` reports them, and the commands
//! behind Win+P ("Project") and screen rotation.

/// One connected display.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Head {
    pub name: String,
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    /// Current (or preferred) mode, before rotation.
    pub width: i32,
    pub height: i32,
    pub scale: f32,
    pub mirroring: Option<String>,
    pub transform: String,
}

impl Head {
    /// A laptop's own panel.
    pub fn internal(&self) -> bool {
        ["eDP", "LVDS", "DSI"].iter().any(|p| self.name.starts_with(p))
    }

    /// Width in the layout (logical pixels, after rotation and scale).
    pub fn layout_width(&self) -> i32 {
        let w = if self.transform.contains("90") || self.transform.contains("270") { self.height } else { self.width };
        (w as f32 / self.scale.max(0.1)).round() as i32
    }
}

fn quoted(s: &str) -> Option<String> {
    let a = s.find('"')?;
    let b = s[a + 1..].find('"')?;
    Some(s[a + 1..a + 1 + b].to_string())
}

/// Parse `cosmic-randr list --kdl`.
pub fn parse_kdl(text: &str) -> Vec<Head> {
    let mut heads = Vec::new();
    let mut cur: Option<Head> = None;
    let mut preferred: Option<(i32, i32)> = None;
    let mut have_current = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("output ") {
            if let Some(mut h) = cur.take() {
                if !have_current && let Some((w, hh)) = preferred {
                    h.width = w;
                    h.height = hh;
                }
                heads.push(h);
            }
            preferred = None;
            have_current = false;
            cur = Some(Head {
                name: quoted(t).unwrap_or_default(),
                enabled: t.contains("enabled=#true"),
                scale: 1.0,
                transform: "normal".into(),
                ..Default::default()
            });
            continue;
        }
        let Some(h) = cur.as_mut() else { continue };
        let mut parts = t.split_whitespace();
        match parts.next() {
            Some("position") => {
                h.x = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                h.y = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            Some("scale") => h.scale = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            Some("mirroring") => h.mirroring = quoted(t),
            Some("transform") => h.transform = quoted(t).unwrap_or_else(|| "normal".into()),
            Some("mode") => {
                let w = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let hh = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                if t.contains("current=#true") {
                    h.width = w;
                    h.height = hh;
                    have_current = true;
                } else if t.contains("preferred=#true") && preferred.is_none() {
                    preferred = Some((w, hh));
                }
            }
            _ => {}
        }
    }
    if let Some(mut h) = cur.take() {
        if !have_current && let Some((w, hh)) = preferred {
            h.width = w;
            h.height = hh;
        }
        heads.push(h);
    }
    heads
}

/// The four Win+P choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Project {
    PcOnly,
    Duplicate,
    Extend,
    SecondOnly,
}

impl Project {
    pub const ALL: [Project; 4] = [Project::PcOnly, Project::Duplicate, Project::Extend, Project::SecondOnly];

    pub fn label(self) -> &'static str {
        match self {
            Project::PcOnly => "PC screen only",
            Project::Duplicate => "Duplicate",
            Project::Extend => "Extend",
            Project::SecondOnly => "Second screen only",
        }
    }
}

/// The display treated as "this PC": the laptop panel, else the first one.
pub fn main_head(heads: &[Head]) -> Option<&Head> {
    heads.iter().find(|h| h.internal()).or_else(|| heads.first())
}

/// Which choice the current setup matches (highlighted in the flyout).
pub fn current_project(heads: &[Head]) -> Option<Project> {
    let main = main_head(heads)?;
    let others: Vec<&Head> = heads.iter().filter(|h| h.name != main.name).collect();
    if others.is_empty() {
        return Some(Project::PcOnly);
    }
    if main.enabled && others.iter().all(|h| !h.enabled) {
        return Some(Project::PcOnly);
    }
    if !main.enabled && others.iter().any(|h| h.enabled) {
        return Some(Project::SecondOnly);
    }
    if others.iter().filter(|h| h.enabled).all(|h| h.mirroring.as_deref() == Some(main.name.as_str())) {
        return Some(Project::Duplicate);
    }
    Some(Project::Extend)
}

/// `cosmic-randr` argument lists that switch to `p`, run in order.
pub fn project_commands(heads: &[Head], p: Project) -> Vec<Vec<String>> {
    let Some(main) = main_head(heads) else { return Vec::new() };
    let others: Vec<&Head> = heads.iter().filter(|h| h.name != main.name).collect();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut cmds = Vec::new();
    match p {
        Project::PcOnly => {
            cmds.push(s(&["enable", &main.name]));
            for o in &others {
                cmds.push(s(&["disable", &o.name]));
            }
        }
        Project::SecondOnly => {
            if others.is_empty() {
                return Vec::new();
            }
            for o in &others {
                cmds.push(s(&["enable", &o.name]));
            }
            cmds.push(s(&["disable", &main.name]));
        }
        Project::Duplicate => {
            cmds.push(s(&["enable", &main.name]));
            for o in &others {
                cmds.push(s(&["mirror", &o.name, &main.name]));
            }
        }
        Project::Extend => {
            // Main display at the left, the others in a row to its right.
            cmds.push(s(&["enable", &main.name]));
            cmds.push(s(&["position", &main.name, "0", "0"]));
            let mut x = main.layout_width();
            for o in &others {
                cmds.push(s(&["enable", &o.name]));
                cmds.push(s(&["position", &o.name, &x.to_string(), "0"]));
                x += o.layout_width();
            }
        }
    }
    cmds
}

/// `cosmic-randr` arguments that rotate `head` (for auto-rotate).
pub fn rotate_command(head: &Head, transform: &str) -> Vec<String> {
    vec![
        "mode".into(),
        head.name.clone(),
        head.width.to_string(),
        head.height.to_string(),
        "--transform".into(),
        transform.into(),
    ]
}

/// Accelerometer orientation (iio-sensor-proxy) to an output transform.
pub fn transform_for_orientation(orientation: &str) -> Option<&'static str> {
    match orientation {
        "normal" => Some("normal"),
        "bottom-up" => Some("rotate180"),
        "left-up" => Some("rotate90"),
        "right-up" => Some("rotate270"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = r#"output "eDP-1" enabled=#true {
  description make="BOE" model="0x0BCA"
  physical 309 174
  position 0 0
  scale 1.25
  modes {
    mode 1920 1080 60000 current=#true preferred=#true
    mode 1280 720 60000
  }
}
output "HDMI-A-1" enabled=#true {
  description make="Dell" model="P2419H"
  physical 527 296
  position 0 0
  scale 1.00
  mirroring "eDP-1"
  modes {
    mode 1920 1080 60000 current=#true preferred=#true
  }
}
"#;

    #[test]
    fn parses_heads() {
        let h = parse_kdl(TWO);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].name, "eDP-1");
        assert!(h[0].internal());
        assert_eq!((h[0].width, h[0].height), (1920, 1080));
        assert_eq!(h[0].scale, 1.25);
        assert_eq!(h[1].mirroring.as_deref(), Some("eDP-1"));
        assert_eq!(h[0].layout_width(), 1536);
    }

    #[test]
    fn detects_duplicate_and_plans_extend() {
        let h = parse_kdl(TWO);
        assert_eq!(current_project(&h), Some(Project::Duplicate));
        let c = project_commands(&h, Project::Extend);
        assert!(c.contains(&vec!["position".to_string(), "HDMI-A-1".into(), "1536".into(), "0".into()]));
        let c = project_commands(&h, Project::SecondOnly);
        assert_eq!(c.last().unwrap(), &vec!["disable".to_string(), "eDP-1".into()]);
    }

    #[test]
    fn single_display_is_pc_only() {
        let h = parse_kdl(&TWO[..TWO.find("output \"HDMI").unwrap()]);
        assert_eq!(current_project(&h), Some(Project::PcOnly));
        assert!(project_commands(&h, Project::SecondOnly).is_empty());
    }

    #[test]
    fn orientations() {
        assert_eq!(transform_for_orientation("left-up"), Some("rotate90"));
        assert_eq!(transform_for_orientation("undefined"), None);
    }
}
