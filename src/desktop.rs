use crate::{Document, Editor};
use eframe::egui::{self, Color32, ColorImage, Pos2, Sense, TextureOptions, Vec2};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::path::Path;

pub fn run() -> eframe::Result {
    let mut app = App::new();
    eframe::run_ui_native(
        "RustedPaint",
        eframe::NativeOptions::default(),
        move |ui, _| app.ui(ui),
    )
}

struct App {
    editor: Editor,
    texture: Option<egui::TextureHandle>,
    zoom: f32,
    drag: Option<Pos2>,
    brush_last: Option<(u32, u32)>,
    color: [u8; 4],
    brush_size: u32,
    bucket_tolerance: u8,
    path: String,
    status: String,
    new_width: u32,
    new_height: u32,
    tool: Tool,
    rename: String,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
    Select,
    Fill,
    Move,
}
impl App {
    fn brush_color(&self) -> crate::Rgba8 {
        if self.tool == Tool::Eraser {
            crate::Rgba8::TRANSPARENT
        } else {
            crate::Rgba8::new(self.color[0], self.color[1], self.color[2], self.color[3])
        }
    }
    fn new() -> Self {
        Self {
            editor: Editor::new(Document::new(800, 600).unwrap()),
            texture: None,
            zoom: 1.0,
            drag: None,
            brush_last: None,
            color: [0, 0, 0, 255],
            brush_size: 1,
            bucket_tolerance: 0,
            path: String::new(),
            status: String::new(),
            new_width: 800,
            new_height: 600,
            tool: Tool::Select,
            rename: String::new(),
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("RustedPaint");
            ui.add(egui::Slider::new(&mut self.zoom, 0.25..=4.0).text("Zoom"));
            if ui.button("100%").clicked() {
                self.zoom = 1.0;
            }
            if ui.button("Undo").clicked() {
                self.editor.undo();
            }
            if ui.button("Redo").clicked() {
                self.editor.redo();
            }
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Pencil, "Pencil");
            ui.selectable_value(&mut self.tool, Tool::Eraser, "Eraser");
            ui.selectable_value(&mut self.tool, Tool::Select, "Select");
            ui.selectable_value(&mut self.tool, Tool::Fill, "Bucket");
            ui.selectable_value(&mut self.tool, Tool::Move, "Move");
            if matches!(self.tool, Tool::Pencil | Tool::Eraser) {
                ui.add(egui::Slider::new(&mut self.brush_size, 1..=64).text("Size"));
            }
            if self.tool == Tool::Fill {
                ui.add(egui::Slider::new(&mut self.bucket_tolerance, 0..=255).text("Tolerance"));
            }
            if self.editor.has_move_preview() {
                if ui.button("Finish Move").clicked() {
                    let _ = self.editor.finish_move_pixels();
                }
                if ui.button("Cancel Move").clicked() {
                    self.editor.cancel_move_pixels();
                }
            }
            if ui.button("Select All").clicked() {
                self.editor.select_rectangle(
                    (0, 0),
                    (
                        self.editor.document().width() - 1,
                        self.editor.document().height() - 1,
                    ),
                );
            }
            if ui.button("Deselect").clicked() {
                self.editor.set_selection(None);
            }
            if ui.button("Copy").clicked() {
                let _ = self.editor.copy_selection();
            }
            if ui.button("Cut").clicked() {
                let _ = self.editor.cut_selection();
            }
            if ui.button("Paste").clicked() {
                let _ = self.editor.paste_clipboard();
            }
        });
        ui.horizontal(|ui| {
            ui.label("File");
            ui.text_edit_singleline(&mut self.path);
            if ui.button("Open").clicked() {
                match open_path(&self.path) {
                    Ok(document) => {
                        self.editor.replace_document(document);
                        self.status = "Opened".into();
                    }
                    Err(error) => self.status = error,
                }
            }
            if ui.button("Export PNG").clicked() {
                self.status = match export_png(&self.editor, &self.path) {
                    Ok(()) => "Exported".into(),
                    Err(error) => error,
                };
            }
            ui.label(&self.status);
        });
        ui.horizontal(|ui| {
            ui.label("New canvas");
            ui.add(egui::DragValue::new(&mut self.new_width).range(1..=16_384));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut self.new_height).range(1..=16_384));
            if ui.button("Create").clicked() {
                match Document::new(self.new_width, self.new_height) {
                    Ok(document) => {
                        self.editor.replace_document(document);
                        self.status =
                            format!("Created {} × {} canvas", self.new_width, self.new_height);
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("New Layer").clicked() {
                self.editor.add_layer("Layer");
            }
            if ui.button("Delete Layer").clicked() {
                let _ = self.editor.delete_active_layer();
            }
            if ui.button("Duplicate").clicked() {
                let _ = self.editor.duplicate_active_layer();
            }
            if ui.button("Merge Down").clicked() {
                let _ = self.editor.merge_active_layer_down();
            }
            if ui.button("Crop to Selection").clicked() {
                let _ = self.editor.crop_to_selection();
            }
            if ui.button("Deselect").clicked() {
                self.editor.set_selection(None);
            }
        });
        ui.horizontal(|ui| {
            ui.label("RGBA");
            for channel in &mut self.color {
                ui.add(egui::DragValue::new(channel).range(0..=255));
            }
            let preview = Color32::from_rgba_unmultiplied(
                self.color[0],
                self.color[1],
                self.color[2],
                self.color[3],
            );
            let (preview_rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            ui.painter().rect_filled(preview_rect, 3.0, preview);
            if ui.button("Fill Selection").clicked() {
                let _ = self.editor.fill_selection(crate::Rgba8::new(
                    self.color[0],
                    self.color[1],
                    self.color[2],
                    self.color[3],
                ));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Swatches");
            for (name, color) in [
                ("Black", [0, 0, 0, 255]),
                ("White", [255, 255, 255, 255]),
                ("Red", [220, 40, 40, 255]),
                ("Green", [45, 160, 75, 255]),
                ("Blue", [45, 100, 220, 255]),
                ("Yellow", [245, 210, 45, 255]),
                ("Clear", [0, 0, 0, 0]),
            ] {
                if ui
                    .add(
                        egui::Button::new(name).fill(Color32::from_rgba_unmultiplied(
                            color[0], color[1], color[2], color[3],
                        )),
                    )
                    .clicked()
                {
                    self.color = color;
                }
            }
        });
        let rgba: Vec<u8> = self
            .editor
            .preview_composite()
            .into_iter()
            .flat_map(|p| [p.red, p.green, p.blue, p.alpha])
            .collect();
        let image = ColorImage::from_rgba_unmultiplied(
            [
                self.editor.document().width() as usize,
                self.editor.document().height() as usize,
            ],
            &rgba,
        );
        let texture = self.texture.get_or_insert_with(|| {
            ui.ctx()
                .load_texture("canvas", image.clone(), TextureOptions::NEAREST)
        });
        texture.set(image, TextureOptions::NEAREST);
        let size = Vec2::new(
            self.editor.document().width() as f32 * self.zoom,
            self.editor.document().height() as f32 * self.zoom,
        );
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, Color32::DARK_GRAY);
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        let to_point = |v: Pos2| {
            (
                ((v.x - rect.left()) / self.zoom).max(0.0) as u32,
                ((v.y - rect.top()) / self.zoom).max(0.0) as u32,
            )
        };
        if response.drag_started() {
            self.drag = response.interact_pointer_pos();
            if matches!(self.tool, Tool::Pencil | Tool::Eraser) {
                if let Some(position) = self.drag {
                    if self.editor.begin_brush_stroke() {
                        self.brush_last = Some(to_point(position));
                    }
                }
            }
            if self.tool == Tool::Move {
                let _ = self.editor.begin_move_pixels();
            }
        }
        if response.dragged() && matches!(self.tool, Tool::Pencil | Tool::Eraser) {
            if let Some(position) = response.interact_pointer_pos() {
                let next = to_point(position);
                if let Some(last) = self.brush_last.replace(next) {
                    let _ = self.editor.draw_brush_segment(
                        last,
                        next,
                        self.brush_color(),
                        self.brush_size,
                    );
                }
            }
        }
        if response.dragged() && self.tool == Tool::Move {
            if let (Some(start), Some(current)) = (self.drag, response.interact_pointer_pos()) {
                self.editor.update_move_pixels((
                    ((current.x - start.x) / self.zoom).round() as i32,
                    ((current.y - start.y) / self.zoom).round() as i32,
                ));
            }
        }
        if response.drag_stopped() {
            if let (Some(a), Some(b)) = (self.drag.take(), response.interact_pointer_pos()) {
                match self.tool {
                    Tool::Pencil | Tool::Eraser => {
                        let end = to_point(b);
                        if let Some(last) = self.brush_last.take() {
                            let _ = self.editor.draw_brush_segment(
                                last,
                                end,
                                self.brush_color(),
                                self.brush_size,
                            );
                        }
                        self.editor.finish_brush_stroke();
                    }
                    Tool::Select => {
                        self.editor.select_rectangle(to_point(a), to_point(b));
                    }
                    Tool::Fill => {
                        let _ = self.editor.bucket_fill_with_tolerance(
                            to_point(a),
                            crate::Rgba8::new(
                                self.color[0],
                                self.color[1],
                                self.color[2],
                                self.color[3],
                            ),
                            self.bucket_tolerance,
                        );
                    }
                    Tool::Move => {
                        self.editor.update_move_pixels((
                            ((b.x - a.x) / self.zoom).round() as i32,
                            ((b.y - a.y) / self.zoom).round() as i32,
                        ));
                    }
                }
            }
        }
        if response.clicked() {
            if let Some(position) = response.interact_pointer_pos() {
                let point = (
                    ((position.x - rect.left()) / self.zoom).max(0.0) as u32,
                    ((position.y - rect.top()) / self.zoom).max(0.0) as u32,
                );
                let color =
                    crate::Rgba8::new(self.color[0], self.color[1], self.color[2], self.color[3]);
                match self.tool {
                    Tool::Pencil | Tool::Eraser => {
                        let _ = self
                            .editor
                            .draw_brush_line(point, point, color, self.brush_size);
                    }
                    Tool::Fill => {
                        let _ = self.editor.bucket_fill_with_tolerance(
                            point,
                            color,
                            self.bucket_tolerance,
                        );
                    }
                    Tool::Select | Tool::Move => {}
                }
            }
        }
        if let Some(selection) = self.editor.selection() {
            let selection_rect = egui::Rect::from_min_size(
                Pos2::new(
                    rect.left() + selection.x() as f32 * self.zoom,
                    rect.top() + selection.y() as f32 * self.zoom,
                ),
                Vec2::new(
                    selection.width() as f32 * self.zoom,
                    selection.height() as f32 * self.zoom,
                ),
            );
            ui.painter().rect_stroke(
                selection_rect,
                0.0,
                egui::Stroke::new(1.0, Color32::YELLOW),
                egui::StrokeKind::Middle,
            );
        }
        ui.separator();
        ui.label("Layers");
        let mut chosen = None;
        let mut visibility = None;
        for layer in self.editor.document().layers() {
            ui.horizontal(|ui| {
                let mut visible = layer.is_visible();
                if ui.checkbox(&mut visible, "").changed() {
                    visibility = Some((layer.id(), visible));
                }
                if ui
                    .selectable_label(layer.id() == self.editor.active_layer(), layer.name())
                    .clicked()
                {
                    chosen = Some(layer.id());
                }
            });
        }
        if let Some(layer) = chosen {
            self.editor.set_active_layer(layer);
        }
        if let Some((layer, visible)) = visibility {
            let _ = self.editor.set_layer_visible(layer, visible);
        }
        let active = self.editor.active_layer();
        let mut opacity = self
            .editor
            .document()
            .layers()
            .iter()
            .find(|layer| layer.id() == active)
            .map(|layer| layer.opacity())
            .unwrap_or(255);
        if ui
            .add(egui::Slider::new(&mut opacity, 0..=255).text("Opacity"))
            .changed()
        {
            let _ = self.editor.set_layer_opacity(active, opacity);
        }
        let current = self
            .editor
            .document()
            .layers()
            .iter()
            .find(|layer| layer.id() == active)
            .map(|layer| layer.blend_mode())
            .unwrap_or(crate::BlendMode::Normal);
        let mut mode = current;
        egui::ComboBox::from_label("Blend")
            .selected_text(format!("{mode:?}"))
            .show_ui(ui, |ui| {
                for candidate in [
                    crate::BlendMode::Normal,
                    crate::BlendMode::Multiply,
                    crate::BlendMode::Additive,
                    crate::BlendMode::ColorBurn,
                    crate::BlendMode::ColorDodge,
                    crate::BlendMode::Reflect,
                    crate::BlendMode::Glow,
                    crate::BlendMode::Screen,
                    crate::BlendMode::Overlay,
                    crate::BlendMode::Difference,
                    crate::BlendMode::Negation,
                    crate::BlendMode::Lighten,
                    crate::BlendMode::Darken,
                    crate::BlendMode::Xor,
                ] {
                    ui.selectable_value(&mut mode, candidate, format!("{candidate:?}"));
                }
            });
        if mode != current {
            let _ = self.editor.set_layer_blend_mode(active, mode);
        }
        ui.horizontal(|ui| {
            let layer_index = self
                .editor
                .document()
                .layers()
                .iter()
                .position(|layer| layer.id() == active)
                .unwrap_or(0);
            if ui.button("Lower").clicked() && layer_index > 0 {
                let _ = self.editor.move_active_layer(layer_index - 1);
            }
            if ui.button("Raise").clicked()
                && layer_index + 1 < self.editor.document().layers().len()
            {
                let _ = self.editor.move_active_layer(layer_index + 1);
            }
            ui.text_edit_singleline(&mut self.rename);
            if ui.button("Rename Active").clicked() && !self.rename.is_empty() {
                let _ = self.editor.rename_active_layer(self.rename.clone());
            }
        });
        let shortcuts = ui.ctx().input(|input| {
            (
                input.modifiers.command,
                input.key_pressed(egui::Key::Z),
                input.key_pressed(egui::Key::Y),
                input.key_pressed(egui::Key::S),
                input.key_pressed(egui::Key::P),
                input.key_pressed(egui::Key::F),
                input.key_pressed(egui::Key::M),
                input.key_pressed(egui::Key::A),
                input.key_pressed(egui::Key::D),
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Escape),
                input.key_pressed(egui::Key::Num0),
                input.key_pressed(egui::Key::E),
                input.key_pressed(egui::Key::C),
                input.key_pressed(egui::Key::X),
                input.key_pressed(egui::Key::V),
            )
        });
        if shortcuts.0 && shortcuts.1 {
            self.editor.undo();
        } else if shortcuts.0 && shortcuts.2 {
            self.editor.redo();
        } else if shortcuts.0 && shortcuts.7 {
            self.editor.select_rectangle(
                (0, 0),
                (
                    self.editor.document().width() - 1,
                    self.editor.document().height() - 1,
                ),
            );
        } else if shortcuts.0 && shortcuts.8 {
            self.editor.set_selection(None);
        } else if shortcuts.0 && shortcuts.13 {
            let _ = self.editor.copy_selection();
        } else if shortcuts.0 && shortcuts.14 {
            let _ = self.editor.cut_selection();
        } else if shortcuts.0 && shortcuts.15 {
            let _ = self.editor.paste_clipboard();
        } else if shortcuts.9 {
            if !self.editor.finish_brush_stroke() {
                let _ = self.editor.finish_move_pixels();
            }
        } else if shortcuts.10 {
            if !self.editor.cancel_brush_stroke() {
                self.editor.cancel_move_pixels();
            }
        } else if shortcuts.11 {
            self.zoom = 1.0;
        } else if shortcuts.3 {
            self.tool = Tool::Select;
        } else if shortcuts.4 {
            self.tool = Tool::Pencil;
        } else if shortcuts.5 {
            self.tool = Tool::Fill;
        } else if shortcuts.6 {
            self.tool = Tool::Move;
        } else if shortcuts.12 {
            self.tool = Tool::Eraser;
        }
    }
}

fn open_path(path: &str) -> Result<Document, String> {
    let input = BufReader::new(File::open(path).map_err(|error| error.to_string())?);
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("pdn") => {
            Document::read_pdn(input).map_err(|error| error.to_string())
        }
        Some(extension) if extension.eq_ignore_ascii_case("png") => {
            Document::read_png(input).map_err(|error| error.to_string())
        }
        _ => Err("Supported input formats: PNG and PDN".into()),
    }
}
fn export_png(editor: &Editor, path: &str) -> Result<(), String> {
    if !path.to_ascii_lowercase().ends_with(".png") {
        return Err("Export path must end in .png".into());
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    editor
        .document()
        .write_png(BufWriter::new(output))
        .map_err(|error| error.to_string())
}
