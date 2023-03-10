use nannou::color::{ConvertFrom, LinSrgb, Mix};
use nannou::prelude::*;
use nannou_audio as audio;
use nannou_audio::Buffer;
use nannou_egui::{self, egui, Egui};
use ordered_float::NotNan;
use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;
use ringbuf::{Consumer, Producer, RingBuffer};

const LINE_LENGTH: usize = 4096;

struct Model {
    locations: Vec<Vec3>,
    camera_pos: Vec3,
    _in_stream: audio::Stream<InputModel>,
    consumer: Consumer<f32>,
    tuning_notes: Vec<String>,
    current_note: String,
    current_level: f32,
    ui_visible: bool,
    egui: Egui,
    settings: Settings,
    is_running: bool,

    line_bounds: [f32; 2],
    midi_bounds: MidiBounds,
}

struct Settings {
    power_threshold: f32,
    clarity_threshold: f32,
    key: &'static str,
    tuning: &'static str,
    left_color: LinSrgb,
    right_color: LinSrgb,
    should_calc_bounds_from_key: bool,
}

struct MidiBounds {
    low: u8,
    high: u8,
}

impl Default for MidiBounds {
    fn default() -> Self {
        Self { low: 48, high: 103 }
    }
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
        .size(1920, 1080)
        .build()
        .unwrap();

    let window = app.window(window_id).unwrap();
    let egui = Egui::from_window(&window);

    // Initialise the audio host so we can spawn an audio stream.
    let audio_host = audio::Host::new();

    // Create a ring buffer and split it into producer and consumer
    let latency_samples = 8192;
    let ring_buffer = RingBuffer::<f32>::new(latency_samples * 2); // Add some latency
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

    Model {
        locations: Vec::with_capacity(LINE_LENGTH),
        camera_pos: Vec3::ZERO,
        _in_stream: in_stream,
        consumer: cons,
        tuning_notes: harptabber::tuning_to_notes_in_order("richter").0,
        current_note: "4".to_owned(),
        current_level: 0.0,
        ui_visible: true,
        egui,
        is_running: false,
        line_bounds: [-8.0, 8.0],
        midi_bounds: calc_freq_bounds("C"),
        settings: Settings {
            power_threshold: 3.0,
            clarity_threshold: 0.7,
            key: "C",
            tuning: "richter",
            left_color: lin_srgb(0.0, 0.1, 0.8),
            right_color: lin_srgb(1.0, 0.1, 0.8),
            should_calc_bounds_from_key: true,
        },
    }
}

fn update(_app: &App, model: &mut Model, update: Update) {
    ui(model, update);
    let settings = &mut model.settings;

    let mut new_pos = if let Some(pos) = model.locations.last() {
        *pos
    } else {
        Vec3::ZERO
    };

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
                new_pos.x = map_range(
                    freq_to_midi_float(frequency),
                    model.midi_bounds.low as f32,
                    model.midi_bounds.high as f32,
                    model.line_bounds[0],
                    model.line_bounds[1],
                );
                model.current_note = midi_to_tab(midi, settings.key, &model.tuning_notes);
            }
            new_pos.y -= 0.1;
            new_pos.z += 0.3;

            if model.locations.len() == model.locations.capacity() {
                model.locations.rotate_left(1);
                model.locations.pop();
            }
            if model.is_running {
                model.locations.push(new_pos);
            }

            buf.clear();
        }
    }

    let mut direction = new_pos - model.camera_pos;
    direction.x = 0.0;
    model.camera_pos += direction;
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
                        if ui.selectable_value(&mut settings.key, key, key).changed() {
                            if settings.should_calc_bounds_from_key {
                                model.midi_bounds = calc_freq_bounds(settings.key);
                            }
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
                    for tuning in tunings.iter() {
                        if ui
                            .selectable_value(&mut settings.tuning, tuning, tuning)
                            .changed()
                        {
                            model.tuning_notes = harptabber::tuning_to_notes_in_order(tuning).0;
                        }
                    }
                });

            ui.horizontal(|ui| {
                edit_hsv(ui, &mut settings.left_color);
                ui.label("Left color");
            });
            ui.horizontal(|ui| {
                edit_hsv(ui, &mut settings.right_color);
                ui.label("Right color");
            });

            if ui
                .checkbox(
                    &mut settings.should_calc_bounds_from_key,
                    "calculate bounds from key",
                )
                .changed()
            {
                if settings.should_calc_bounds_from_key {
                    model.midi_bounds = calc_freq_bounds(settings.key);
                } else {
                    model.midi_bounds = MidiBounds::default();
                }
            }

            if ui.button("reset").clicked() {
                model.locations.clear();
                model.is_running = false;
            }

            ui.label("F1 to hide");
        });
    }
}

fn edit_hsv(ui: &mut egui::Ui, color: &mut LinSrgb) {
    let hsv_color: Hsv = Hsv::convert_from(*color);
    let mut egui_hsv = egui::color::Hsva::new(
        hsv_color.hue.to_positive_radians() / (std::f32::consts::PI * 2.0),
        hsv_color.saturation,
        hsv_color.value,
        1.0,
    );

    if egui::color_picker::color_edit_button_hsva(
        ui,
        &mut egui_hsv,
        egui::color_picker::Alpha::Opaque,
    )
    .changed()
    {
        let hsv = nannou::color::hsv(egui_hsv.h, egui_hsv.s, egui_hsv.v);
        *color = LinSrgb::convert_from(hsv);
    }
}

fn to_screen_position(point: &Vec3) -> Vec2 {
    let z = point.z - 10.0;
    // z is always negative
    let x = point.x / (0.01 * -z);
    let y = point.y / (0.01 * -z);
    Vec2::new(10.0 * x, 10.0 * y)
}

fn from_camera_view(point: Vec3, model: &Model) -> Vec2 {
    let point = point - model.camera_pos;
    to_screen_position(&point)
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    if app.elapsed_frames() == 1 {
        draw.background().color(BLACK);
    }

    let left_color = model.settings.left_color;
    let right_color = model.settings.right_color;

    let points_iter = model.locations.iter().map(|point| {
        let screen_pos = from_camera_view(*point, model);
        let mix_factor = map_range(point.x, -8.0, 8.0, 0.0, 1.0);
        let color = left_color.mix(&right_color, mix_factor);
        (screen_pos, color)
    });

    draw.polyline()
        .weight(10.0 * model.current_level + 1.0)
        .points_colored(points_iter);

    // soft clear screen
    draw.rect()
        .w_h(2000.0, 2000.0)
        .color(srgba(0.0, 0.0, 0.0, 0.15));

    let text_pos = from_camera_view(*model.locations.last().unwrap_or(&Vec3::ZERO), model);
    if model.is_running {
        draw.text(&model.current_note).x(text_pos.x).font_size(32);
    }

    draw.to_frame(app, &frame).unwrap();
    model.egui.draw_to_frame(&frame).unwrap();
}

struct InputModel {
    pub producer: Producer<f32>,
}

fn pass_in(model: &mut InputModel, buffer: &Buffer) {
    for sample in buffer.frames().map(|f| f[0]) {
        model.producer.push(sample).ok();
    }
}

fn freq_to_midi(freq: f32) -> u8 {
    (12.0 * (freq / 440.0).log2() + 69.0).round() as u8
}

fn calc_freq_bounds(key: &str) -> MidiBounds {
    const C4_MIDI: i8 = 60;
    const C7_MIDI: i8 = 96;
    let offset = get_harmonica_key_semitone_offset(key);
    MidiBounds {
        low: (C4_MIDI + offset) as u8,
        high: (C7_MIDI + offset) as u8,
    }
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
