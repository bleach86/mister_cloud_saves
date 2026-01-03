#[macro_use]
extern crate rocket;

use rocket::State;
use rocket::http::Status;
use rocket::response::status::NotFound;
use rocket::{fs::NamedFile, serde::json::Json};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use uuid::Uuid;

mod database;

use database::Database;

use mister_save_utils::{FetchSaveRequest, SaveFile, SaveFileType, UserSaveData};

#[get("/health")]
async fn health() -> Status {
    Status::Ok
}

#[post("/upload_save/<user_id>", data = "<save_file>")]
async fn upload_save(
    user_id: &str,
    save_file: Json<SaveFile>,
    db: &State<Arc<Database>>,
) -> Status {
    let mut _save_file = save_file.clone().into_inner();

    let user_dir = PathBuf::from(format!("user_saves/{}", user_id));

    if !user_dir.exists() {
        return Status::NotFound;
    };

    let saves_folder = match _save_file.save_type {
        SaveFileType::GameSave => "saves",
        SaveFileType::SaveState => "savestates",
        SaveFileType::NvRam => "nvram",
        _ => {
            println!("Unsupported save file type");
            return Status::BadRequest;
        }
    };

    let saves_dir = user_dir.join(saves_folder);
    let core_path = saves_dir.join(&_save_file.core);

    if let Err(e) = tokio::fs::create_dir_all(&core_path).await {
        println!("Failed to create core directory: {:?}", e);
        return Status::InternalServerError;
    }

    let file_path = core_path.join(&format!("{}", &_save_file.name));
    match File::create(&file_path).await {
        Ok(mut file) => {
            if _save_file.data.is_none() {
                println!("No data provided in save file");
                return Status::BadRequest;
            }

            if let Err(e) =
                tokio::io::AsyncWriteExt::write_all(&mut file, &_save_file.data.as_ref().unwrap())
                    .await
            {
                println!("Failed to write save file: {:?}", e);
                return Status::InternalServerError;
            }
        }
        Err(e) => {
            println!("Failed to create save file: {:?}", e);
            return Status::InternalServerError;
        }
    }

    _save_file.data = None; // Clear data before storing metadata

    match db.set_user_save_data(user_id, &_save_file) {
        Some(true) => {}
        _ => {
            println!("Failed to update user save data for user_id: {}", user_id);
            return Status::InternalServerError;
        }
    };

    Status::Ok
}

#[post("/fetch_save", data = "<save_request>")]
async fn fetch_save(save_request: Json<FetchSaveRequest>) -> Result<NamedFile, NotFound<String>> {
    let user_dir = PathBuf::from(format!("user_saves/{}", &save_request.user_id));

    if !user_dir.exists() {
        return Err(NotFound(format!(
            "User directory not found for user_id: {}",
            save_request.user_id
        )));
    };

    let save_folder = match save_request.save_type {
        SaveFileType::GameSave => "saves",
        SaveFileType::SaveState => "savestates",
        SaveFileType::NvRam => "nvram",
        _ => {
            return Err(NotFound(format!(
                "Unsupported save file type: {:?}",
                save_request.save_type
            )));
        }
    };

    let path = format!(
        "user_saves/{}/{}/{}/{}",
        &save_request.user_id, save_folder, &save_request.core, &save_request.name
    );

    match NamedFile::open(PathBuf::from(&path)).await {
        Ok(file) => Ok(file),
        Err(_) => Err(NotFound(format!("Save file not found"))),
    }
}

#[get("/fetch_user_data/<user_id>")]
async fn fetch_user_data(
    user_id: &str,
    db: &State<Arc<Database>>,
) -> Result<Json<UserSaveData>, NotFound<String>> {
    let user_save_data: UserSaveData = match db.get_user_save_data(user_id) {
        Some(data) => data,
        None => {
            return Err(NotFound(format!(
                "No save data found for user_id: {}",
                user_id
            )));
        }
    };
    Ok(Json(user_save_data))
}

#[get("/generate_user_id")]
async fn generate_user_id() -> Result<String, Status> {
    let user_id = Uuid::new_v4();

    let user_dir = PathBuf::from(format!("user_saves/{}", user_id));

    if let Err(e) = tokio::fs::create_dir_all(&user_dir).await {
        println!("Failed to create user directory: {:?}", e);
        return Err(Status::InternalServerError);
    }

    match tokio::fs::create_dir_all(user_dir.join("saves")).await {
        Ok(_) => {}
        Err(e) => {
            println!("Failed to create saves directory: {:?}", e);
            return Err(Status::InternalServerError);
        }
    }

    match tokio::fs::create_dir_all(user_dir.join("savestates")).await {
        Ok(_) => {}
        Err(e) => {
            println!("Failed to create states directory: {:?}", e);
            return Err(Status::InternalServerError);
        }
    }

    match tokio::fs::create_dir_all(user_dir.join("nvram")).await {
        Ok(_) => {}
        Err(e) => {
            println!("Failed to create nvram directory: {:?}", e);
            return Err(Status::InternalServerError);
        }
    }

    Ok(user_id.to_string())
}

#[launch]
fn rocket() -> _ {
    let db = Arc::new(Database::new("user_saves_sled").expect("Failed to open database"));

    rocket::build().manage(db).mount(
        "/",
        routes![
            generate_user_id,
            upload_save,
            fetch_save,
            fetch_user_data,
            health
        ],
    )
}
