#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager; // Needed in v2
use tauri_plugin_dialog::DialogExt; // v2 dialog plugin

#[derive(Debug, Serialize, Deserialize)]
struct FileItem {
    name: String,
    path: String,
    #[serde(rename = "type")]
    file_type: String,
    children: Option<Vec<FileItem>>,
}

#[tauri::command]
async fn open_directory_picker(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .blocking_pick_folder();

    match picked {
        Some(file_path) => {
            match file_path {
                tauri_plugin_dialog::FilePath::Path(path_buf) => {
                    Ok(Some(path_buf.to_string_lossy().to_string()))
                }
                tauri_plugin_dialog::FilePath::Url(url) => {
                    Ok(Some(url.to_string()))
                }
            }
        }
        None => Ok(None),
    }
}



#[tauri::command]
async fn get_vault_files(path: String) -> Result<Vec<FileItem>, String> {
    let vault_path = Path::new(&path);

    if !vault_path.exists() || !vault_path.is_dir() {
        return Err("Invalid vault path".to_string());
    }

    match read_directory_recursive(vault_path) {
        Ok(items) => Ok(items),
        Err(e) => Err(format!("Failed to read directory: {}", e)),
    }
}

fn read_directory_recursive(dir_path: &Path) -> Result<Vec<FileItem>, std::io::Error> {
    let mut items = Vec::new();

    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files and directories
        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            let children = read_directory_recursive(&path).ok();
            items.push(FileItem {
                name,
                path: path.to_string_lossy().to_string(),
                file_type: "folder".to_string(),
                children,
            });
        } else if let Some(extension) = path.extension() {
            if extension == "md" {
                items.push(FileItem {
                    name,
                    path: path.to_string_lossy().to_string(),
                    file_type: "file".to_string(),
                    children: None,
                });
            }
        }
    }

    // Sort: folders first, then files alphabetically
    items.sort_by(|a, b| match (a.file_type.as_str(), b.file_type.as_str()) {
        ("folder", "file") => std::cmp::Ordering::Less,
        ("file", "folder") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(items)
}

#[tauri::command]
async fn read_note(path: String) -> Result<String, String> {
    let note_path = Path::new(&path);

    if !note_path.exists() || !note_path.is_file() {
        return Err("Note file does not exist".to_string());
    }

    match fs::read_to_string(note_path) {
        Ok(content) => Ok(content),
        Err(e) => Err(format!("Failed to read note: {}", e)),
    }
}

#[tauri::command]
async fn write_note(path: String, content: String) -> Result<(), String> {
    let note_path = Path::new(&path);

    // Ensure parent directory exists
    if let Some(parent) = note_path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                return Err(format!("Failed to create directory: {}", e));
            }
        }
    }

    match fs::write(note_path, content) {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("Failed to write note: {}", e)),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init()) // required for dialog
        .invoke_handler(tauri::generate_handler![
            open_directory_picker,
            get_vault_files,
            read_note,
            write_note
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
