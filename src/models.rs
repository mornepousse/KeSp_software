use crate::logic::keycode;
use crate::logic::layout::KeycapPos;
use crate::logic::layout_remap;
use crate::{KeycapData, LayerInfo, MacroStepInfo, MacroInfo};
use slint::{Model, SharedString, VecModel};
use std::rc::Rc;

/// Interpolate a heatmap color from cold (blue) to hot (red).
/// value is 0.0..1.0
pub fn heatmap_color(value: f32) -> slint::Color {
    let r = (value * 255.0).min(255.0) as u8;
    let g = ((1.0 - (value - 0.5).abs() * 2.0) * 255.0).max(0.0) as u8;
    let b = ((1.0 - value) * 255.0).min(255.0) as u8;
    slint::Color::from_argb_u8(255, r, g, b)
}

pub fn build_keycap_model(keys: &[KeycapPos]) -> Rc<VecModel<KeycapData>> {
    let keycaps: Vec<KeycapData> = keys
        .iter()
        .enumerate()
        .map(|(idx, kp)| KeycapData {
            x: kp.x,
            y: kp.y,
            w: kp.w,
            h: kp.h,
            rotation: kp.angle,
            rotation_cx: kp.w / 2.0,
            rotation_cy: kp.h / 2.0,
            label: SharedString::from(format!("{},{}", kp.col, kp.row)),
            sublabel: SharedString::default(),
            keycode: 0,
            color: slint::Color::from_argb_u8(255, 0x44, 0x47, 0x5a),
            selected: false,
            index: idx as i32,
        })
        .collect();
    Rc::new(VecModel::from(keycaps))
}

pub fn build_layer_model(names: &[String]) -> Rc<VecModel<LayerInfo>> {
    let layers: Vec<LayerInfo> = names
        .iter()
        .enumerate()
        .map(|(i, name)| LayerInfo {
            index: i as i32,
            name: SharedString::from(name.as_str()),
            active: i == 0,
        })
        .collect();
    Rc::new(VecModel::from(layers))
}

/// Update keycap labels from keymap data (row x col -> keycode -> label)
pub fn update_keycap_labels(
    keycap_model: &VecModel<KeycapData>,
    keys: &[KeycapPos],
    keymap: &[Vec<u16>],
    layout: &layout_remap::KeyboardLayout,
) {
    for i in 0..keycap_model.row_count() {
        let mut item = keycap_model.row_data(i).unwrap();
        let kp = &keys[i];
        let row = kp.row as usize;
        let col = kp.col as usize;

        if row < keymap.len() && col < keymap[row].len() {
            let code = keymap[row][col];
            let decoded = keycode::decode_keycode(code);
            let remapped = layout_remap::remap_key_label(layout, &decoded);
            let label = remapped.unwrap_or(&decoded).to_string();
            item.keycode = code as i32;
            item.label = SharedString::from(label);
            item.sublabel = if decoded != format!("0x{:04X}", code) {
                SharedString::default()
            } else {
                SharedString::from(format!("0x{:04X}", code))
            };
        }
        keycap_model.set_row_data(i, item);
    }
}

/// Build MacroStepInfo items from in-memory step data for the Slint model.
pub fn build_macro_step_infos(steps: &[(String, u8, u32)]) -> Vec<MacroStepInfo> {
    steps
        .iter()
        .map(|(action, kc, delay)| {
            let label = if action == "delay" {
                format!("{} ms", delay)
            } else {
                keycode::hid_key_name(*kc)
            };
            MacroStepInfo {
                action_type: SharedString::from(action.as_str()),
                keycode: *kc as i32,
                label: SharedString::from(label),
                delay_ms: *delay as i32,
            }
        })
        .collect()
}

/// Build MacroInfo list items from parsed macro entries.
pub fn build_macro_list(entries: &[crate::logic::parsers::MacroEntry]) -> Vec<MacroInfo> {
    entries
        .iter()
        .map(|e| MacroInfo {
            slot: e.slot as i32,
            name: SharedString::from(e.name.as_str()),
            steps: e.steps.len() as i32,
        })
        .collect()
}

/// Convert firmware MacroStep entries into our in-memory edit format.
/// Firmware format: keycode=0xFF means delay (modifier*10 ms),
/// otherwise modifier is a bit field: bit0=press, bit1=release.
/// If modifier==0x01 => press, 0x02 => release, 0x03 => tap (press+release).
pub fn firmware_steps_to_edit(steps: &[crate::logic::parsers::MacroStep]) -> Vec<(String, u8, u32)> {
    steps
        .iter()
        .map(|s| {
            if s.is_delay() {
                ("delay".to_string(), 0, s.delay_ms())
            } else {
                let action = match s.modifier {
                    0x01 => "press",
                    0x02 => "release",
                    _ => "tap", // 0x03 or default
                };
                (action.to_string(), s.keycode, 0)
            }
        })
        .collect()
}

/// Convert in-memory edit steps back to firmware hex format "kc:mod,kc:mod,..."
pub fn edit_steps_to_hex(steps: &[(String, u8, u32)]) -> String {
    steps
        .iter()
        .map(|(action, kc, delay)| {
            if action == "delay" {
                // Delay: keycode=0xFF, modifier = delay_ms / 10
                let ticks = (delay / 10).min(255) as u8;
                format!("{:02X}:{:02X}", 0xFF, ticks)
            } else {
                let modifier = match action.as_str() {
                    "press" => 0x01u8,
                    "release" => 0x02u8,
                    _ => 0x03u8, // tap
                };
                format!("{:02X}:{:02X}", kc, modifier)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
