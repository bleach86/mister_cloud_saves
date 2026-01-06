use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use glob::glob;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use ini::ini;
use reqwest;
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    path::PathBuf,
    process,
    sync::LazyLock,
    time::Duration,
};
use tokio::sync::Mutex;

use mister_save_utils::{
    ConflictAction, FetchSaveRequest, SaveFile, SaveFileType, UploadSaveRequest, UserSaveData,
    hash_file, hashes_equal, is_hidden_path, read_file_to_bytes,
};
mod inotify_watcher;
use inotify_watcher::*;

static SERVER_URL: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));
static USER_ID: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));
static CURRENT_CORE: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));
static IS_ONE_SHOT: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));
static SAVE_MAP_PATH: &str = "/media/fat/cloud_saves/mister_save_map.json";

#[tokio::main]
async fn main() {
    let arg = std::env::args().nth(1);
    if let Some(a) = arg {
        if a.to_lowercase() == "--version" || a.to_lowercase() == "-v" {
            let version: &str = env!("CARGO_PKG_VERSION");
            println!("MiSTer Cloud Saves Client Version v{}", version);
            return;
        } else if a.to_lowercase() == "--one-shot" {
            *IS_ONE_SHOT.lock().await = true;
        }
    }

    create_pid_file().await;
    clean_tmp_files().await;
    read_config().await;
    wait_for_network().await;

    update_save_map().await;
    let _ = sync_saves().await;

    if IS_ONE_SHOT.lock().await.clone() == true {
        return;
    }

    watch_dirs().await;
}

async fn read_config() {
    let config_path = PathBuf::from("/media/fat/cloud_saves.ini");
    if !config_path.exists() {
        println!("Config file not found at {:?}", config_path);
        return;
    }

    let config_path_str = match config_path.to_str() {
        Some(s) => s,
        None => {
            println!("Failed to convert config path to string");
            return;
        }
    };

    let cloud_saves_ini = ini!(config_path_str);

    let server_map = match cloud_saves_ini.get("server") {
        Some(s) => s,
        None => {
            println!("No [server] section in config");
            return;
        }
    };

    let user_map = match cloud_saves_ini.get("user") {
        Some(u) => u,
        None => {
            println!("No [user] section in config");
            return;
        }
    };

    let server_url = match server_map.get("server_url") {
        Some(url) => match url {
            Some(u) => u,
            None => {
                println!("No server URL specified in config");
                return;
            }
        },
        None => {
            println!("No 'server_url' key in [server] section");
            return;
        }
    };

    let user_id = match user_map.get("user_id") {
        Some(id) => match id {
            Some(u) => u,
            None => {
                println!("No user ID specified in config");
                return;
            }
        },
        None => {
            println!("No 'user_id' key in [user] section");
            return;
        }
    };

    *SERVER_URL.lock().await = server_url.clone();
    *USER_ID.lock().await = user_id.clone();
}

async fn clean_tmp_files() {
    let pattern = "/media/fat/cloud_saves/tmp/*";
    for entry in glob(pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    println!("Failed to remove temp file {:?}: {:?}", path, e);
                }
            }
            Err(e) => println!("Glob error: {:?}", e),
        }
    }
}

async fn create_pid_file() {
    if IS_ONE_SHOT.lock().await.clone() == true {
        // No PID file for one-shot runs
        return;
    }

    let pid_path = PathBuf::from("/var/run/mister_save_client.pid");
    let pid = process::id();
    if let Err(e) = tokio::fs::write(&pid_path, pid.to_string()).await {
        println!("Failed to write PID file {:?}: {:?}", pid_path, e);
    }
}

async fn wait_for_network() -> bool {
    let server_url = SERVER_URL.lock().await.clone();
    loop {
        if let Ok(_) = reqwest::get(format!("{}/health", server_url)).await {
            return true;
        } else {
            println!("Waiting for network...");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

pub async fn handle_core_change_event() {
    if CURRENT_CORE.lock().await.as_str() == "MENU" {
        return;
    }

    let core_name_path = PathBuf::from("/tmp/CORENAME");

    let core_name = match tokio::fs::read_to_string(&core_name_path).await {
        Ok(name) => name.trim().to_string(),
        Err(e) => {
            println!(
                "Failed to read core name from {:?}: {:?}",
                core_name_path, e
            );
            return;
        }
    };

    if core_name == "MENU".to_string() {
        update_save_map().await;
        let _ = sync_saves().await;
    }

    *CURRENT_CORE.lock().await = core_name;
}

pub async fn handle_file_event(save_type: SaveFileType, path: PathBuf) {
    let save_map_path = PathBuf::from(SAVE_MAP_PATH);

    let mut save_map = match tokio::fs::read_to_string(&save_map_path).await {
        Ok(content) => match serde_json::from_str::<UserSaveData>(&content) {
            Ok(map) => map,
            Err(e) => {
                println!("Failed to parse save map JSON: {:?}", e);
                return;
            }
        },
        Err(e) => {
            println!("Failed to read save map file: {:?}", e);
            return;
        }
    };

    let save_name = path
        .file_name()
        .map_or("".to_string(), |n| n.to_string_lossy().to_string());

    let core_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map_or("".to_string(), |n| n.to_string_lossy().to_string());

    let file_data = match read_file_to_bytes(&path).await {
        Ok(data) => data,
        Err(e) => {
            println!("Failed to read file {:?}: {:?}", path, e);
            return;
        }
    };

    let file_hash = match hash_file(&path, Some(&file_data)).await {
        Ok(hash) => hash,
        Err(e) => {
            println!("Failed to hash file {:?}: {:?}", path, e);
            return;
        }
    };

    let save_key = format!("{}/{}", core_name, save_name);

    let modified_index: u64 = match save_type {
        SaveFileType::GameSave => insert_save_data(
            &mut save_map.game_saves,
            &save_key,
            &save_name,
            &core_name,
            file_hash,
            SaveFileType::GameSave,
        ),
        SaveFileType::SaveState => insert_save_data(
            &mut save_map.save_states,
            &save_key,
            &save_name,
            &core_name,
            file_hash,
            SaveFileType::SaveState,
        ),
        SaveFileType::NvRam => insert_save_data(
            &mut save_map.nv_ram,
            &save_key,
            &save_name,
            &core_name,
            file_hash,
            SaveFileType::NvRam,
        ),
        SaveFileType::CoreWatch => {
            // Should not happen
            0
        }
    };

    let server_url = SERVER_URL.lock().await.clone();
    let user_id = USER_ID.lock().await.clone();

    upload_file(
        path,
        save_type,
        modified_index,
        Some(&file_data),
        server_url,
        user_id,
    )
    .await;

    let json_data = match serde_json::to_vec(&save_map) {
        Ok(data) => data,
        Err(e) => {
            println!("Failed to serialize save map to JSON: {:?}", e);
            return;
        }
    };

    if let Err(e) = tokio::fs::write(&save_map_path, &json_data).await {
        println!(
            "Failed to write save map to file {:?}: {:?}",
            save_map_path, e
        );
    }
}

fn insert_save_data(
    saves: &mut HashMap<String, SaveFile>,
    save_key: &str,
    save_name: &str,
    core_name: &str,
    file_hash: u64,
    save_type: SaveFileType,
) -> u64 {
    if saves
        .get(save_key)
        .map_or(false, |s| hashes_equal(s.hash, file_hash))
    {
        // No changes
        return saves.get(save_key).map_or(0, |s| s.modified_index);
    }

    let modified_index = saves.get(save_key).map_or(0, |s| s.modified_index + 1);

    let save_file = SaveFile {
        name: save_name.to_string(),
        save_type,
        core: core_name.to_string(),
        hash: file_hash,
        modified_index,
        user_id: "local".to_string(),
        data: None,
    };

    saves.insert(save_key.to_string(), save_file);

    modified_index
}

async fn get_server_data() -> Result<UserSaveData, Box<dyn std::error::Error + Send + Sync>> {
    println!("Fetching user data from server...");
    let client = reqwest::Client::new();
    let server_url = SERVER_URL.lock().await.clone();
    let user_id = USER_ID.lock().await.clone();
    let response = client
        .get(format!("{}/fetch_user_data/{}", server_url, user_id))
        .send()
        .await;

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<UserSaveData>().await {
                    Ok(user_data) => Ok(user_data),
                    Err(e) => Err(e.into()),
                }
            } else {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to fetch user data: HTTP {}", resp.status()),
                )))
            }
        }
        Err(e) => Err(e.into()),
    }
}

async fn sync_saves() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Starting save synchronization...");

    let save_map_path = PathBuf::from(SAVE_MAP_PATH);
    let content = tokio::fs::read_to_string(&save_map_path).await?;
    let mut local_data: UserSaveData = serde_json::from_str(&content)?;
    let remote_data = get_server_data().await?;

    let manage_conflicts = *IS_ONE_SHOT.lock().await;
    let server_url = SERVER_URL.lock().await.clone();
    let user_id = USER_ID.lock().await.clone();

    let mut download_tasks: Vec<FetchSaveRequest> = Vec::new();
    let mut upload_tasks: Vec<UploadSaveRequest> = Vec::new();

    process_category(
        SaveFileType::GameSave,
        &mut local_data.game_saves,
        &remote_data.game_saves,
        manage_conflicts,
        &mut download_tasks,
        &mut upload_tasks,
        server_url.clone(),
        user_id.clone(),
    )
    .await;

    process_category(
        SaveFileType::SaveState,
        &mut local_data.save_states,
        &remote_data.save_states,
        manage_conflicts,
        &mut download_tasks,
        &mut upload_tasks,
        server_url.clone(),
        user_id.clone(),
    )
    .await;

    process_category(
        SaveFileType::NvRam,
        &mut local_data.nv_ram,
        &remote_data.nv_ram,
        manage_conflicts,
        &mut download_tasks,
        &mut upload_tasks,
        server_url.clone(),
        user_id.clone(),
    )
    .await;

    let total_tasks = download_tasks.len() + upload_tasks.len();

    let mp = MultiProgress::new();
    let total_pb = if total_tasks > 0 && manage_conflicts {
        mp.add(ProgressBar::new(total_tasks as u64))
    } else {
        ProgressBar::hidden()
    };

    let download_pb: ProgressBar = if download_tasks.len() > 0 && manage_conflicts {
        mp.add(ProgressBar::new(download_tasks.len() as u64))
    } else {
        ProgressBar::hidden()
    };

    let upload_pb: ProgressBar = if upload_tasks.len() > 0 && manage_conflicts {
        mp.add(ProgressBar::new(upload_tasks.len() as u64))
    } else {
        ProgressBar::hidden()
    };

    let style =
        ProgressStyle::with_template("{prefix:<10} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%)")?;

    total_pb.set_style(style.clone());
    download_pb.set_style(style.clone());
    upload_pb.set_style(style.clone());

    total_pb.set_prefix("Total");
    download_pb.set_prefix("Download");
    upload_pb.set_prefix("Upload");

    for download in download_tasks {
        let _ = fetch_save_file(&download).await;
        download_pb.inc(1);
        total_pb.inc(1);
    }

    for upload in upload_tasks {
        upload_file(
            upload.path,
            upload.save_type,
            upload.modified_index,
            None,
            upload.server_url,
            upload.user_id,
        )
        .await;

        upload_pb.inc(1);
        total_pb.inc(1);
    }

    download_pb.finish();
    upload_pb.finish();
    total_pb.finish_with_message("Sync complete!");

    let json_data = serde_json::to_vec(&local_data)?;
    tokio::fs::write(&save_map_path, &json_data).await?;

    println!("Sync complete!");
    Ok(())
}

async fn process_category(
    save_type: SaveFileType,
    local_saves: &mut HashMap<String, SaveFile>,
    remote_saves: &HashMap<String, SaveFile>,
    manage_conflicts: bool,
    download_tasks: &mut Vec<FetchSaveRequest>,
    upload_tasks: &mut Vec<UploadSaveRequest>,
    server_url: String,
    user_id: String,
) {
    let mut conflict_state = ConflictAction::AskUser;

    let all_keys: HashSet<String> = local_saves
        .keys()
        .chain(remote_saves.keys())
        .cloned()
        .collect();

    for key in all_keys {
        let local_entry = local_saves.get(&key);
        let remote_entry = remote_saves.get(&key);

        match (local_entry, remote_entry) {
            (Some(local), None) => {
                queue_upload(
                    upload_tasks,
                    key.clone(),
                    save_type.clone(),
                    local.modified_index,
                    server_url.clone(),
                    user_id.clone(),
                );
            }

            (None, Some(remote)) => {
                local_saves.insert(key.clone(), remote.clone());
                queue_download(download_tasks, remote.clone(), save_type.clone());
            }

            (Some(local), Some(remote)) => {
                if hashes_equal(local.hash, remote.hash) {
                    if local.modified_index < remote.modified_index {
                        // Update local modified index to match remote
                        if let Some(l_mut) = local_saves.get_mut(&key) {
                            l_mut.modified_index = remote.modified_index;
                        }
                    } else if local.modified_index > remote.modified_index {
                        // Update remote modified index to match local
                        queue_upload(
                            upload_tasks,
                            key.clone(),
                            save_type.clone(),
                            local.modified_index,
                            server_url.clone(),
                            user_id.clone(),
                        );
                    }

                    continue; // Synced
                }

                let local_is_newer = local.modified_index > remote.modified_index;

                if !manage_conflicts {
                    if local_is_newer {
                        queue_upload(
                            upload_tasks,
                            key.clone(),
                            save_type.clone(),
                            local.modified_index,
                            server_url.clone(),
                            user_id.clone(),
                        );
                    } else {
                        local_saves.insert(key.clone(), remote.clone());
                        queue_download(download_tasks, remote.clone(), save_type.clone());
                    }
                    continue;
                }

                let (primary, secondary) = if local_is_newer {
                    (local, remote)
                } else {
                    (remote, local)
                };

                if conflict_state != ConflictAction::KeepLocalAll
                    && conflict_state != ConflictAction::KeepRemoteAll
                {
                    conflict_state = prompt_user_conflict(primary, secondary, local_is_newer);
                }

                match conflict_state {
                    ConflictAction::KeepLocal | ConflictAction::KeepLocalAll => {
                        if local_is_newer {
                            // Standard upload
                            queue_upload(
                                upload_tasks,
                                key.clone(),
                                save_type.clone(),
                                local.modified_index,
                                server_url.clone(),
                                user_id.clone(),
                            );
                        } else {
                            // Force Local: Remote is newer, but we want local.
                            // Bump local index to Remote + 1 so next sync other clients accepts it.
                            let new_idx = remote.modified_index + 1;
                            if let Some(l_mut) = local_saves.get_mut(&key) {
                                l_mut.modified_index = new_idx;
                            }
                            queue_upload(
                                upload_tasks,
                                key.clone(),
                                save_type.clone(),
                                new_idx,
                                server_url.clone(),
                                user_id.clone(),
                            );
                        }
                    }
                    ConflictAction::KeepRemote | ConflictAction::KeepRemoteAll => {
                        // Standard download (overwrites local entry in map)
                        local_saves.insert(key.clone(), remote.clone());
                        queue_download(download_tasks, remote.clone(), save_type.clone());
                    }
                    _ => {} // Should not happen given logic above
                }
            }
            (None, None) => unreachable!(),
        }
    }
}

fn prompt_user_conflict(
    newer: &SaveFile,
    older: &SaveFile,
    local_is_newer: bool,
) -> ConflictAction {
    println!("Conflict: {}/{}", newer.core, newer.name);
    println!(
        "  Newer ({}): Index {}",
        if local_is_newer { "Local" } else { "Remote" },
        newer.modified_index
    );
    println!(
        "  Older ({}): Index {}",
        if local_is_newer { "Remote" } else { "Local" },
        older.modified_index
    );
    println!("Action: (L)ocal, (R)emote, (LALL) Local All, (RALL) Remote All, (A)bort");

    loop {
        print!("> ");
        match std::io::stdout().flush() {
            Ok(_) => {}
            Err(_) => {
                println!("Failed to flush stdout.");
                continue;
            }
        };

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            println!("Failed to read input.");
            continue;
        }

        return match input.trim().to_uppercase().as_str() {
            "L" => ConflictAction::KeepLocal,
            "R" => ConflictAction::KeepRemote,
            "LALL" => ConflictAction::KeepLocalAll,
            "RALL" => ConflictAction::KeepRemoteAll,
            "A" => {
                println!("Aborting sync.");
                std::process::exit(0);
            }
            _ => {
                println!("Invalid input.");
                continue;
            }
        };
    }
}

fn queue_upload(
    upload_tasks: &mut Vec<UploadSaveRequest>,
    path: String,
    save_type: SaveFileType,
    index: u64,
    server_url: String,
    user_id: String,
) {
    upload_tasks.push(UploadSaveRequest {
        path: PathBuf::from(path),
        save_type,
        modified_index: index,
        server_url,
        user_id,
    });
}

fn queue_download(
    download_tasks: &mut Vec<FetchSaveRequest>,
    remote: SaveFile,
    save_type: SaveFileType,
) {
    let req = FetchSaveRequest {
        user_id: remote.user_id,
        core: remote.core,
        name: remote.name,
        save_type,
        modified_index: remote.modified_index,
    };
    download_tasks.push(req);
}

async fn fetch_save_file(
    request: &FetchSaveRequest,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let server_url = SERVER_URL.lock().await.clone();

    let response = client
        .post(&format!("{}/fetch_save", server_url))
        .json(request)
        .send()
        .await;

    let save_folder = match request.save_type {
        SaveFileType::GameSave => "saves",
        SaveFileType::SaveState => "savestates",
        SaveFileType::NvRam => "config",
        _ => {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Invalid save type",
            )));
        }
    };

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                let bytes = resp.bytes().await?;

                let mut zdecode = ZlibDecoder::new(&bytes[..]);
                let mut decompressed_data = Vec::new();
                zdecode.read_to_end(&mut decompressed_data)?;

                let base_dir = PathBuf::from("/media/fat");
                let save_dir = base_dir.join(save_folder).join(&request.core);

                // Create save directory if it doesn't exist
                tokio::fs::create_dir_all(&save_dir).await?;
                let final_save_path = save_dir.join(&request.name);

                // Write to temp file first
                let temp_dir = base_dir.join("cloud_saves/tmp");
                tokio::fs::create_dir_all(&temp_dir).await?;
                let temp_save_path = temp_dir.join(request.name.clone());

                tokio::fs::write(&temp_save_path, &decompressed_data).await?;

                // Move temp file to final location
                tokio::fs::rename(&temp_save_path, &final_save_path).await?;

                Ok(())
            } else {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to fetch save file: HTTP {}", resp.status()),
                )))
            }
        }
        Err(e) => Err(e.into()),
    }
}

async fn upload_file(
    path: PathBuf,
    save_type: SaveFileType,
    modified_index: u64,
    data: Option<&[u8]>,
    server_url: String,
    user_id: String,
) {
    let base_dir = match save_type {
        SaveFileType::GameSave => PathBuf::from("/media/fat/saves"),
        SaveFileType::SaveState => PathBuf::from("/media/fat/savestates"),
        SaveFileType::NvRam => PathBuf::from("/media/fat/config"),
        _ => {
            println!("Unsupported save type for upload: {:?}", save_type);
            return;
        }
    };

    let full_path = if path.is_absolute() {
        path.clone()
    } else {
        base_dir.join(&path)
    };

    let data = match data {
        Some(d) => d.to_vec(),
        None => match read_file_to_bytes(&full_path).await {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("Failed to read file {:?}: {:?}", full_path, e);
                return;
            }
        },
    };

    let mut zencode: ZlibEncoder<Vec<u8>> = ZlibEncoder::new(Vec::new(), Compression::default());

    if let Err(e) = zencode.write_all(&data) {
        println!("Failed to compress file upload {:?}: {:?}", full_path, e);
        return;
    }

    let compressed_data = match zencode.finish() {
        Ok(data) => data,
        Err(e) => {
            println!(
                "Failed to finish compression for file {:?}: {:?}",
                full_path, e
            );
            return;
        }
    };

    let file_name = match full_path.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => {
            println!("Failed to get file name for {:?}", full_path);
            return;
        }
    };

    let core = match full_path.parent().and_then(|p| p.file_name()) {
        Some(core_name) => core_name.to_string_lossy().to_string(),
        None => {
            println!("Failed to get core name for {:?}", full_path);
            return;
        }
    };

    let file_hash = match hash_file(&full_path, Some(&data)).await {
        Ok(hash) => hash,
        Err(e) => {
            println!("Failed to hash file upload {:?}: {:?}", full_path, e);
            return;
        }
    };

    let save_file = SaveFile {
        name: file_name,
        save_type,
        core,
        hash: file_hash,
        user_id: user_id.clone(),
        data: Some(compressed_data),
        modified_index,
    };

    let client = reqwest::Client::new();
    match client
        .post(&format!("{}/upload_save/{}", server_url, user_id))
        .json(&save_file)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                //println!("Successfully uploaded save file {:?}", full_path);
            } else {
                println!(
                    "Failed to upload save file {:?}: HTTP {}",
                    full_path,
                    resp.status()
                );
            }
        }
        Err(e) => {
            println!("Failed to upload save file {:?}: {:?}", full_path, e);
        }
    }
}

pub async fn update_save_map() {
    let save_types: Vec<SaveFileType> = vec![
        SaveFileType::GameSave,
        SaveFileType::SaveState,
        SaveFileType::NvRam,
    ];
    let save_map_path = PathBuf::from(SAVE_MAP_PATH);

    let mut result: UserSaveData = UserSaveData::default();
    let mut saves: HashMap<String, SaveFile> = HashMap::new();
    let mut save_states: HashMap<String, SaveFile> = HashMap::new();
    let mut nv_rams: HashMap<String, SaveFile> = HashMap::new();

    let mut existing_map: UserSaveData = tokio::fs::read_to_string(&save_map_path)
        .await
        .ok()
        .and_then(|content| serde_json::from_str::<UserSaveData>(&content).ok())
        .unwrap_or_default();

    for save_type in save_types {
        let saves_path = match save_type {
            SaveFileType::GameSave => "/media/fat/saves",
            SaveFileType::SaveState => "/media/fat/savestates",
            SaveFileType::NvRam => "/media/fat/config/nvram",
            SaveFileType::CoreWatch => continue, // skip
        };

        let save_files = match glob(&format!("{}/**/*", saves_path)) {
            Ok(paths) => paths,
            Err(e) => {
                println!("Failed to read glob pattern: {:?}", e);
                continue;
            }
        };

        for entry in save_files {
            if let Ok(path) = entry {
                if is_hidden_path(&path) {
                    continue;
                }

                if path.is_file() {
                    let file_name = path
                        .file_name()
                        .map_or("".to_string(), |n| n.to_string_lossy().to_string());
                    let core_name = path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map_or("".to_string(), |n| n.to_string_lossy().to_string());
                    let save_key = format!("{}/{}", core_name, file_name);

                    let file_hash = match hash_file(&path, None).await {
                        Ok(h) => h,
                        Err(e) => {
                            println!("Failed to hash file {:?}: {:?}", path, e);
                            continue;
                        }
                    };

                    let save_file = SaveFile {
                        name: file_name.clone(),
                        save_type: save_type.clone(),
                        core: core_name.clone(),
                        hash: file_hash,
                        modified_index: 0,
                        user_id: "local".to_string(),
                        data: None,
                    };

                    match save_type {
                        SaveFileType::GameSave => {
                            saves.insert(save_key, save_file);
                        }
                        SaveFileType::SaveState => {
                            save_states.insert(save_key, save_file);
                        }
                        SaveFileType::NvRam => {
                            nv_rams.insert(save_key, save_file);
                        }
                        _ => {}
                    }
                }
            }
        }

        match save_type {
            SaveFileType::GameSave => {
                update_save_files_from(&saves, &mut existing_map.game_saves);
            }
            SaveFileType::SaveState => {
                update_save_files_from(&save_states, &mut existing_map.save_states);
            }
            SaveFileType::NvRam => {
                update_save_files_from(&nv_rams, &mut existing_map.nv_ram);
            }
            _ => {}
        }
    }

    result.game_saves = existing_map.game_saves;
    result.save_states = existing_map.save_states;
    result.nv_ram = existing_map.nv_ram;

    if let Ok(json_data) = serde_json::to_vec(&result) {
        if let Err(e) = tokio::fs::write(&save_map_path, &json_data).await {
            println!("Failed to write save map: {:?}", e);
        }
    } else {
        println!("Failed to serialize save map");
    }
}

fn update_save_files_from(
    new_saves: &HashMap<String, SaveFile>,
    existing_map: &mut HashMap<String, SaveFile>,
) {
    for (k, v) in new_saves.iter() {
        if !existing_map.contains_key(k) {
            existing_map.insert(k.clone(), v.clone());
        } else {
            // Update hash if changed
            let existing_save = existing_map.get_mut(k).unwrap();
            if existing_save.hash != v.hash {
                existing_save.hash = v.hash;
                existing_save.modified_index += 1;
            }
        }
    }
    // Remove entries no longer present
    existing_map.retain(|k, _| new_saves.contains_key(k));
}
