use ::base64::prelude::*;
use rocket::response::{Responder, Response, Result as RocketResult};
use rocket::{
    Request, State,
    http::{Header, Status},
    request::{FromRequest, Outcome},
};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use url::Url;
use urlencoding::decode;

use crate::Database;

use mister_save_utils::{read_file_to_bytes, rzip::RzipStream};

const STORAGE_ROOT: &str = "storage";

pub struct BasicAuth {
    pub username: String,
    pub destination: Option<String>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for BasicAuth {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if let Some(auth_header) = request.headers().get_one("Authorization") {
            if auth_header.starts_with("Basic ") {
                let encoded = &auth_header[6..];
                if let Ok(decoded_bytes) = BASE64_STANDARD.decode(encoded) {
                    if let Ok(decoded_str) = String::from_utf8(decoded_bytes) {
                        let parts: Vec<&str> = decoded_str.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let username = parts[0].to_string();
                            let _password = parts[1].to_string();

                            let _db = match request.guard::<&State<Arc<Database>>>().await {
                                Outcome::Success(db) => db,
                                _ => {
                                    return Outcome::Error((Status::InternalServerError, ()));
                                }
                            };

                            let destination = request
                                .headers()
                                .get_one("Destination")
                                .map(|s| s.to_string());
                            let decoded_destination = decode_destination(destination);

                            return Outcome::Success(BasicAuth {
                                username,
                                destination: decoded_destination,
                            });
                        }
                    }
                }
            }
        }
        Outcome::Error((Status::Unauthorized, ()))
    }
}

pub struct WebDavOptions;

impl<'r> Responder<'r, 'static> for WebDavOptions {
    fn respond_to(self, _: &'r Request<'_>) -> RocketResult<'static> {
        Response::build()
            .status(Status::Ok)
            .header(Header::new("DAV", "1"))
            .header(Header::new(
                "Allow",
                "OPTIONS, GET, HEAD, POST, PUT, DELETE, MKCOL, MOVE",
            ))
            .header(Header::new("MS-Author-Via", "DAV"))
            .sized_body(0, Cursor::new(""))
            .ok()
    }
}

#[options("/retroarch")]
pub fn options_root() -> WebDavOptions {
    WebDavOptions
}

#[options("/retroarch/<_path..>")]
pub fn options_sub(_path: PathBuf) -> WebDavOptions {
    WebDavOptions
}

#[route("/retroarch/<path..>", method = "MKCOL")]
pub async fn mkcol(path: PathBuf, auth: BasicAuth) -> Status {
    let full_path = safe_path(path, &auth.username);

    if full_path.exists() {
        return Status::MethodNotAllowed;
    }

    match fs::create_dir_all(&full_path).await {
        Ok(_) => Status::Created,
        Err(_) => Status::InternalServerError,
    }
}

#[route("/retroarch/<path..>", method = "MOVE")]
pub async fn move_dav_file(path: PathBuf, auth: BasicAuth) -> Status {
    let full_path = safe_path(path, &auth.username);

    // Extract the Destination header
    let destination_header = match auth.destination.as_deref() {
        Some(dest) => dest,
        None => return Status::BadRequest,
    };

    let destination_path = PathBuf::from(destination_header);
    let full_destination_path = safe_path(destination_path, &auth.username);

    if full_path == full_destination_path {
        return Status::Forbidden;
    }

    if !full_path.exists() {
        return Status::NotFound;
    }

    let existing = if full_destination_path.exists() {
        if full_destination_path.is_dir() {
            if let Err(e) = fs::remove_dir_all(&full_destination_path).await {
                println!("Failed to remove existing directory: {:?}", e);
                return Status::InternalServerError;
            };
        } else {
            if let Err(e) = fs::remove_file(&full_destination_path).await {
                println!("Failed to remove existing file: {:?}", e);
                return Status::InternalServerError;
            };
        }
        true
    } else {
        false
    };

    if let Some(parent) = full_destination_path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent).await {
                println!("Failed to create parent directories: {:?}", e);
                return Status::InternalServerError;
            }
        }
    }

    if let Err(e) = fs::rename(&full_path, &full_destination_path).await {
        println!("Failed to move file: {:?}", e);
        return Status::InternalServerError;
    }

    if existing {
        Status::NoContent
    } else {
        Status::Created
    }
}

#[get("/retroarch/<path..>")]
pub async fn get_dav_file(path: PathBuf, auth: BasicAuth) -> Result<Vec<u8>, Status> {
    let full_path = safe_path(path, &auth.username);

    if !full_path.exists() || full_path.is_dir() {
        return Err(Status::NotFound);
    }

    let file_bytes = match read_file_to_bytes(&full_path).await {
        Ok(bytes) => bytes,
        Err(e) => {
            println!("Failed to read file: {:?}", e);
            return Err(Status::InternalServerError);
        }
    };

    Ok(file_bytes)
}

#[put("/retroarch/<path..>", data = "<data>")]
pub async fn put_dav_file(path: PathBuf, data: Vec<u8>, auth: BasicAuth) -> Status {
    let full_path = safe_path(path, &auth.username);
    if let Some(parent) = full_path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            println!("Failed to create parent directories: {:?}", e);
            return Status::InternalServerError;
        }
    }

    match fs::write(&full_path, &data).await {
        Ok(_) => Status::Ok,
        Err(e) => {
            println!("Failed to write file: {:?}", e);
            Status::InternalServerError
        }
    }
}

#[delete("/retroarch/<path..>")]
pub async fn delete_dav_file(path: PathBuf, auth: BasicAuth) -> Status {
    let full_path = safe_path(path, &auth.username);
    match fs::remove_file(&full_path).await {
        Ok(_) => Status::Ok,
        Err(e) => {
            println!("Failed to delete file: {:?}", e);
            Status::InternalServerError
        }
    }
}

fn safe_path(path: PathBuf, user_id: &str) -> PathBuf {
    let mut full = PathBuf::from(STORAGE_ROOT).join(user_id);
    for part in &path {
        if part != ".." {
            full.push(part);
        }
    }
    full
}

fn decode_destination(destination: Option<String>) -> Option<String> {
    match &destination {
        Some(dest) => match Url::parse(dest) {
            Ok(url) => match decode(url.path()) {
                Ok(decoded_path) => {
                    let decoded_path = decoded_path.into_owned();
                    decoded_path
                        .strip_prefix("/retroarch/")
                        .map(|s| s.to_string())
                }
                Err(_) => None,
            },
            Err(_) => None,
        },
        None => None,
    }
}
