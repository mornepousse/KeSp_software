use crate::msg::{AppShared, BgMsg};
use crate::models::{build_macro_step_infos, build_macro_list, firmware_steps_to_edit, edit_steps_to_hex};
use crate::logic::{binary_protocol, protocol};
use crate::{MainWindow, AppState, MacroBridge, MacroStepInfo};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

pub fn setup(window: &MainWindow, shared: &AppShared) {
    setup_refresh_macros(window, shared);
    setup_select_macro(window, shared);
    setup_new_macro(window, shared);
    setup_add_step(window, shared);
    setup_remove_step(window, shared);
    setup_move_step_up(window, shared);
    setup_move_step_down(window, shared);
    setup_save_macro(window, shared);
    setup_delete_macro(window, shared);
}

fn setup_refresh_macros(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_refresh_macros(move || {
        if let Some(w) = window_weak.upgrade() {
            w.global::<AppState>().set_status_text("Loading macros...".into());
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
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::LIST_MACROS, &[]) {
                    Ok(resp) => {
                        let entries = crate::logic::parsers::parse_macros_binary(&resp.payload);
                        tx.send(BgMsg::MacroListData(entries));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Macro error: {}", e))); }
                }
            } else {
                match ser.query_command(protocol::CMD_MACROS_TEXT) {
                    Ok(lines) => {
                        let entries = crate::logic::parsers::parse_macro_lines(&lines);
                        tx.send(BgMsg::MacroListData(entries));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Macro error: {}", e))); }
                }
            }
        });
    });
}

fn setup_select_macro(window: &MainWindow, shared: &AppShared) {
    let macro_entries = shared.macro_entries.clone();
    let macro_steps = shared.macro_steps.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_select_macro(move |slot| {
        let window = match window_weak.upgrade() {
            Some(w) => w,
            None => return,
        };
        let bridge = window.global::<MacroBridge>();
        bridge.set_selected_macro(slot);
        let entries = macro_entries.borrow();
        let found = entries.iter().find(|e| e.slot as i32 == slot);
        match found {
            Some(entry) => {
                bridge.set_macro_name(SharedString::from(entry.name.as_str()));
                let edit_steps = firmware_steps_to_edit(&entry.steps);
                let infos = build_macro_step_infos(&edit_steps);
                *macro_steps.borrow_mut() = edit_steps;
                bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
            }
            None => {
                bridge.set_macro_name(SharedString::default());
                macro_steps.borrow_mut().clear();
                bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::<MacroStepInfo>::from(vec![]))));
            }
        }
    });
}

fn setup_new_macro(window: &MainWindow, shared: &AppShared) {
    let macro_entries = shared.macro_entries.clone();
    let macro_steps = shared.macro_steps.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_new_macro(move || {
        let window = match window_weak.upgrade() { Some(w) => w, None => return };
        let bridge = window.global::<MacroBridge>();
        let entries = macro_entries.borrow();
        let used: Vec<u8> = entries.iter().map(|e| e.slot).collect();
        match (0u8..16).find(|s| !used.contains(s)) {
            Some(slot) => {
                bridge.set_selected_macro(slot as i32);
                bridge.set_macro_name(SharedString::from(format!("Macro{}", slot)));
                macro_steps.borrow_mut().clear();
                bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::<MacroStepInfo>::from(vec![]))));
            }
            None => {
                window.global::<AppState>().set_status_text("All 16 macro slots are used".into());
            }
        }
    });
}

fn setup_add_step(window: &MainWindow, shared: &AppShared) {
    let macro_steps = shared.macro_steps.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_add_step(move |action_type, keycode_or_delay| {
        let window = match window_weak.upgrade() { Some(w) => w, None => return };
        let bridge = window.global::<MacroBridge>();
        let action = action_type.to_string();
        let mut steps = macro_steps.borrow_mut();
        if action == "delay" {
            let delay_str = bridge.get_delay_input().to_string();
            let delay_ms: u32 = delay_str.trim().parse().unwrap_or(50);
            steps.push(("delay".to_string(), 0, delay_ms));
        } else {
            steps.push((action, keycode_or_delay as u8, 0));
        }
        let infos = build_macro_step_infos(&steps);
        bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
    });
}

fn setup_remove_step(window: &MainWindow, shared: &AppShared) {
    let macro_steps = shared.macro_steps.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_remove_step(move |idx| {
        let window = match window_weak.upgrade() { Some(w) => w, None => return };
        let bridge = window.global::<MacroBridge>();
        let mut steps = macro_steps.borrow_mut();
        let i = idx as usize;
        if i < steps.len() { steps.remove(i); }
        let infos = build_macro_step_infos(&steps);
        bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
    });
}

fn setup_move_step_up(window: &MainWindow, shared: &AppShared) {
    let macro_steps = shared.macro_steps.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_move_step_up(move |idx| {
        let window = match window_weak.upgrade() { Some(w) => w, None => return };
        let bridge = window.global::<MacroBridge>();
        let mut steps = macro_steps.borrow_mut();
        let i = idx as usize;
        if i > 0 && i < steps.len() { steps.swap(i, i - 1); }
        let infos = build_macro_step_infos(&steps);
        bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
    });
}

fn setup_move_step_down(window: &MainWindow, shared: &AppShared) {
    let macro_steps = shared.macro_steps.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_move_step_down(move |idx| {
        let window = match window_weak.upgrade() { Some(w) => w, None => return };
        let bridge = window.global::<MacroBridge>();
        let mut steps = macro_steps.borrow_mut();
        let i = idx as usize;
        if i + 1 < steps.len() { steps.swap(i, i + 1); }
        let infos = build_macro_step_infos(&steps);
        bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
    });
}

fn setup_save_macro(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let macro_steps = shared.macro_steps.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_save_macro(move || {
        let window = match window_weak.upgrade() { Some(w) => w, None => return };
        let bridge = window.global::<MacroBridge>();
        let slot = bridge.get_selected_macro();
        if slot < 0 { return; }
        let name = bridge.get_macro_name().to_string();
        let steps = macro_steps.borrow();
        let steps_hex = edit_steps_to_hex(&steps);
        let slot_u8 = slot as u8;
        window.global::<AppState>().set_status_text(SharedString::from(format!("Saving macro {}...", slot)));
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
                let payload = binary_protocol::macro_add_seq_payload(slot_u8, &name, &steps_hex);
                match ser.send_binary(binary_protocol::cmd::MACRO_ADD_SEQ, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Macro {} saved", slot))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Macro save error: {}", e))); }
                }
            } else {
                let cmd = protocol::cmd_macroseq(slot_u8, &name, &steps_hex);
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Macro {} saved", slot))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Macro save error: {}", e))); }
                }
            }
        });
    });
}

fn setup_delete_macro(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let macro_steps = shared.macro_steps.clone();
    let macro_entries = shared.macro_entries.clone();
    let window_weak = window.as_weak();
    window.global::<MacroBridge>().on_delete_macro(move |slot| {
        if slot < 0 { return; }
        let slot_u8 = slot as u8;
        macro_steps.borrow_mut().clear();
        { macro_entries.borrow_mut().retain(|e| e.slot != slot_u8); }
        if let Some(w) = window_weak.upgrade() {
            let bridge = w.global::<MacroBridge>();
            bridge.set_selected_macro(-1);
            bridge.set_macro_name(SharedString::default());
            bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::<MacroStepInfo>::from(vec![]))));
            let entries = macro_entries.borrow();
            let list = build_macro_list(&entries);
            bridge.set_macros(ModelRc::from(Rc::new(VecModel::from(list))));
            w.global::<AppState>().set_status_text(SharedString::from(format!("Deleting macro {}...", slot)));
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
            if ser.v2 {
                let payload = binary_protocol::macro_delete_payload(slot_u8);
                match ser.send_binary(binary_protocol::cmd::MACRO_DELETE, &payload) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Macro {} deleted", slot))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Macro delete error: {}", e))); }
                }
            } else {
                let cmd = protocol::cmd_macro_del(slot_u8);
                match ser.send_command(&cmd) {
                    Ok(_) => { tx.send(BgMsg::StatusMsg(format!("Macro {} deleted", slot))); }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Macro delete error: {}", e))); }
                }
            }
        });
    });
}
