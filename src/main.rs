use std::path::Path;

use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use nannou_egui::{self, egui, Egui};
use ordered_float::NotNan;
use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;
use regex::Regex;
use ringbuf::HeapRb;

struct Model {
    _in_stream: audio::Stream<InputModel>,
    consumer: ringbuf::HeapConsumer<f32>,
    tuning_notes: Vec<String>,
    current_note: String,
    current_level: f32,
    ui_visible: bool,
    egui: Egui,
    settings: Settings,
    is_running: bool,

    texture: wgpu::Texture,
    note_positions: Vec<Vec2>,
    last_frequency: f32,
}

fn calc_note_positions(tuning_notes: &[String]) -> Vec<Vec2> {
    let re = Regex::new(r"(?P<dir>-?)(?P<note>\d{1,2})(?P<rest>.*)").unwrap();
    let blow_y = 31.0;
    let draw_y = -10.0;
    let draw_x = -250.0;
    let hole_x_dist = 55.0;
    let hole_y_dist = 21.0;
    let mut res = Vec::new();
    println!("{:?}", tuning_notes);

    for note in tuning_notes.iter() {
        if let Some(caps) = re.captures(note) {
            let direction = &caps["dir"]; // "-" or ""
            let bends_or_ob = &caps["rest"];
            println!("{}, {}, {}", direction, &caps["note"], bends_or_ob);

            let mut y = if direction == "-" { draw_y } else { blow_y };

            if let Ok(note_n) = &caps["note"].parse::<usize>() {
                let x = draw_x + (note_n - 1) as f32 * hole_x_dist;
                let y_offset = bends_or_ob.len() as f32 * hole_y_dist;

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

struct Settings {
    power_threshold: f32,
    clarity_threshold: f32,
    key: &'static str,
    tuning: &'static str,
}

fn main() {
    nannou::app(model).update(update).run();
}

fn model(app: &App) -> Model {
    let window_id = app
        .new_window()
        .view(view)
        .raw_event(raw_window_event)
        .key_pressed(key_pressed)
        .size(640, 280)
        .build()
        .unwrap();

    let window = app.window(window_id).unwrap();
    let egui = Egui::from_window(&window);

    // Initialise the audio host so we can spawn an audio stream.
    let audio_host = audio::Host::new();

    // Create a ring buffer and split it into producer and consumer
    let latency_samples = 8192;
    let ring_buffer = HeapRb::<f32>::new(latency_samples * 2); // Add some latency
    let (mut prod, cons) = ring_buffer.split();
    for _ in 0..latency_samples {
        // The ring buffer has twice as much space as necessary to add latency here,
        // so this should never fail
        prod.push(0.0).unwrap();
    }

    // Create input model and input stream using that model
    let in_model = InputModel { producer: prod };
    let in_stream = audio_host
        .new_input_stream(in_model)
        .capture(pass_in)
        .build()
        .unwrap();

    in_stream.play().unwrap();

    let texture = wgpu::Texture::from_path(app, Path::new("layout.png")).unwrap();
    let tuning_notes = harptabber::tuning_to_notes_in_order("richter").0;
    let note_positions = calc_note_positions(&tuning_notes);

    Model {
        _in_stream: in_stream,
        consumer: cons,
        tuning_notes,
        current_note: "4".to_owned(),
        current_level: 0.0,
        ui_visible: false,
        egui,
        is_running: false,
        settings: Settings {
            power_threshold: 3.0,
            clarity_threshold: 0.7,
            key: "C",
            tuning: "richter",
        },
        texture,
        note_positions,
        last_frequency: 0.0,
    }
}

fn update(_app: &App, model: &mut Model, update: Update) {
    ui(model, update);
    let settings = &mut model.settings;

    let mut buf = Vec::with_capacity(1024);
    while !model.consumer.is_empty() {
        let recorded_sample = model.consumer.pop().unwrap_or(0.0);

        buf.push(recorded_sample);
        if buf.len() == 1024 {
            model.current_level = buf
                .iter()
                .filter_map(|x| NotNan::new(x.abs()).ok())
                .max()
                .unwrap()
                .into();

            const SAMPLE_RATE: usize = 44100;
            const SIZE: usize = 1024;
            const PADDING: usize = SIZE / 2;

            let mut detector = McLeodDetector::new(SIZE, PADDING);

            if let Some(pitch) = detector.get_pitch(
                &buf,
                SAMPLE_RATE,
                settings.power_threshold,
                settings.clarity_threshold,
            ) {
                model.is_running = true;
                println!("pitch: {}, clarity: {}", pitch.frequency, pitch.clarity);
                let frequency = pitch.frequency;
                let midi = freq_to_midi(frequency);
                let note_index = (midi as i32) - 60;
                if let Some(_) = model.note_positions.get(note_index as usize) {
                    model.last_frequency = frequency;
                }
                model.current_note = midi_to_tab(midi, settings.key, &model.tuning_notes);
            }

            // if model.is_running {
            //     model.locations.push_overwrite(new_pos);
            // }

            buf.clear();
        }
    }
}

fn ui(model: &mut Model, update: Update) {
    let egui = &mut model.egui;
    let settings = &mut model.settings;

    egui.set_elapsed_time(update.since_start);
    let ctx = egui.begin_frame();

    if model.ui_visible {
        egui::Window::new("Settings").show(&ctx, |ui| {
            ui.label("Power threshold:");
            ui.add(egui::Slider::new(&mut settings.power_threshold, 0.0..=5.0));

            ui.label("Clarity threshold:");
            ui.add(egui::Slider::new(
                &mut settings.clarity_threshold,
                0.0..=1.0,
            ));

            let keys = [
                "C", "G", "D", "A", "E", "B", "F#", "Db", "Ab", "Eb", "Bb", "F", "LF", "LC", "LD",
                "HG",
            ];
            egui::ComboBox::from_label("Key")
                .selected_text(settings.key)
                .show_ui(ui, |ui| {
                    for key in keys.iter() {
                        if ui.selectable_value(&mut settings.key, key, *key).changed() {
                            // TODO
                        }
                    }
                });

            let tunings = [
                "richter",
                "country",
                "wilde tuning",
                "wilde minor tuning",
                "melody maker",
                "natural minor",
                "harmonic minor",
                "paddy richter",
                "pentaharp",
                "powerdraw",
                "powerbender",
                "diminished",
                "easy 3rd",
            ];
            egui::ComboBox::from_label("Tuning")
                .selected_text(settings.tuning)
                .width(150.0)
                .show_ui(ui, |ui| {
                    for &tuning in tunings.iter() {
                        if ui
                            .selectable_value(&mut settings.tuning, tuning, tuning)
                            .changed()
                        {
                            let tuning_notes = harptabber::tuning_to_notes_in_order(tuning).0;
                            model.note_positions = calc_note_positions(&tuning_notes);
                            model.tuning_notes = tuning_notes;
                            // TODO change image
                        }
                    }
                });

            ui.label("F1 to hide");
        });
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    if app.elapsed_frames() == 1 {
        draw.background().color(BLACK);
    }
    draw.texture(&model.texture);

    let midi = freq_to_midi(model.last_frequency);
    let midi_f = freq_to_midi_float(model.last_frequency);
    let note_index = (midi as i32) - 60;
    if let Some(pos) = model.note_positions.get(note_index as usize) {
        if model.is_running {
            if model.current_level > 0.05 {
                let fac = (model.current_level * 10.0).min(2.0);
                draw.ellipse()
                    .x(pos.x)
                    .y(pos.y)
                    .wh(Vec2::new(fac * 10.0, fac * 10.0));
            }
        }
    }

    // soft clear screen
    draw.rect()
        .w_h(2000.0, 2000.0)
        .color(srgba(0.0, 0.0, 0.0, 0.15));

    let text_pos = Vec2::ZERO;
    if model.is_running {
        draw.text(&model.current_note)
            .x(text_pos.x)
            .y(-110.0)
            .font_size(32);
    }

    draw.to_frame(app, &frame).unwrap();
    model.egui.draw_to_frame(&frame).unwrap();
}

struct InputModel {
    pub producer: ringbuf::HeapProducer<f32>,
}

fn pass_in(model: &mut InputModel, buffer: &Buffer) {
    for sample in buffer.frames().map(|f| f[0]) {
        model.producer.push(sample).ok();
    }
}

fn freq_to_midi(freq: f32) -> u8 {
    (12.0 * (freq / 440.0).log2() + 69.0).round() as u8
}

fn freq_to_midi_float(freq: f32) -> f32 {
    12.0 * (freq / 440.0).log2() + 69.0
}

fn get_harmonica_key_semitone_offset(key: &str) -> i8 {
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

fn midi_to_tab(midi: u8, key: &str, notes_in_order: &[String]) -> String {
    let offset = get_harmonica_key_semitone_offset(key);
    let index: isize = midi as isize - 60 - offset as isize;
    if index < 0 || index > notes_in_order.len() as isize - 1 {
        return "".to_owned();
    }
    notes_in_order[index as usize].to_owned()
}

fn raw_window_event(_app: &App, model: &mut Model, event: &nannou::winit::event::WindowEvent) {
    // Let egui handle things like keyboard and mouse input.
    model.egui.handle_raw_event(event);
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    if key == Key::F1 {
        model.ui_visible = !model.ui_visible;
    }
}
