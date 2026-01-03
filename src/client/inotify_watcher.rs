use mister_save_utils::SaveFileType;
use notify_debouncer_full::{new_debouncer, notify::RecursiveMode};
use std::{path::Path, time::Duration};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{handle_core_change_event, handle_file_event};

pub async fn watch_dirs() {
    let save_types: Vec<SaveFileType> = vec![
        SaveFileType::GameSave,
        SaveFileType::CoreWatch,
        SaveFileType::SaveState,
        SaveFileType::NvRam,
    ];
    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    for save_type in save_types {
        let path = match save_type {
            SaveFileType::GameSave => "/media/fat/saves",
            SaveFileType::SaveState => "/media/fat/savestates",
            SaveFileType::CoreWatch => "/tmp/CORENAME",
            SaveFileType::NvRam => "/media/fat/config/nvram",
        };

        let path_clone: String = path.to_string();
        let handle: JoinHandle<()> = tokio::spawn(async move {
            if let Err(error) = watch(&path_clone, save_type).await {
                eprintln!("Error watching {}: {:?}", path_clone, error);
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks (they should run forever)
    for handle in handles {
        let _ = handle.await;
    }
}

pub async fn watch<P: AsRef<Path>>(path: P, save_type: SaveFileType) -> notify::Result<()> {
    let (tx_blocking, rx_blocking) = std::sync::mpsc::channel();

    let (tx_async, mut rx_async) = mpsc::unbounded_channel();

    let mut debouncer = new_debouncer(Duration::from_millis(2500), None, tx_blocking)?;
    debouncer.watch(path.as_ref(), RecursiveMode::Recursive)?;

    tokio::task::spawn_blocking(move || {
        for result in rx_blocking {
            if tx_async.send(result).is_err() {
                break;
            }
        }
    });

    while let Some(result) = rx_async.recv().await {
        match result {
            Ok(events) => {
                for event in events {
                    match &event.kind {
                        notify::EventKind::Create(_) => {
                            println!("File created: {:?}", event.paths);

                            if save_type == SaveFileType::CoreWatch {
                                continue;
                            }

                            for path in &event.paths {
                                handle_file_event(save_type.clone(), path.clone()).await;
                            }
                        }
                        notify::EventKind::Modify(_) => {
                            if save_type == SaveFileType::CoreWatch {
                                handle_core_change_event().await;
                                continue;
                            }

                            for path in &event.paths {
                                handle_file_event(save_type.clone(), path.clone()).await;
                            }
                        }
                        notify::EventKind::Remove(_) => {
                            //
                        }
                        _ => {}
                    }
                }
            }
            Err(errors) => {
                for error in errors {
                    eprintln!("Error: {error:?}");
                }
            }
        }
    }

    Ok(())
}
