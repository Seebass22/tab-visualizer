use nannou::prelude::Vec2;
use regex::Regex;

pub const BLOW_Y: f32 = 50.0;
pub const DRAW_X: f32 = -480.0;
pub const HOLE_X_DIST: f32 = 110.0;
pub const HOLE_Y_DIST: f32 = 45.0;
pub const DRAW_Y: f32 = BLOW_Y - 2.0 * HOLE_Y_DIST;

pub fn calc_note_positions(tuning_notes: &[String]) -> Vec<Vec2> {
    let re = Regex::new(r"(?P<dir>-?)(?P<note>\d{1,2})(?P<rest>.*)").unwrap();
    let mut res = Vec::new();

    for note in tuning_notes.iter() {
        if let Some(caps) = re.captures(note) {
            let direction = &caps["dir"]; // "-" or ""
            let bends_or_ob = &caps["rest"];

            let mut y = if direction == "-" { DRAW_Y } else { BLOW_Y };

            if let Ok(note_n) = &caps["note"].parse::<usize>() {
                let x = DRAW_X + (note_n - 1) as f32 * HOLE_X_DIST;
                let y_offset = bends_or_ob.len() as f32 * HOLE_Y_DIST;

                if direction == "-" {
                    y -= y_offset;
                } else {
                    y += y_offset;
                }
                res.push(Vec2::new(x, y));
            } else {
                // invalid note numeber?
                res.push(Vec2::new(1000.0, 1000.0));
                println!("note {} not found", note);
            }
        } else {
            // regex miss?
            res.push(Vec2::new(1000.0, 1000.0));
            println!("note {} not found", note);
        }
    }
    res
}

pub fn freq_to_midi(freq: f32) -> u8 {
    (12.0 * (freq / 440.0).log2() + 69.0).round() as u8
}

pub fn get_harmonica_key_semitone_offset(key: &str) -> i8 {
    match key {
        "C" => 0,
        "G" => -5,
        "D" => 2,
        "A" => -3,
        "E" => 4,
        "B" => -1,
        "F#" => 6,
        "Db" => 1,
        "Ab" => -4,
        "Eb" => 3,
        "Bb" => -2,
        "F" => 5,
        "LF" => -7,
        "LC" => -12,
        "LD" => -10,
        "HG" => 7,
        _ => {
            panic!()
        }
    }
}

pub fn midi_to_tab(midi: u8, key: &str, notes_in_order: &[String]) -> String {
    let offset = get_harmonica_key_semitone_offset(key);
    let index: isize = midi as isize - 60 - offset as isize;
    if index < 0 || index > notes_in_order.len() as isize - 1 {
        return "".to_owned();
    }
    notes_in_order[index as usize].to_owned()
}
