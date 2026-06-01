mod tab_utils;
use egui::{FontId, RichText};
use nannou::color::rgb_u32;
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use nannou_egui::{self, egui, Egui};
use ordered_float::NotNan;
use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;
use ringbuf::HeapRb;
use tab_utils::*;

struct Model {
    _in_stream: audio::Stream<InputModel>,
    consumer: ringbuf::HeapConsumer<f32>,
    tuning_notes_in_order: Vec<String>,
    tuning_note_layout: Vec<Vec<String>>,
    current_note: String,
    current_level: f32,
    ui_visible: bool,
    egui: Egui,
    settings: Settings,
    is_running: bool,

    note_positions: Vec<Vec2>,
    last_frequency: f32,
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
        .size(1280, 720)
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

    let tuning_notes = harptabber::tuning_to_notes_in_order("richter").0;
    let note_positions = calc_note_positions(&tuning_notes);

    Model {
        _in_stream: in_stream,
        consumer: cons,
        tuning_notes_in_order: tuning_notes,
        tuning_note_layout: harptabber::get_tabkeyboard_layout("richter"),
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
                let note_index =
                    (midi as i32) - 60 - get_harmonica_key_semitone_offset(settings.key) as i32;
                if model.note_positions.get(note_index as usize).is_some() {
                    model.last_frequency = frequency;
                }
                model.current_note = midi_to_tab(midi, settings.key, &model.tuning_notes_in_order);
            }

            buf.clear();
        }
    }
}

fn harmonica_settings(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    note_positions: &mut Vec<Vec2>,
    tuning_notes_in_order: &mut Vec<String>,
    tuning_note_layout: &mut Vec<Vec<String>>,
) {
    ui.vertical(|ui| {
        ui.label(RichText::new("Harmonica settings").font(FontId::proportional(20.0)));
        let keys = [
            "C", "G", "D", "A", "E", "B", "F#", "Db", "Ab", "Eb", "Bb", "F", "LF", "LC", "LD", "HG",
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
                        *note_positions = calc_note_positions(&tuning_notes);
                        *tuning_notes_in_order = tuning_notes;
                        *tuning_note_layout = harptabber::get_tabkeyboard_layout(tuning);
                    }
                }
            });
    });
}

fn pitch_detection_settings(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.vertical(|ui| {
        ui.label(RichText::new("Pitch detection settings").font(FontId::proportional(20.0)));
        ui.label("Power threshold:");
        ui.add(egui::Slider::new(&mut settings.power_threshold, 0.0..=5.0));

        ui.label("Clarity threshold:");
        ui.add(egui::Slider::new(
            &mut settings.clarity_threshold,
            0.0..=1.0,
        ));
    });
}

fn ui(model: &mut Model, update: Update) {
    model.egui.set_elapsed_time(update.since_start);
    let ctx = model.egui.begin_frame();

    if model.ui_visible {
        egui::Window::new("Settings").show(&ctx, |ui| {
            ui.horizontal(|ui| {
                harmonica_settings(
                    ui,
                    &mut model.settings,
                    &mut model.note_positions,
                    &mut model.tuning_notes_in_order,
                    &mut model.tuning_note_layout,
                );
                ui.add_space(50.0);
                pitch_detection_settings(ui, &mut model.settings);
            });

            ui.label("F1 to hide");
        });
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let colors = [
        rgb_u32(0xfbbbad),
        rgb_u32(0xee8695),
        rgb_u32(0x4a7a96),
        rgb_u32(0x333f58),
        rgb_u32(0x292831),
    ];
    let bg_color = colors[4];
    let tuning_text_color = colors[2];
    let tab_color = colors[1];
    let selected_note_color = colors[1];
    let hole_color = colors[2];
    let hole_outline_color = colors[3];

    if app.elapsed_frames() == 1 {
        draw.background().color(bg_color);
    }

    draw.rect().w_h(2000.0, 2000.0).color(bg_color);
    if app.elapsed_frames() < 100 {
        draw.text("F1 to toggle settings")
            .x(-430.0)
            .y(300.0)
            .width(1000.0)
            .color(tab_color)
            .font_size(32);
    }

    if model.is_running {
        draw.text(&model.current_note)
            .x(0.0)
            .y(-250.0)
            .color(tab_color)
            .font_size(60);
    }

    draw.text(&format!(
        "{} {} harmonica",
        model.settings.key, model.settings.tuning
    ))
    .width(1000.0)
    .x(0.0)
    .y(250.0)
    .color(tuning_text_color)
    .font_size(48);

    for (mut i, row) in model.tuning_note_layout.iter().rev().enumerate() {
        if i > 3 {
            i += 1;
        }
        for (j, note) in row.iter().enumerate() {
            if note.is_empty() {
                continue;
            }
            let x = DRAW_X + j as f32 * HOLE_X_DIST;
            let y = (DRAW_Y - 3.0 * HOLE_Y_DIST) + i as f32 * HOLE_Y_DIST;
            draw.rect()
                .x(x)
                .y(y)
                .wh(Vec2::new(HOLE_X_DIST - 10.0, HOLE_Y_DIST - 10.0))
                .color(hole_color)
                .stroke(hole_outline_color)
                .stroke_weight(4.0);
            draw.text(note)
                .x(x - 8.0)
                .y(y + 4.0)
                .color(bg_color)
                .font_size(22);
        }
    }
    // draw.polyline()
    //     .points(model.note_positions.iter().copied())
    //     .color(RED);

    let midi = freq_to_midi(model.last_frequency);
    let note_index =
        (midi as i32) - 60 - get_harmonica_key_semitone_offset(model.settings.key) as i32;
    if let Some(pos) = model.note_positions.get(note_index as usize) {
        if model.is_running && model.current_level > 0.05 {
            let fac = (model.current_level * 10.0).min(2.0);
            draw.ellipse()
                .x(pos.x)
                .y(pos.y)
                .wh(Vec2::new(fac * 10.0, fac * 10.0))
                .color(selected_note_color);
        }
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

fn raw_window_event(_app: &App, model: &mut Model, event: &nannou::winit::event::WindowEvent) {
    // Let egui handle things like keyboard and mouse input.
    model.egui.handle_raw_event(event);
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    if key == Key::F1 {
        model.ui_visible = !model.ui_visible;
    }
}
