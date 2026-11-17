//! Library catalog. Layout: `<root>/<format>/<item folder>/`.
//! Each item folder holds its own `info.json` (name, quantity) and an optional cover image.
//! The folder name itself doesn't matter; the name shown in the app comes from `info.json`.

use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const INFO_FILE: &str = "info.json";
const COVER_FILES: [&str; 3] = ["cover.png", "cover.jpg", "cover.jpeg"];
const MAX_NAME_LEN: usize = 200;
const MAX_DATE_LEN: usize = 40;

// Scans and saves never run at the same time.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// The format folder, e.g. "CD&DVD".
    pub format: String,
    /// The item's own folder (its name is not shown).
    pub folder: String,
    /// Display name, from info.json (falls back to the folder name).
    pub name: String,
    pub quantity: u32,
    /// Free text such as "23.12.2012" (or just "2012"); empty when not set.
    pub date: String,
    /// Absolute path of the cover image, if the folder has one.
    pub cover: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Library {
    pub items: Vec<Item>,
    pub warning: Option<String>,
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || name.eq_ignore_ascii_case("System Volume Information")
}

/// Sub-folders of `dir`, sorted by name, without hidden/system ones.
fn subdirs(dir: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_dir(&name) {
            continue;
        }
        out.push((name, path));
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    Ok(out)
}

fn find_cover(folder: &Path) -> Option<String> {
    COVER_FILES
        .iter()
        .map(|f| folder.join(f))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

/// info.json as a JSON object. `Ok(None)` if the file doesn't exist yet.
/// Other keys in the file are kept untouched when we save.
fn read_info(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("couldn't read {INFO_FILE}: {e}"))?;
    let text = text.trim_start_matches('\u{feff}');
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(format!("{INFO_FILE} isn't a JSON object")),
        Err(e) => Err(format!("{INFO_FILE} is damaged ({e})")),
    }
}

fn write_info(path: &Path, info: &Map<String, Value>) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(info).map_err(|e| e.to_string())?;
    text.push('\n');
    let tmp = path.with_extension("json.tmp");
    let result = fs::write(&tmp, text).and_then(|_| fs::rename(&tmp, path));
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(format!("Couldn't save {INFO_FILE} ({e})."));
    }
    Ok(())
}

fn name_from(info: &Map<String, Value>, folder: &str) -> String {
    match info.get("name").and_then(Value::as_str).map(str::trim) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => folder.to_string(),
    }
}

fn date_from(info: &Map<String, Value>) -> String {
    info.get("date").and_then(Value::as_str).map(|d| d.trim().to_string()).unwrap_or_default()
}

fn quantity_from(info: &Map<String, Value>) -> u32 {
    info.get("quantity")
        .and_then(Value::as_u64)
        .map(|q| q.min(u32::MAX as u64) as u32)
        .unwrap_or(1)
}

pub fn scan(root: &Path) -> Result<Library, String> {
    let _guard = lock();
    if !root.is_dir() {
        return Err("That folder doesn't exist.".to_string());
    }
    let mut warnings: Vec<String> = Vec::new();
    let mut items: Vec<Item> = Vec::new();

    let formats = subdirs(root).map_err(|e| format!("Couldn't read the folder: {e}"))?;
    for (format, format_dir) in formats {
        let folders = match subdirs(&format_dir) {
            Ok(f) => f,
            Err(e) => {
                warnings.push(format!("Couldn't read {format}: {e}"));
                continue;
            }
        };
        for (folder, dir) in folders {
            // Scanning never writes anything; info.json is created on the first edit.
            let (name, quantity, date) = match read_info(&dir.join(INFO_FILE)) {
                Ok(Some(info)) => (name_from(&info, &folder), quantity_from(&info), date_from(&info)),
                Ok(None) => (folder.clone(), 1, String::new()),
                Err(e) => {
                    warnings.push(format!("{format}/{folder}: {e}"));
                    (folder.clone(), 1, String::new())
                }
            };
            items.push(Item { format: format.clone(), folder, name, quantity, date, cover: find_cover(&dir) });
        }
    }

    items.sort_by(|a, b| {
        (a.format.to_lowercase(), a.name.to_lowercase())
            .cmp(&(b.format.to_lowercase(), b.name.to_lowercase()))
    });

    Ok(Library {
        items,
        warning: if warnings.is_empty() { None } else { Some(warnings.join("\n")) },
    })
}

/// A plain single folder name: no separators, no "..", so edits can't escape the library.
fn plain(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.contains(['/', '\\', ':'])
        && !part.chars().any(|c| c.is_control())
}

fn item_dir(root: &Path, format: &str, folder: &str) -> Result<PathBuf, String> {
    if !plain(format) || !plain(folder) {
        return Err("Invalid item.".to_string());
    }
    let dir = root.join(format).join(folder);
    if !dir.is_dir() {
        return Err("That item's folder no longer exists.".to_string());
    }
    Ok(dir)
}

/// Read-modify-write one item's info.json. Refuses to touch a damaged file.
fn update(root: &Path, format: &str, folder: &str, change: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let _guard = lock();
    let dir = item_dir(root, format, folder)?;
    let path = dir.join(INFO_FILE);
    let mut info = match read_info(&path) {
        Ok(Some(info)) => info,
        Ok(None) => {
            let mut m = Map::new();
            m.insert("name".into(), Value::String(folder.to_string()));
            m.insert("quantity".into(), Value::from(1u32));
            m
        }
        Err(e) => return Err(format!("Not saved: {e}. Fix or delete it in {format}/{folder}.")),
    };
    change(&mut info);
    write_info(&path, &info)
}

pub fn set_quantity(root: &Path, format: &str, folder: &str, quantity: u32) -> Result<(), String> {
    update(root, format, folder, |info| {
        info.insert("quantity".into(), Value::from(quantity));
    })
}

pub fn set_name(root: &Path, format: &str, folder: &str, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A name can't be empty.".to_string());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(format!("A name can be at most {MAX_NAME_LEN} characters."));
    }
    update(root, format, folder, |info| {
        info.insert("name".into(), Value::String(name.to_string()));
    })
}

pub fn set_date(root: &Path, format: &str, folder: &str, date: &str) -> Result<(), String> {
    let date = date.trim();
    if date.chars().count() > MAX_DATE_LEN {
        return Err(format!("A date can be at most {MAX_DATE_LEN} characters."));
    }
    if date.chars().any(|c| c.is_control()) {
        return Err("That date has invalid characters.".to_string());
    }
    update(root, format, folder, |info| {
        if date.is_empty() {
            info.remove("date");
        } else {
            info.insert("date".into(), Value::String(date.to_string()));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Tmp {
            let p = std::env::temp_dir().join(format!(
                "catalog-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn mk(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(&p).unwrap();
            p
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(lib: &Library) -> Vec<String> {
        lib.items.iter().map(|i| format!("{}:{}:{}", i.format, i.name, i.quantity)).collect()
    }

    #[test]
    fn scan_uses_folder_name_by_default_and_writes_nothing() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/48213907");
        t.mk("CD&DVD/10000001");
        fs::write(a.join("cover.png"), b"x").unwrap();

        let lib = scan(&t.0).unwrap();
        assert_eq!(names(&lib), ["CD&DVD:10000001:1", "CD&DVD:48213907:1"]);
        assert!(lib.warning.is_none());
        assert!(lib.items[1].cover.is_some() && lib.items[0].cover.is_none());
        assert!(!a.join("info.json").exists(), "scanning must not write files");
    }

    #[test]
    fn name_and_quantity_are_saved_in_the_items_own_folder() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/48213907");
        set_name(&t.0, "CD&DVD", "48213907", "  Пасхальный хор  ").unwrap();
        set_quantity(&t.0, "CD&DVD", "48213907", 5).unwrap();

        assert!(a.join("info.json").is_file());
        let lib = scan(&t.0).unwrap();
        assert_eq!(names(&lib), ["CD&DVD:Пасхальный хор:5"]);
        assert_eq!(lib.items[0].folder, "48213907");
    }

    #[test]
    fn date_is_saved_trimmed_cleared_and_validated() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/x1");
        assert_eq!(scan(&t.0).unwrap().items[0].date, "");

        set_date(&t.0, "CD&DVD", "x1", "  23.12.2012 ").unwrap();
        assert_eq!(scan(&t.0).unwrap().items[0].date, "23.12.2012");
        set_date(&t.0, "CD&DVD", "x1", "2012").unwrap();
        assert_eq!(scan(&t.0).unwrap().items[0].date, "2012");

        // other fields untouched, and an empty date removes the key
        set_quantity(&t.0, "CD&DVD", "x1", 3).unwrap();
        set_date(&t.0, "CD&DVD", "x1", "   ").unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(a.join("info.json")).unwrap()).unwrap();
        assert!(v.get("date").is_none());
        assert_eq!(v["quantity"], 3);
        assert_eq!(scan(&t.0).unwrap().items[0].date, "");

        assert!(set_date(&t.0, "CD&DVD", "x1", &"9".repeat(41)).is_err());
        assert!(set_date(&t.0, "CD&DVD", "x1", "1\n2").is_err());
        assert!(set_date(&t.0, "..", "x1", "2012").is_err());
    }

    #[test]
    fn quantity_first_keeps_folder_name_as_name() {
        let t = Tmp::new();
        let a = t.mk("Books/Some book");
        set_quantity(&t.0, "Books", "Some book", 3).unwrap();
        let text = fs::read_to_string(a.join("info.json")).unwrap();
        assert!(text.contains("\"name\": \"Some book\"") && text.contains("\"quantity\": 3"));
    }

    #[test]
    fn renaming_the_folder_keeps_its_data() {
        let t = Tmp::new();
        t.mk("CD&DVD/aaaa1111");
        set_name(&t.0, "CD&DVD", "aaaa1111", "My disc").unwrap();
        set_quantity(&t.0, "CD&DVD", "aaaa1111", 2).unwrap();
        fs::rename(t.0.join("CD&DVD/aaaa1111"), t.0.join("CD&DVD/bbbb2222")).unwrap();
        assert_eq!(names(&scan(&t.0).unwrap()), ["CD&DVD:My disc:2"]);
    }

    #[test]
    fn other_keys_in_info_json_survive_edits() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/x1");
        fs::write(a.join("info.json"), r#"{"name":"A","quantity":2,"tags":["choir","1998"]}"#).unwrap();
        set_name(&t.0, "CD&DVD", "x1", "B").unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(a.join("info.json")).unwrap()).unwrap();
        assert_eq!(v["name"], "B");
        assert_eq!(v["quantity"], 2);
        assert_eq!(v["tags"][1], "1998");
    }

    #[test]
    fn hand_edited_info_json_is_understood() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/x1");
        let b = t.mk("CD&DVD/x2");
        let c = t.mk("CD&DVD/x3");
        fs::write(a.join("info.json"), "\u{feff}{\"name\": \"With BOM\", \"quantity\": 4, \"date\": \" 1998 \"}").unwrap();
        fs::write(b.join("info.json"), r#"{"name": "No quantity"}"#).unwrap();
        fs::write(c.join("info.json"), r#"{"name": "   ", "quantity": -3}"#).unwrap();
        let lib = scan(&t.0).unwrap();
        assert_eq!(names(&lib), ["CD&DVD:No quantity:1", "CD&DVD:With BOM:4", "CD&DVD:x3:1"]);
        assert_eq!(lib.items[1].date, "1998");
    }

    #[test]
    fn damaged_info_json_is_listed_with_a_warning_and_never_overwritten() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/x1");
        fs::write(a.join("info.json"), "{ not json").unwrap();
        let lib = scan(&t.0).unwrap();
        assert_eq!(names(&lib), ["CD&DVD:x1:1"]);
        assert!(lib.warning.unwrap().contains("damaged"));
        assert!(set_name(&t.0, "CD&DVD", "x1", "New").is_err());
        assert!(set_quantity(&t.0, "CD&DVD", "x1", 9).is_err());
        assert_eq!(fs::read_to_string(a.join("info.json")).unwrap(), "{ not json");
    }

    #[test]
    fn edits_cannot_escape_the_library() {
        let t = Tmp::new();
        t.mk("CD&DVD/x1");
        for (f, d) in [("..", "x1"), ("CD&DVD", ".."), ("CD&DVD/x1", ""), ("CD&DVD", "..\\x1"), ("", "x1"), ("C:", "x1")] {
            assert!(set_name(&t.0, f, d, "hack").is_err(), "{f:?} {d:?}");
        }
        assert!(set_name(&t.0, "CD&DVD", "gone", "x").is_err());
        assert!(!t.0.join("info.json").exists());
    }

    #[test]
    fn bad_names_are_refused() {
        let t = Tmp::new();
        t.mk("CD&DVD/x1");
        assert!(set_name(&t.0, "CD&DVD", "x1", "   ").is_err());
        assert!(set_name(&t.0, "CD&DVD", "x1", &"x".repeat(201)).is_err());
        assert!(set_name(&t.0, "CD&DVD", "x1", "Хор, \"Пасхальный\" \\ концерт").is_ok());
        assert_eq!(scan(&t.0).unwrap().items[0].name, "Хор, \"Пасхальный\" \\ концерт");
    }

    #[test]
    fn hidden_and_system_folders_are_ignored_and_files_too() {
        let t = Tmp::new();
        t.mk("CD&DVD/Real");
        t.mk(".git/objects");
        t.mk("$RECYCLE.BIN/x");
        t.mk("System Volume Information/x");
        fs::write(t.0.join("catalog.csv"), "old").unwrap();
        fs::write(t.0.join("CD&DVD/readme.txt"), "x").unwrap();
        assert_eq!(names(&scan(&t.0).unwrap()), ["CD&DVD:Real:1"]);
    }
}