use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use xxhash_rust::xxh3::xxh3_64;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SaveFileType {
    GameSave,
    SaveState,
    CoreWatch,
}

impl Default for SaveFileType {
    fn default() -> Self {
        SaveFileType::GameSave
    }
}

pub struct SaveCategory<'a> {
    pub save_type: SaveFileType,
    pub local_map: &'a mut HashMap<String, SaveFile>,
    pub remote_map: &'a HashMap<String, SaveFile>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ConflictAction {
    KeepLocal,
    KeepRemote,
    KeepLocalAll,
    KeepRemoteAll,
    AskUser,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SaveFile {
    pub name: String,
    pub save_type: SaveFileType,
    pub core: String,
    pub hash: u64,
    pub modified_index: u64,
    pub user_id: String,
    pub data: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct UserSaveData {
    pub user_id: String,
    pub game_saves: HashMap<String, SaveFile>,
    pub save_states: HashMap<String, SaveFile>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct FetchSaveRequest {
    pub user_id: String,
    pub core: String,
    pub name: String,
    pub save_type: SaveFileType,
    pub modified_index: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct UploadSaveRequest {
    pub path: PathBuf,
    pub save_type: SaveFileType,
    pub modified_index: u64,
    pub server_url: String,
    pub user_id: String,
}

pub async fn read_file_to_bytes(path: &PathBuf) -> std::io::Result<Vec<u8>> {
    let file_bytes = tokio::fs::read(path).await?;
    Ok(file_bytes)
}

pub async fn hash_file(
    path: &PathBuf,
    file_data: Option<&[u8]>,
) -> Result<u64, Box<dyn std::error::Error>> {
    let file_bytes = match file_data {
        Some(data) => data.to_vec(),
        None => read_file_to_bytes(path).await?,
    };
    let hash: u64 = xxh3_64(&file_bytes);

    Ok(hash)
}

pub fn hashes_equal(hash1: u64, hash2: u64) -> bool {
    hash1 == hash2
}
