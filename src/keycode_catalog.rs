use crate::logic::keycode;
use crate::KeycodeEntry;
use slint::SharedString;

/// Build the full list of keycode entries for the key selector, grouped by category.
/// Entries with code=-1 are section headers.
pub fn build_keycode_entries() -> Vec<KeycodeEntry> {
    let mut e = Vec::new();

    // Letters A-Z (0x04 - 0x1D)
    push_header(&mut e, "Letters");
    for code in 0x04u8..=0x1D {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Letters");
    }

    // Numbers 0-9 (0x1E - 0x27)
    push_header(&mut e, "Numbers");
    for code in 0x1Eu8..=0x27 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Numbers");
    }

    // Modifiers (0xE0 - 0xE7)
    push_header(&mut e, "Modifiers");
    for code in 0xE0u8..=0xE7 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Modifiers");
    }

    // Navigation
    push_header(&mut e, "Navigation");
    for code in [0x28u8, 0x29, 0x2A, 0x2B, 0x2C, 0x39, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52] {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Navigation");
    }

    // F-Keys (F1-F24)
    push_header(&mut e, "F-Keys");
    for code in 0x3Au8..=0x45 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "F-Keys");
    }
    for code in 0x68u8..=0x73 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "F-Keys");
    }

    // Punctuation
    push_header(&mut e, "Punctuation");
    for code in 0x2Du8..=0x38 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Punctuation");
    }

    // Layers - MO, TO, OSL
    push_header(&mut e, "Layers");
    for layer in 0..10 {
        let code = ((layer + 1) << 8) as i32;
        push_entry(&mut e, code, &format!("MO {}", layer), "Layers");
    }
    for layer in 0..10 {
        let code = ((layer + 0x0B) << 8) as i32;
        push_entry(&mut e, code, &format!("TO {}", layer), "Layers");
    }
    for layer in 0..10 {
        let code = (0x3100 + layer) as i32;
        push_entry(&mut e, code, &format!("OSL {}", layer), "Layers");
    }

    // Special
    push_header(&mut e, "Special");
    let specials: &[(u16, &str)] = &[
        (0x0000, "None"), (0x3200, "Caps Word"), (0x3300, "Repeat"),
        (0x3400, "Leader"), (0x3500, "Feed"), (0x3600, "Play"),
        (0x3700, "Sleep"), (0x3800, "Meds"), (0x3900, "GEsc"),
        (0x3A00, "Layer Lock"), (0x3C00, "AS Toggle"),
    ];
    for &(code, label) in specials {
        push_entry(&mut e, code as i32, label, "Special");
    }

    // One-Shot Mod
    push_header(&mut e, "One-Shot Mod");
    let osm_mods: &[(u8, &str)] = &[
        (0x01, "OSM Ctrl"), (0x02, "OSM Shift"), (0x04, "OSM Alt"),
        (0x08, "OSM GUI"), (0x10, "OSM RCtrl"), (0x20, "OSM RShift"),
        (0x40, "OSM RAlt"), (0x80, "OSM RGUI"),
    ];
    for &(mod_mask, label) in osm_mods {
        push_entry(&mut e, 0x3000 + mod_mask as i32, label, "One-Shot Mod");
    }

    // Bluetooth
    push_header(&mut e, "Bluetooth");
    let bt: &[(u16, &str)] = &[
        (0x2900, "BT Next"), (0x2A00, "BT Prev"), (0x2B00, "BT Pair"),
        (0x2C00, "BT Disc"), (0x2E00, "USB/BT"), (0x2F00, "BT On/Off"),
    ];
    for &(code, label) in bt {
        push_entry(&mut e, code as i32, label, "Bluetooth");
    }

    // Media
    push_header(&mut e, "Media");
    let media: &[(u8, &str)] = &[
        (0x7F, "Mute"), (0x80, "Vol Up"), (0x81, "Vol Down"),
    ];
    for &(code, label) in media {
        push_entry(&mut e, code as i32, label, "Media");
    }

    // Numpad
    push_header(&mut e, "Numpad");
    for code in 0x53u8..=0x63 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Numpad");
    }

    // Macros M1-M20
    push_header(&mut e, "Macros");
    for idx in 1..=20 {
        let code = ((0x14 + idx) << 8) as i32;
        push_entry(&mut e, code, &format!("M{}", idx), "Macros");
    }

    e
}

pub fn push_header(entries: &mut Vec<KeycodeEntry>, name: &str) {
    entries.push(KeycodeEntry {
        code: -1,
        label: SharedString::from(name),
        category: SharedString::from(name),
    });
}

pub fn push_entry(entries: &mut Vec<KeycodeEntry>, code: i32, label: &str, category: &str) {
    entries.push(KeycodeEntry {
        code,
        label: SharedString::from(label),
        category: SharedString::from(category),
    });
}

/// Filter keycode entries by search text (case-insensitive).
/// Preserves section headers if the section has at least one matching entry.
pub fn filter_keycode_entries(all: &[KeycodeEntry], filter: &str) -> Vec<KeycodeEntry> {
    if filter.is_empty() {
        return all.to_vec();
    }
    let lower = filter.to_lowercase();
    let mut result = Vec::new();
    let mut i = 0;
    while i < all.len() {
        if all[i].code == -1 {
            // This is a section header. Collect all entries in this section.
            let header_idx = i;
            i += 1;
            let mut section_entries = Vec::new();
            while i < all.len() && all[i].code != -1 {
                if all[i].label.to_lowercase().contains(&lower) {
                    section_entries.push(all[i].clone());
                }
                i += 1;
            }
            if !section_entries.is_empty() {
                result.push(all[header_idx].clone());
                result.extend(section_entries);
            }
        } else {
            i += 1;
        }
    }
    result
}
