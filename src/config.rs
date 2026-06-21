//! snertwl config: a tiny, dependency-free parser for `snertwl.conf`.
//!
//! Format is line-based `key = value`, `#` starts a comment. Scalars set the
//! modifier, gap and background; `bind = MODS, KEY, ACTION[, ARG]` lines define
//! keybindings (Hyprland-ish syntax). Anything we can't parse is warned about
//! and skipped, so a typo never stops the compositor from starting.
//!
//! If no config file exists we fall back to built-in defaults that reproduce the
//! old hardcoded behavior, so snertwl is usable out of the box.

use std::env;
use std::ffi::CString;
use std::fs;
use std::os::raw::c_char;
use std::path::PathBuf;

/// Modifier bits (mirror the WLR_MODIFIER_* enum).
pub const MOD_SHIFT: u32 = 1 << 0; // Shift
pub const MOD_CTRL: u32 = 1 << 2; // Control
pub const MOD_ALT: u32 = 1 << 3; // Alt
pub const MOD_LOGO: u32 = 1 << 6; // Super / Logo

/// The modifier bits we consider when matching binds. Excludes Caps Lock (1<<1)
/// and Num Lock (Mod2, 1<<4) so they never break a binding.
pub const MOD_MASK: u32 = MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_LOGO;

extern "C" {
    fn snertwl_keysym_from_name(name: *const c_char) -> u32;
}

/// What a keybinding does when triggered.
#[derive(Clone)]
pub enum Action {
    Spawn(String),
    Close,
    Quit,
    FocusNext,
    FocusPrev,
    /// Switch to workspace (0-based index).
    Workspace(usize),
    /// Move the focused window to a workspace (0-based index).
    MoveToWorkspace(usize),
}

/// One key combination mapped to an action.
#[derive(Clone)]
pub struct Bind {
    pub mods: u32,
    pub keysym: u32,
    pub action: Action,
}

/// Parsed compositor configuration.
pub struct Config {
    /// The primary modifier (`$mod` / `MOD` in binds); Super by default.
    pub modifier: u32,
    /// Gap between/around tiled windows, in pixels.
    pub gap: i32,
    /// Background color of empty workspace area (r, g, b in 0..1).
    pub background: (f32, f32, f32),
    pub binds: Vec<Bind>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            modifier: MOD_LOGO,
            gap: 10,
            background: (0.0, 0.6, 0.6),
            binds: Vec::new(),
        }
    }
}

impl Config {
    /// Load config from `$XDG_CONFIG_HOME/snertwl/snertwl.conf` (or
    /// `~/.config/snertwl/snertwl.conf`). Missing file -> built-in defaults.
    /// `SNERTWL_MOD=alt` overrides the modifier (for nested dev under Hyprland,
    /// which grabs Super-chords before us).
    pub fn load() -> Config {
        let mut cfg = Config::default();

        let contents = config_path().and_then(|p| fs::read_to_string(&p).ok());
        match &contents {
            Some(text) => {
                println!("snertwl: loaded config");
                cfg.parse_scalars(text);
            }
            None => println!("snertwl: no config file — using defaults"),
        }

        // Env override wins over the config's modifier line.
        if let Ok("alt") = env::var("SNERTWL_MOD").as_deref() {
            cfg.modifier = MOD_ALT;
        }

        // Binds are parsed after the modifier is final, so `MOD` resolves right.
        match &contents {
            Some(text) => cfg.parse_binds(text),
            None => cfg.binds = default_binds(cfg.modifier),
        }
        if cfg.binds.is_empty() {
            cfg.binds = default_binds(cfg.modifier);
        }

        println!(
            "snertwl: modifier = {}, gap = {}, {} bind(s)",
            mod_name(cfg.modifier),
            cfg.gap,
            cfg.binds.len()
        );
        cfg
    }

    /// First pass: scalar settings (everything except `bind`).
    fn parse_scalars(&mut self, text: &str) {
        for (n, raw) in lines(text) {
            let Some((key, val)) = split_kv(raw) else { continue };
            match key {
                "modifier" => match parse_mods(val, MOD_LOGO) {
                    Some(m) => self.modifier = m,
                    None => warn(n, "unknown modifier", raw),
                },
                "gap" => match val.parse::<i32>() {
                    Ok(g) if g >= 0 => self.gap = g,
                    _ => warn(n, "invalid gap", raw),
                },
                "background" => match parse_color(val) {
                    Some(c) => self.background = c,
                    None => warn(n, "invalid background (want `r g b`)", raw),
                },
                "bind" => {} // handled in parse_binds
                _ => warn(n, "unknown setting", raw),
            }
        }
    }

    /// Second pass: `bind = MODS, KEY, ACTION[, ARG]`.
    fn parse_binds(&mut self, text: &str) {
        for (n, raw) in lines(text) {
            let Some((key, val)) = split_kv(raw) else { continue };
            if key != "bind" {
                continue;
            }
            match self.parse_bind(val) {
                Some(b) => self.binds.push(b),
                None => warn(n, "invalid bind", raw),
            }
        }
    }

    fn parse_bind(&self, val: &str) -> Option<Bind> {
        // mods, key, action, [arg (may contain commas, e.g. a spawn command)]
        let mut parts = val.splitn(4, ',');
        let mods = parse_mods(parts.next()?.trim(), self.modifier)?;
        let keysym = keysym_from_name(parts.next()?.trim())?;
        let action_name = parts.next()?.trim();
        let arg = parts.next().map(|s| s.trim());
        let action = parse_action(action_name, arg)?;
        Some(Bind { mods, keysym, action })
    }
}

/// The default binds, replicating snertwl's original hardcoded behavior.
fn default_binds(modifier: u32) -> Vec<Bind> {
    let m = modifier;
    let ms = modifier | MOD_SHIFT;
    let mut binds = vec![
        Bind { mods: m, keysym: key("Return"), action: Action::Spawn("kitty".into()) },
        Bind { mods: m, keysym: key("Q"), action: Action::Close },
        Bind { mods: ms, keysym: key("Q"), action: Action::Quit },
        Bind { mods: m, keysym: key("J"), action: Action::FocusNext },
        Bind { mods: m, keysym: key("K"), action: Action::FocusPrev },
    ];
    for i in 0..9u32 {
        let name = (b'1' + i as u8) as char;
        let name = name.to_string();
        binds.push(Bind { mods: m, keysym: key(&name), action: Action::Workspace(i as usize) });
        binds.push(Bind { mods: ms, keysym: key(&name), action: Action::MoveToWorkspace(i as usize) });
    }
    binds
}

// --- parsing helpers -------------------------------------------------------

/// Iterate (1-based line number, trimmed non-empty/non-comment line).
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines().enumerate().filter_map(|(i, l)| {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            None
        } else {
            Some((i + 1, l))
        }
    })
}

/// Split a `key = value` line; returns lowercased key and trimmed value.
fn split_kv(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    Some((k.trim(), v.trim()))
}

/// Parse a modifier spec like `SUPER SHIFT`, `super+shift`, `MOD`, `$mod`.
/// `MOD`/`$mod`/`mainmod` expand to `primary`.
fn parse_mods(spec: &str, primary: u32) -> Option<u32> {
    let mut bits = 0;
    for tok in spec.split(['+', ' ', '\t']).filter(|t| !t.is_empty()) {
        bits |= match tok.to_ascii_uppercase().trim_start_matches('$') {
            "MOD" | "MAINMOD" => primary,
            "SUPER" | "LOGO" | "WIN" => MOD_LOGO,
            "ALT" | "MOD1" => MOD_ALT,
            "SHIFT" => MOD_SHIFT,
            "CTRL" | "CONTROL" => MOD_CTRL,
            _ => return None,
        };
    }
    Some(bits)
}

fn parse_color(spec: &str) -> Option<(f32, f32, f32)> {
    let mut it = spec.split_whitespace();
    let r = it.next()?.parse().ok()?;
    let g = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((r, g, b))
}

fn parse_action(name: &str, arg: Option<&str>) -> Option<Action> {
    match name.to_ascii_lowercase().as_str() {
        "spawn" | "exec" => Some(Action::Spawn(arg?.to_string())),
        "close" | "killactive" => Some(Action::Close),
        "quit" | "exit" => Some(Action::Quit),
        "focusnext" => Some(Action::FocusNext),
        "focusprev" => Some(Action::FocusPrev),
        "workspace" => Some(Action::Workspace(workspace_index(arg?)?)),
        "movetoworkspace" => Some(Action::MoveToWorkspace(workspace_index(arg?)?)),
        _ => None,
    }
}

/// Parse a 1-based workspace number (`1`..`9`) to a 0-based index.
fn workspace_index(arg: &str) -> Option<usize> {
    let n: usize = arg.trim().parse().ok()?;
    (1..=9).contains(&n).then(|| n - 1)
}

/// Resolve a key name to a keysym, or None if xkb doesn't know it.
fn keysym_from_name(name: &str) -> Option<u32> {
    let c = CString::new(name).ok()?;
    let sym = unsafe { snertwl_keysym_from_name(c.as_ptr()) };
    (sym != 0).then_some(sym)
}

/// Like `keysym_from_name` but for trusted built-in defaults (must resolve).
fn key(name: &str) -> u32 {
    keysym_from_name(name).expect("built-in default key name should resolve")
}

fn mod_name(m: u32) -> &'static str {
    match m {
        MOD_ALT => "Alt",
        MOD_LOGO => "Super",
        _ => "custom",
    }
}

fn warn(line: usize, msg: &str, raw: &str) {
    eprintln!("snertwl: config line {line}: {msg}: `{raw}`");
}

fn config_path() -> Option<PathBuf> {
    if let Ok(dir) = env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("snertwl/snertwl.conf"));
        }
    }
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/snertwl/snertwl.conf"))
}
