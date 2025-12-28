use mister_save_utils::{SaveFile, SaveFileType, UserSaveData};

use sled::{Db, IVec, Result, Tree};

pub struct Database {
    db: Db,
    user_saves_tree: Tree,
}

impl Database {
    pub fn new(path: &str) -> sled::Result<Self> {
        let db = sled::open(path)?;
        let user_saves_tree = db.open_tree("user_saves_sled")?;
        Ok(Database {
            db,
            user_saves_tree,
        })
    }

    pub fn get_user_save_data(&self, user_id: &str) -> Option<UserSaveData> {
        let mut user_data = UserSaveData::default();
        user_data.user_id = user_id.to_string();

        let prefix = format!("{}/", user_id);
        let iter = self.user_saves_tree.scan_prefix(prefix.as_bytes());

        for item in iter {
            match item {
                Ok((key_ivec, value_ivec)) => {
                    let save_file: SaveFile = match serde_json::from_slice(&value_ivec) {
                        Ok(data) => data,
                        Err(_) => continue,
                    };

                    let save_key = format!("{}/{}", save_file.core, save_file.name);

                    match save_file.save_type {
                        SaveFileType::GameSave => {
                            user_data.game_saves.insert(save_key.clone(), save_file);
                        }
                        SaveFileType::SaveState => {
                            user_data.save_states.insert(save_key.clone(), save_file);
                        }
                        _ => continue,
                    }
                }
                Err(_) => continue,
            }
        }

        Some(user_data)
    }

    pub fn set_user_save_data(&self, user_id: &str, data: &SaveFile) -> Option<bool> {
        let save_key = format!("{}/{}/{}", user_id, data.core, data.name);

        let serialized_data = match serde_json::to_vec(data) {
            Ok(d) => d,
            Err(_) => return None,
        };

        match self.user_saves_tree.insert(save_key, serialized_data) {
            Ok(_) => Some(true),
            Err(_) => None,
        }
    }
}
