use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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
