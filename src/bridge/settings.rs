use crate::msg::{AppShared, BgMsg};
use crate::models::update_keycap_labels;
use crate::logic::{binary_protocol, layout_remap, serial::SerialManager};
use crate::{MainWindow, AppState, SettingsBridge};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::sync::mpsc;

pub fn setup(window: &MainWindow, shared: &AppShared) {
    setup_initial_settings(window);
    setup_change_layout(window, shared);
    setup_backup(window, shared);
    setup_restore(window, shared);
    setup_ota_select_file(window, shared);
    setup_ota_start(window, shared);
    setup_flash_select_file(window, shared);
    setup_flash_start(window, shared);
}

fn setup_initial_settings(window: &MainWindow) {
    // Populate available layouts
    let layout_names: Vec<SharedString> = layout_remap::KeyboardLayout::all()
        .iter()
        .map(|l| SharedString::from(l.name()))
        .collect();
    let layout_model = Rc::new(VecModel::from(layout_names));
    window.global::<SettingsBridge>().set_available_layouts(ModelRc::from(layout_model));

    // Set initial selection to match saved setting
    let saved_settings = crate::logic::settings::load();
    let saved_layout_name = saved_settings.keyboard_layout.to_ascii_uppercase();
    let initial_idx = layout_remap::KeyboardLayout::all()
        .iter()
        .position(|l| l.name() == saved_layout_name)
        .unwrap_or(0);
    window.global::<SettingsBridge>().set_selected_layout_index(initial_idx as i32);

    // Populate programming ports
    let prog_ports: Vec<SharedString> = SerialManager::list_ports()
        .into_iter()
        .map(SharedString::from)
        .collect();
    let prog_ports_model = Rc::new(VecModel::from(prog_ports));
    window.global::<SettingsBridge>().set_prog_ports(ModelRc::from(prog_ports_model));
}

fn setup_change_layout(window: &MainWindow, shared: &AppShared) {
    let keyboard_layout = shared.keyboard_layout.clone();
    let keycap_model = shared.keycap_model.clone();
    let keys_arc = shared.keys.clone();
    let current_keymap = shared.current_keymap.clone();
    let window_weak = window.as_weak();
    window.global::<SettingsBridge>().on_change_layout(move |idx| {
        let all = layout_remap::KeyboardLayout::all();
        let idx = idx as usize;
        if idx >= all.len() { return; }
        let new_layout = all[idx];
        *keyboard_layout.borrow_mut() = new_layout;

        // Save to settings
        let mut settings = crate::logic::settings::load();
        settings.keyboard_layout = new_layout.name().to_string();
        crate::logic::settings::save(&settings);

        // Re-render keycap labels with new layout
        let km = current_keymap.borrow();
        if !km.is_empty() {
            update_keycap_labels(&keycap_model, &keys_arc.lock().unwrap(), &km, &new_layout);
        }

        if let Some(w) = window_weak.upgrade() {
            w.global::<AppState>().set_status_text(
                SharedString::from(format!("Layout: {}", new_layout.name()))
            );
        }
    });
}

fn setup_backup(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<SettingsBridge>().on_backup(move || {
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut ser = match serial.lock() {
                Ok(s) => s,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("Lock error: {}", e)));
                    return;
                }
            };
            if !ser.connected {
                tx.send(BgMsg::StatusMsg("Not connected".into()));
                return;
            }

            let layer_names = ser.get_layer_names().unwrap_or_default();
            let num_layers = layer_names.len().max(1);
            let mut keymaps = Vec::new();
            for l in 0..num_layers {
                match ser.get_keymap(l as u8) {
                    Ok(km) => keymaps.push(km),
                    Err(e) => {
                        tx.send(BgMsg::StatusMsg(format!("Backup error (layer {}): {}", l, e)));
                        return;
                    }
                }
            }
            drop(ser); // release lock before file dialog

            let backup = serde_json::json!({
                "layer_names": layer_names,
                "keymaps": keymaps,
            });
            let json = serde_json::to_string_pretty(&backup).unwrap_or_default();

            let save_dialog = rfd::FileDialog::new()
                .set_title("Save Backup")
                .add_filter("JSON", &["json"])
                .set_file_name("kase_backup.json")
                .save_file();

            match save_dialog {
                Some(path) => {
                    match std::fs::write(&path, &json) {
                        Ok(_) => {
                            tx.send(BgMsg::StatusMsg(format!("Backup saved to {}", path.display())));
                        }
                        Err(e) => {
                            tx.send(BgMsg::StatusMsg(format!("Backup write error: {}", e)));
                        }
                    }
                }
                None => {
                    tx.send(BgMsg::StatusMsg("Backup cancelled".into()));
                }
            }
        });
    });
}

fn setup_restore(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<SettingsBridge>().on_restore(move || {
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let open_dialog = rfd::FileDialog::new()
                .set_title("Restore Backup")
                .add_filter("JSON", &["json"])
                .pick_file();

            let path = match open_dialog {
                Some(p) => p,
                None => {
                    tx.send(BgMsg::StatusMsg("Restore cancelled".into()));
                    return;
                }
            };

            let contents = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("Read error: {}", e)));
                    return;
                }
            };

            let parsed: serde_json::Value = match serde_json::from_str(&contents) {
                Ok(v) => v,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("JSON parse error: {}", e)));
                    return;
                }
            };

            let keymaps = match parsed.get("keymaps").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => {
                    tx.send(BgMsg::StatusMsg("Invalid backup: missing keymaps".into()));
                    return;
                }
            };

            let mut ser = match serial.lock() {
                Ok(s) => s,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("Lock error: {}", e)));
                    return;
                }
            };
            if !ser.connected {
                tx.send(BgMsg::StatusMsg("Not connected".into()));
                return;
            }

            for (layer_idx, layer_val) in keymaps.iter().enumerate() {
                let rows = match layer_val.as_array() {
                    Some(r) => r,
                    None => continue,
                };
                for (row_idx, row_val) in rows.iter().enumerate() {
                    let cols = match row_val.as_array() {
                        Some(c) => c,
                        None => continue,
                    };
                    for (col_idx, code_val) in cols.iter().enumerate() {
                        let code = code_val.as_u64().unwrap_or(0) as u16;
                        if let Err(e) = ser.set_key(layer_idx as u8, row_idx as u8, col_idx as u8, code) {
                            tx.send(BgMsg::StatusMsg(format!("Restore error at L{}R{}C{}: {}", layer_idx, row_idx, col_idx, e)));
                            return;
                        }
                    }
                }
                tx.send(BgMsg::StatusMsg(format!("Restored layer {}/{}", layer_idx + 1, keymaps.len())));
            }

            match ser.get_keymap(0) {
                Ok(km) => { tx.send(BgMsg::Keymap(km)); }
                Err(_) => {}
            }
            tx.send(BgMsg::StatusMsg("Restore complete".into()));
        });
    });
}

fn setup_ota_select_file(window: &MainWindow, shared: &AppShared) {
    let ota_firmware_path = shared.ota_firmware_path.clone();
    let window_weak = window.as_weak();
    window.global::<SettingsBridge>().on_ota_select_file(move || {
        let ota_firmware_path = ota_firmware_path.clone();
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new()
                .set_title("Select OTA Firmware")
                .add_filter("Binary", &["bin"])
                .pick_file();

            if let Some(path) = dialog {
                let display = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                *ota_firmware_path.lock().unwrap() = path.display().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<SettingsBridge>().set_ota_file_name(SharedString::from(&display));
                    }
                });
            }
        });
    });
}

fn setup_ota_start(window: &MainWindow, shared: &AppShared) {
    let ota_firmware_path = shared.ota_firmware_path.clone();
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<SettingsBridge>().on_ota_start(move || {
        let fw_path = ota_firmware_path.lock().unwrap().clone();
        if fw_path.is_empty() { return; }

        if let Some(w) = window_weak.upgrade() {
            w.global::<SettingsBridge>().set_ota_in_progress(true);
            w.global::<SettingsBridge>().set_ota_progress(0.0);
            w.global::<SettingsBridge>().set_ota_status(SharedString::from("Reading firmware file..."));
        }

        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let firmware = match std::fs::read(&fw_path) {
                Ok(data) => data,
                Err(e) => {
                    tx.send(BgMsg::OtaProgress(0.0, format!("File read error: {}", e)));
                    return;
                }
            };

            tx.send(BgMsg::OtaProgress(0.02, format!("Firmware: {} KB", firmware.len() / 1024)));

            let mut ser = match serial.lock() {
                Ok(s) => s,
                Err(e) => {
                    tx.send(BgMsg::OtaProgress(0.0, format!("Lock error: {}", e)));
                    return;
                }
            };
            if !ser.connected || !ser.v2 {
                tx.send(BgMsg::OtaProgress(0.0, "Not connected (v2 required for OTA)".into()));
                return;
            }

            // Send OTA_START with firmware size
            let size = firmware.len() as u32;
            let size_payload = size.to_le_bytes().to_vec();
            match ser.send_binary(binary_protocol::cmd::OTA_START, &size_payload) {
                Ok(_) => {}
                Err(e) => {
                    tx.send(BgMsg::OtaProgress(0.0, format!("OTA start error: {}", e)));
                    return;
                }
            }

            tx.send(BgMsg::OtaProgress(0.05, "OTA started, sending data...".into()));

            // Send data in 512-byte chunks
            let chunk_size = 512;
            let total_chunks = (firmware.len() + chunk_size - 1) / chunk_size;
            for (i, chunk) in firmware.chunks(chunk_size).enumerate() {
                match ser.send_binary(binary_protocol::cmd::OTA_DATA, chunk) {
                    Ok(_) => {}
                    Err(e) => {
                        let _ = ser.send_binary(binary_protocol::cmd::OTA_ABORT, &[]);
                        tx.send(BgMsg::OtaProgress(0.0, format!("OTA data error at chunk {}: {}", i, e)));
                        return;
                    }
                }
                let progress = 0.05 + 0.90 * ((i + 1) as f32 / total_chunks as f32);
                if (i + 1) % 20 == 0 || i + 1 == total_chunks {
                    tx.send(BgMsg::OtaProgress(
                        progress,
                        format!("Sending {}/{} ({} KB / {} KB)", i + 1, total_chunks, ((i + 1) * chunk_size).min(firmware.len()) / 1024, firmware.len() / 1024),
                    ));
                }
            }

            tx.send(BgMsg::OtaProgress(1.0, "OTA complete — device will reboot".into()));
        });
    });
}

fn setup_flash_select_file(window: &MainWindow, shared: &AppShared) {
    let flash_firmware_path = shared.flash_firmware_path.clone();
    let window_weak = window.as_weak();
    window.global::<SettingsBridge>().on_flash_select_file(move || {
        let flash_firmware_path = flash_firmware_path.clone();
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new()
                .set_title("Select ESP32 Firmware")
                .add_filter("Binary", &["bin"])
                .pick_file();

            if let Some(path) = dialog {
                let display = path.display().to_string();
                *flash_firmware_path.lock().unwrap() = display.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<SettingsBridge>().set_prog_path(SharedString::from(&display));
                    }
                });
            }
        });
    });
}

fn setup_flash_start(window: &MainWindow, shared: &AppShared) {
    let flash_firmware_path = shared.flash_firmware_path.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<SettingsBridge>().on_flash_start(move || {
        let fw_path = flash_firmware_path.lock().unwrap().clone();
        if fw_path.is_empty() { return; }

        // Get selected port name from the bridge
        let port_name = if let Some(w) = window_weak.upgrade() {
            let bridge = w.global::<SettingsBridge>();
            let idx = bridge.get_selected_prog_port() as usize;
            let ports_model = bridge.get_prog_ports();
            if idx < ports_model.row_count() {
                ports_model.row_data(idx).map(|s| s.to_string()).unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if port_name.is_empty() {
            tx.send(BgMsg::FlashProgress(0.0, "No port selected".into()));
            return;
        }

        let tx = tx.clone();
        std::thread::spawn(move || {
            let firmware = match std::fs::read(&fw_path) {
                Ok(data) => data,
                Err(e) => {
                    tx.send(BgMsg::FlashProgress(0.0, format!("File read error: {}", e)));
                    return;
                }
            };

            let (flash_tx, flash_rx) = mpsc::channel();
            let port_name_clone = port_name.clone();
            let flash_handle = std::thread::spawn(move || {
                crate::logic::flasher::flash_firmware(&port_name_clone, &firmware, 0x10000, &flash_tx)
            });

            // Forward progress messages
            while let Ok(crate::logic::flasher::FlashProgress::OtaProgress(progress, msg)) = flash_rx.recv() {
                tx.send(BgMsg::FlashProgress(progress, msg));
            }

            match flash_handle.join() {
                Ok(Ok(())) => {
                    tx.send(BgMsg::FlashProgress(1.0, "Flash complete!".into()));
                }
                Ok(Err(e)) => {
                    tx.send(BgMsg::FlashProgress(0.0, format!("Flash error: {}", e)));
                }
                Err(_) => {
                    tx.send(BgMsg::FlashProgress(0.0, "Flash thread panicked".into()));
                }
            }
        });
    });
}
