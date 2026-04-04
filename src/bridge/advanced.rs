use crate::msg::{AppShared, BgMsg, spawn_command};
use crate::logic::{binary_protocol, protocol, parsers};
use crate::{MainWindow, AppState, AdvancedBridge};
use slint::{ComponentHandle, Model};

pub fn setup(window: &MainWindow, shared: &AppShared) {
    setup_refresh_all(window, shared);
    setup_save_td(window, shared);
    setup_delete_combo(window, shared);
    setup_delete_leader(window, shared);
    setup_delete_ko(window, shared);
    setup_bt_commands(window, shared);
    setup_tama_commands(window, shared);
    setup_refresh_tama(window, shared);
    setup_save_autoshift(window, shared);
    setup_save_tri_layer(window, shared);
}

fn setup_refresh_all(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<AdvancedBridge>().on_refresh_all(move || {
        if let Some(w) = window_weak.upgrade() {
            w.global::<AppState>().set_status_text("Loading advanced data...".into());
        }
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

            // Query tap dance data
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::TD_LIST, &[]) {
                    Ok(resp) => {
                        let td = parsers::parse_td_binary(&resp.payload);
                        tx.send(BgMsg::TapDanceData(td));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("TD error: {}", e))); }
                }
            } else {
                match ser.query_command(protocol::CMD_TAP_DANCE) {
                    Ok(lines) => {
                        let td = parsers::parse_td_lines(&lines);
                        tx.send(BgMsg::TapDanceData(td));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("TD error: {}", e))); }
                }
            }

            // Query combo data
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::COMBO_LIST, &[]) {
                    Ok(resp) => {
                        let combos = parsers::parse_combo_binary(&resp.payload);
                        tx.send(BgMsg::ComboData(combos));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Combo error: {}", e))); }
                }
            } else {
                match ser.query_command(protocol::CMD_COMBOS) {
                    Ok(lines) => {
                        let combos = parsers::parse_combo_lines(&lines);
                        tx.send(BgMsg::ComboData(combos));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Combo error: {}", e))); }
                }
            }

            // Query leader data
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::LEADER_LIST, &[]) {
                    Ok(resp) => {
                        let leaders = parsers::parse_leader_binary(&resp.payload);
                        tx.send(BgMsg::LeaderData(leaders));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Leader error: {}", e))); }
                }
            } else {
                match ser.query_command(protocol::CMD_LEADER) {
                    Ok(lines) => {
                        let leaders = parsers::parse_leader_lines(&lines);
                        tx.send(BgMsg::LeaderData(leaders));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Leader error: {}", e))); }
                }
            }

            // Query key override data
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::KO_LIST, &[]) {
                    Ok(resp) => {
                        let kos = parsers::parse_ko_binary(&resp.payload);
                        tx.send(BgMsg::KoData(kos));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("KO error: {}", e))); }
                }
            } else {
                match ser.query_command(protocol::CMD_KEY_OVERRIDE) {
                    Ok(lines) => {
                        let kos = parsers::parse_ko_lines(&lines);
                        tx.send(BgMsg::KoData(kos));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("KO error: {}", e))); }
                }
            }

            // Query bluetooth data
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::BT_QUERY, &[]) {
                    Ok(resp) => {
                        let bt_lines = parsers::parse_bt_binary(&resp.payload);
                        tx.send(BgMsg::BtData(bt_lines));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("BT error: {}", e))); }
                }
            } else {
                match ser.query_command(protocol::CMD_BT_STATUS) {
                    Ok(lines) => {
                        tx.send(BgMsg::BtData(lines));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("BT error: {}", e))); }
                }
            }

            // Query autoshift data
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::AUTOSHIFT_TOGGLE, &[0xFF]) {
                    Ok(resp) => {
                        if resp.payload.len() >= 3 {
                            let enabled = resp.payload[0] != 0;
                            let timeout = u16::from_le_bytes([resp.payload[1], resp.payload[2]]) as i32;
                            tx.send(BgMsg::AutoShiftData(enabled, timeout));
                        }
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("AutoShift error: {}", e))); }
                }
            }

            // Query tamagotchi data
            query_tama(&mut ser, &tx);

            tx.send(BgMsg::StatusMsg("Advanced data loaded".into()));
        });
    });
}

/// Shared tamagotchi query logic used by refresh_all and refresh_tama
fn query_tama(ser: &mut crate::logic::serial::SerialManager, tx: &crate::msg::UiSender) {
    if ser.v2 {
        match ser.send_binary(binary_protocol::cmd::TAMA_QUERY, &[]) {
            Ok(resp) => {
                let lines = parsers::parse_tama_binary(&resp.payload);
                parse_tama_lines(&lines, tx);
            }
            Err(e) => { tx.send(BgMsg::StatusMsg(format!("Tama error: {}", e))); }
        }
    } else {
        match ser.query_command(protocol::CMD_TAMA) {
            Ok(lines) => {
                parse_tama_lines(&lines, tx);
            }
            Err(e) => { tx.send(BgMsg::StatusMsg(format!("Tama error: {}", e))); }
        }
    }
}

fn parse_tama_lines(lines: &[String], tx: &crate::msg::UiSender) {
    for line in lines {
        if line.starts_with("TAMA:") {
            let parse_val = |key: &str| -> i32 {
                line.find(key)
                    .and_then(|i| line[i + key.len()..].split_whitespace().next())
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(0)
            };
            let hunger = parse_val("hunger=");
            let happiness = parse_val("happy=");
            let energy = parse_val("energy=");
            let health = parse_val("health=");
            tx.send(BgMsg::TamaData(hunger, happiness, energy, health));
        }
    }
}

fn setup_save_td(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<AdvancedBridge>().on_save_td(move |slot_index| {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let bridge = window.global::<AdvancedBridge>();
        let slots_model = bridge.get_tap_dance_slots();
        let idx = slot_index as usize;
        if idx >= slots_model.row_count() { return; }
        let slot = slots_model.row_data(idx).unwrap();

        let actions: [u16; 4] = [
            slot.tap1_code as u16,
            slot.tap2_code as u16,
            slot.tap3_code as u16,
            slot.hold_code as u16,
        ];

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
            if ser.v2 {
                let payload = binary_protocol::td_set_payload(slot_index as u8, &actions);
                match ser.send_binary(binary_protocol::cmd::TD_SET, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("TD {} saved", slot_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("TD save error: {}", e))); }
                }
            } else {
                let cmd = format!("TDSET {};{:02X},{:02X},{:02X},{:02X}",
                    slot_index, actions[0], actions[1], actions[2], actions[3]);
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("TD {} saved", slot_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("TD save error: {}", e))); }
                }
            }
        });
    });
}

fn setup_delete_combo(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<AdvancedBridge>().on_delete_combo(move |combo_index| {
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
            if ser.v2 {
                let payload = vec![combo_index as u8];
                match ser.send_binary(binary_protocol::cmd::COMBO_DELETE, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Combo {} deleted", combo_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Combo delete error: {}", e))); }
                }
            } else {
                let cmd = protocol::cmd_combodel(combo_index as u8);
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Combo {} deleted", combo_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Combo delete error: {}", e))); }
                }
            }
        });
    });
}

fn setup_delete_leader(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<AdvancedBridge>().on_delete_leader(move |leader_index| {
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
            if ser.v2 {
                let payload = vec![leader_index as u8];
                match ser.send_binary(binary_protocol::cmd::LEADER_DELETE, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Leader {} deleted", leader_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Leader delete error: {}", e))); }
                }
            } else {
                let cmd = protocol::cmd_leaderdel(leader_index as u8);
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Leader {} deleted", leader_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Leader delete error: {}", e))); }
                }
            }
        });
    });
}

fn setup_delete_ko(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<AdvancedBridge>().on_delete_ko(move |ko_index| {
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
            if ser.v2 {
                let payload = vec![ko_index as u8];
                match ser.send_binary(binary_protocol::cmd::KO_DELETE, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("KO {} deleted", ko_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("KO delete error: {}", e))); }
                }
            } else {
                let cmd = protocol::cmd_kodel(ko_index as u8);
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("KO {} deleted", ko_index))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("KO delete error: {}", e))); }
                }
            }
        });
    });
}

fn setup_bt_commands(window: &MainWindow, shared: &AppShared) {
    // BT Next
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_bt_next(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::BT_NEXT, "BT NEXT", "BT Next");
        });
    }

    // BT Prev
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_bt_prev(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::BT_PREV, "BT PREV", "BT Prev");
        });
    }

    // BT Pair
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_bt_pair(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::BT_PAIR, "BT PAIR", "BT Pairing...");
        });
    }

    // BT Disconnect
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_bt_disconnect(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::BT_DISCONNECT, "BT DISCONNECT", "BT Disconnected");
        });
    }

    // BT Toggle USB/BT
    {
        let serial = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_bt_toggle_usb_bt(move || {
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
                if ser.v2 {
                    match ser.send_binary(binary_protocol::cmd::BT_SWITCH, &[0xFF]) {
                        Ok(_) => { tx.send(BgMsg::StatusMsg("USB/BT toggled".into())); }
                        Err(e) => { tx.send(BgMsg::StatusMsg(format!("USB/BT toggle error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("BT TOGGLE");
                    tx.send(BgMsg::StatusMsg("USB/BT toggled".into()));
                }
            });
        });
    }
}

fn setup_tama_commands(window: &MainWindow, shared: &AppShared) {
    // Tama Feed
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_tama_feed(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::TAMA_FEED, "TAMA FEED", "Tama: Fed!");
        });
    }

    // Tama Play
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_tama_play(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::TAMA_PLAY, "TAMA PLAY", "Tama: Played!");
        });
    }

    // Tama Sleep
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_tama_sleep(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::TAMA_SLEEP, "TAMA SLEEP", "Tama: Sleeping...");
        });
    }

    // Tama Meds
    {
        let s = shared.serial.clone();
        let tx = shared.tx.clone();
        window.global::<AdvancedBridge>().on_tama_meds(move || {
            spawn_command(&s, &tx, binary_protocol::cmd::TAMA_MEDICINE, "TAMA MEDS", "Tama: Medicine given!");
        });
    }
}

fn setup_refresh_tama(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<AdvancedBridge>().on_refresh_tama(move || {
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
            query_tama(&mut ser, &tx);
        });
    });
}

fn setup_save_autoshift(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<AdvancedBridge>().on_save_autoshift(move || {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let bridge = window.global::<AdvancedBridge>();
        let enabled = bridge.get_autoshift_enabled();
        let timeout = bridge.get_autoshift_timeout() as u16;
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
            if ser.v2 {
                let payload = vec![
                    if enabled { 1u8 } else { 0u8 },
                    (timeout & 0xFF) as u8,
                    (timeout >> 8) as u8,
                ];
                match ser.send_binary(binary_protocol::cmd::AUTOSHIFT_TOGGLE, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Auto Shift: {} timeout={}ms", if enabled { "ON" } else { "OFF" }, timeout))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("AutoShift error: {}", e))); }
                }
            } else {
                let cmd = if enabled {
                    format!("AUTOSHIFT ON {}", timeout)
                } else {
                    "AUTOSHIFT OFF".to_string()
                };
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Auto Shift: {} timeout={}ms", if enabled { "ON" } else { "OFF" }, timeout))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("AutoShift error: {}", e))); }
                }
            }
        });
    });
}

fn setup_save_tri_layer(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<AdvancedBridge>().on_save_tri_layer(move || {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let bridge = window.global::<AdvancedBridge>();
        let l1 = bridge.get_tri_layer1() as u8;
        let l2 = bridge.get_tri_layer2() as u8;
        let l3 = bridge.get_tri_layer3() as u8;
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
            if ser.v2 {
                let payload = vec![l1, l2, l3];
                match ser.send_binary(binary_protocol::cmd::TRILAYER_SET, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Tri-Layer set: {} + {} = {}", l1, l2, l3))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Tri-Layer error: {}", e))); }
                }
            } else {
                let cmd = protocol::cmd_trilayer(l1, l2, l3);
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Tri-Layer set: {} + {} = {}", l1, l2, l3))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Tri-Layer error: {}", e))); }
                }
            }
        });
    });
}
