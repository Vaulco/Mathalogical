//! Library catalog: scans `<root>/<format>/<item folder>/`, keeps a stable id inside
//! each item folder (`id.txt`) and the item data in one master file (`catalog.csv`).

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const ID_FILE: &str = "id.txt";
const CATALOG_FILE: &str = "catalog.csv";
const COVER_FILES: [&str; 3] = ["cover.png", "cover.jpg", "cover.jpeg"];
const HEADERS: [&str; 4] = ["ID", "Format", "Name", "Quantity"];

// Scans and saves never run at the same time.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub format: String,
    pub name: String,
    pub quantity: u32,
    /// Absolute path of the cover image, if the folder has one.
    pub cover: Option<String>,
    /// True when catalog.csv has this item but its folder no longer exists.
    pub missing: bool,
}

#[derive(Debug, Serialize)]
pub struct Library {
    pub items: Vec<Item>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
struct Row {
    id: String,
    format: String,
    name: String,
    quantity: u32,
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || name.eq_ignore_ascii_case("System Volume Information")
}

/// Sub-folders of `dir`, sorted by name (case-insensitive), without hidden/system ones.
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

fn read_id(folder: &Path) -> Option<String> {
    let text = fs::read_to_string(folder.join(ID_FILE)).ok()?;
    let id = text.trim_start_matches('\u{feff}').trim().to_string();
    if id.is_empty() || id.len() > 64 || id.chars().any(|c| c.is_whitespace()) {
        None
    } else {
        Some(id)
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn find_cover(folder: &Path) -> Option<String> {
    COVER_FILES
        .iter()
        .map(|f| folder.join(f))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

fn load_rows(path: &Path) -> Result<Vec<Row>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path).map_err(|e| format!("Couldn't read {CATALOG_FILE}: {e}"))?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes);

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);

    // Columns are found by header name, so reordering them in Excel is fine.
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("Couldn't read {CATALOG_FILE}: {e}"))?
        .iter()
        .map(|h| h.to_lowercase())
        .collect();
    let col = |name: &str| headers.iter().position(|h| h == name);
    let id_col = col("id").ok_or_else(|| format!("{CATALOG_FILE} has no ID column"))?;
    let format_col = col("format");
    let name_col = col("name");
    let qty_col = col("quantity");

    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for record in reader.records() {
        let record = record.map_err(|e| format!("Couldn't read {CATALOG_FILE}: {e}"))?;
        let id = record.get(id_col).unwrap_or("").to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        rows.push(Row {
            id,
            format: format_col.and_then(|i| record.get(i)).unwrap_or("").to_string(),
            name: name_col.and_then(|i| record.get(i)).unwrap_or("").to_string(),
            quantity: qty_col
                .and_then(|i| record.get(i))
                .and_then(|q| q.parse::<u32>().ok())
                .unwrap_or(1),
        });
    }
    Ok(rows)
}

/// CSV bytes: UTF-8 with BOM (so Excel shows Cyrillic correctly), CRLF line endings.
fn encode_rows(rows: &[Row]) -> Vec<u8> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(Vec::new());
    writer.write_record(HEADERS).expect("write to memory");
    for r in rows {
        writer
            .write_record([&r.id, &r.format, &r.name, &r.quantity.to_string()])
            .expect("write to memory");
    }
    let body = writer.into_inner().expect("flush to memory");
    let mut out = vec![0xEF, 0xBB, 0xBF];
    out.extend(body);
    out
}

fn write_rows(path: &Path, rows: &[Row]) -> Result<(), String> {
    let bytes = encode_rows(rows);
    if fs::read(path).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(()); // nothing changed
    }
    let tmp = path.with_extension("csv.tmp");
    let result = fs::write(&tmp, &bytes).and_then(|_| fs::rename(&tmp, path));
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(format!(
            "Couldn't save {CATALOG_FILE} ({e}). If it's open in Excel, close it and try again."
        ));
    }
    Ok(())
}

fn sort_key(format: &str, name: &str) -> (String, String) {
    (format.to_lowercase(), name.to_lowercase())
}

pub fn scan(root: &Path) -> Result<Library, String> {
    let _guard = lock();
    if !root.is_dir() {
        return Err("That folder doesn't exist.".to_string());
    }
    let catalog_path = root.join(CATALOG_FILE);
    // If catalog.csv can't be read we stop here rather than risk overwriting it.
    let rows = load_rows(&catalog_path)?;
    let old: HashMap<&str, &Row> = rows.iter().map(|r| (r.id.as_str(), r)).collect();

    let mut warnings: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
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
        for (name, folder) in folders {
            let mut id = read_id(&folder).filter(|id| !seen.contains(id));
            if id.is_none() {
                // No id yet, or a copied folder that shares another folder's id.
                let fresh = new_id();
                if let Err(e) = fs::write(folder.join(ID_FILE), &fresh) {
                    warnings.push(format!("Couldn't write {ID_FILE} in {format}/{name}: {e}"));
                    continue;
                }
                id = Some(fresh);
            }
            let id = id.unwrap();
            seen.insert(id.clone());
            let quantity = old.get(id.as_str()).map(|r| r.quantity).unwrap_or(1);
            items.push(Item {
                id,
                format: format.clone(),
                name,
                quantity,
                cover: find_cover(&folder),
                missing: false,
            });
        }
    }

    // Rows whose folder is gone stay in the file, flagged as missing.
    for r in &rows {
        if !seen.contains(&r.id) {
            items.push(Item {
                id: r.id.clone(),
                format: r.format.clone(),
                name: r.name.clone(),
                quantity: r.quantity,
                cover: None,
                missing: true,
            });
        }
    }

    items.sort_by(|a, b| sort_key(&a.format, &a.name).cmp(&sort_key(&b.format, &b.name)));

    let new_rows: Vec<Row> = items
        .iter()
        .map(|i| Row {
            id: i.id.clone(),
            format: i.format.clone(),
            name: i.name.clone(),
            quantity: i.quantity,
        })
        .collect();
    if let Err(e) = write_rows(&catalog_path, &new_rows) {
        warnings.push(e);
    }

    Ok(Library {
        items,
        warning: if warnings.is_empty() { None } else { Some(warnings.join("\n")) },
    })
}

pub fn set_quantity(root: &Path, id: &str, quantity: u32) -> Result<(), String> {
    let _guard = lock();
    let catalog_path = root.join(CATALOG_FILE);
    let mut rows = load_rows(&catalog_path)?;
    let row = rows
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| format!("That item isn't in {CATALOG_FILE}."))?;
    row.quantity = quantity;
    write_rows(&catalog_path, &rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Tmp {
            let p = std::env::temp_dir().join(format!("catalog-test-{}", new_id()));
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
        lib.items.iter().map(|i| format!("{}/{}", i.format, i.name)).collect()
    }

    #[test]
    fn first_scan_assigns_ids_and_writes_catalog() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/Beta");
        t.mk("CD&DVD/Alpha");
        t.mk("Books/Some book");
        fs::write(a.join("cover.png"), b"x").unwrap();

        let lib = scan(&t.0).unwrap();
        assert_eq!(names(&lib), ["Books/Some book", "CD&DVD/Alpha", "CD&DVD/Beta"]);
        assert!(lib.warning.is_none());
        assert!(lib.items.iter().all(|i| i.quantity == 1 && !i.missing));
        assert!(lib.items[2].cover.is_some() && lib.items[1].cover.is_none());
        assert!(a.join("id.txt").is_file());

        let bytes = fs::read(t.0.join("catalog.csv")).unwrap();
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        assert!(String::from_utf8_lossy(&bytes).contains("ID,Format,Name,Quantity"));

        // A second scan changes nothing.
        let again = scan(&t.0).unwrap();
        let ids = |l: &Library| l.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&lib), ids(&again));
    }

    #[test]
    fn rename_keeps_id_and_quantity() {
        let t = Tmp::new();
        t.mk("CD&DVD/Old name");
        let id = scan(&t.0).unwrap().items[0].id.clone();
        set_quantity(&t.0, &id, 4).unwrap();

        fs::rename(t.0.join("CD&DVD/Old name"), t.0.join("CD&DVD/New name")).unwrap();
        let lib = scan(&t.0).unwrap();
        assert_eq!(lib.items.len(), 1);
        assert_eq!(lib.items[0].id, id);
        assert_eq!(lib.items[0].name, "New name");
        assert_eq!(lib.items[0].quantity, 4);
        assert!(!lib.items[0].missing);
    }

    #[test]
    fn copied_folder_gets_a_new_id() {
        let t = Tmp::new();
        let a = t.mk("CD&DVD/A");
        let first = scan(&t.0).unwrap().items[0].id.clone();
        let b = t.mk("CD&DVD/B");
        fs::copy(a.join("id.txt"), b.join("id.txt")).unwrap();

        let lib = scan(&t.0).unwrap();
        assert_eq!(lib.items.len(), 2);
        assert_ne!(lib.items[0].id, lib.items[1].id);
        assert_eq!(lib.items[0].id, first); // the original keeps its id
    }

    #[test]
    fn deleted_folder_is_flagged_missing_not_removed() {
        let t = Tmp::new();
        t.mk("CD&DVD/Keep");
        t.mk("CD&DVD/Gone");
        scan(&t.0).unwrap();
        fs::remove_dir_all(t.0.join("CD&DVD/Gone")).unwrap();

        let lib = scan(&t.0).unwrap();
        assert_eq!(lib.items.len(), 2);
        let gone = lib.items.iter().find(|i| i.name == "Gone").unwrap();
        assert!(gone.missing && gone.cover.is_none());
        // ...and it is still in the file after the scan.
        assert!(fs::read_to_string(t.0.join("catalog.csv")).unwrap().contains("Gone"));
    }

    #[test]
    fn quantity_persists_and_unicode_round_trips() {
        let t = Tmp::new();
        t.mk("CD&DVD/Хор, \"Пасхальный\" концерт");
        let id = scan(&t.0).unwrap().items[0].id.clone();
        set_quantity(&t.0, &id, 7).unwrap();

        let lib = scan(&t.0).unwrap();
        assert_eq!(lib.items[0].name, "Хор, \"Пасхальный\" концерт");
        assert_eq!(lib.items[0].quantity, 7);
        assert!(set_quantity(&t.0, "nope", 1).is_err());
    }

    #[test]
    fn hand_edited_csv_is_understood() {
        let t = Tmp::new();
        t.mk("CD&DVD/A");
        let id = scan(&t.0).unwrap().items[0].id.clone();
        // Excel user reorders columns, lowercases headers, leaves a blank quantity row
        fs::write(t.0.join("catalog.csv"), format!("quantity,name,id,format\r\n12,A,{id},CD&DVD\r\n")).unwrap();
        assert_eq!(scan(&t.0).unwrap().items[0].quantity, 12);
        fs::write(t.0.join("catalog.csv"), format!("quantity,name,id,format\r\n,A,{id},CD&DVD\r\n")).unwrap();
        assert_eq!(scan(&t.0).unwrap().items[0].quantity, 1);
    }

    #[test]
    fn unreadable_catalog_is_not_overwritten() {
        let t = Tmp::new();
        t.mk("CD&DVD/A");
        fs::write(t.0.join("catalog.csv"), "just,some,junk\r\n1,2,3\r\n").unwrap();
        assert!(scan(&t.0).is_err());
        assert!(fs::read_to_string(t.0.join("catalog.csv")).unwrap().contains("junk"));
    }

    #[test]
    fn hidden_and_system_folders_are_ignored() {
        let t = Tmp::new();
        t.mk("CD&DVD/Real");
        t.mk(".git/objects");
        t.mk("$RECYCLE.BIN/x");
        t.mk("System Volume Information/x");
        assert_eq!(names(&scan(&t.0).unwrap()), ["CD&DVD/Real"]);
    }
}