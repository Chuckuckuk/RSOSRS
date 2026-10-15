//! The NPC model picker: a window of its own (an OS window, not a panel inside the editor), opened
//! by clicking an NPC palette row with several models. It shows each model as a thumbnail, five to
//! a row; clicking one chooses it for placing and closes the window.

use crate::catalog::Catalog;
use crate::editor::{Brush, Editor, Tool};
use crate::thumbs::{NpcThumbs, SIZE};
use crate::ui::UiState;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::window::{WindowRef, WindowResolution};
use bevy_egui::{egui, EguiContext, EguiSchedule};

/// Thumbnails per row.
const COLUMNS: usize = 5;

/// Where the picker window's interface is drawn.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PickerContextPass;

/// The picker window's camera, which carries its egui context.
#[derive(Component)]
pub struct PickerCamera;

/// The open picker window: its window and camera entities, and the palette row it shows.
#[derive(Resource, Default)]
pub struct PickerWindow {
    open: Option<(Entity, Entity, usize)>,
    /// Frames drawn since the window opened or changed row: the grid is held at its top for the
    /// first few, while the new window settles to its size (it otherwise starts part-scrolled).
    frames_shown: u32,
}

impl PickerWindow {
    /// The picker's window entity, while it is open.
    pub fn window(&self) -> Option<Entity> {
        self.open.map(|(window, _, _)| window)
    }
}

/// Opens, retitles and closes the picker window to follow `UiState::npc_picker`, and notices the
/// window being closed by hand.
pub fn manage_picker_window(
    mut commands: Commands,
    mut state: ResMut<UiState>,
    mut picker: ResMut<PickerWindow>,
    mut thumbs: ResMut<NpcThumbs>,
    catalog: Res<Catalog>,
    mut windows: Query<&mut Window>,
) {
    // closed with its own close button: the window entity is gone
    if let Some((window, _, _)) = picker.open {
        if windows.get(window).is_err() {
            close(&mut commands, &mut picker, &mut thumbs);
            state.npc_picker = None;
        }
    }
    let title = |g: usize| {
        let group = &catalog.npc_groups[g];
        let level = if group.combat_level > 0 { format!(" (level {})", group.combat_level) } else { String::new() };
        format!("{}{level}: pick a model", group.name)
    };
    match (state.npc_picker, picker.open) {
        (Some(g), None) => {
            let rows = catalog.npc_groups[g].members.len().div_ceil(COLUMNS).clamp(1, 4) as u32;
            let width = (COLUMNS * (SIZE + 24)) as u32 + 40;
            let height = rows * (SIZE as u32 + 44) + 70;
            let window = commands
                .spawn(Window { title: title(g), resolution: WindowResolution::new(width, height), ..default() })
                .id();
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera { clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.1)), ..default() },
                    RenderTarget::Window(WindowRef::Entity(window)),
                    // nothing of the world is drawn here, only the interface
                    RenderLayers::none(),
                    EguiSchedule::new(PickerContextPass),
                    PickerCamera,
                ))
                .id();
            picker.open = Some((window, camera, g));
            picker.frames_shown = 0;
        }
        (Some(g), Some((window, camera, shown))) if g != shown => {
            if let Ok(mut w) = windows.get_mut(window) {
                w.title = title(g);
            }
            picker.open = Some((window, camera, g));
            picker.frames_shown = 0;
        }
        (None, Some(_)) => close(&mut commands, &mut picker, &mut thumbs),
        _ => {}
    }
}

fn close(commands: &mut Commands, picker: &mut PickerWindow, thumbs: &mut NpcThumbs) {
    if let Some((window, camera, _)) = picker.open.take() {
        commands.entity(camera).try_despawn();
        commands.entity(window).try_despawn();
    }
    thumbs.forget_textures();
}

/// The picker window's contents: the models of the chosen row, five to a row.
pub fn picker_ui(
    mut contexts: Query<&mut EguiContext, With<PickerCamera>>,
    mut editor: ResMut<Editor>,
    catalog: Res<Catalog>,
    mut state: ResMut<UiState>,
    mut thumbs: ResMut<NpcThumbs>,
    mut picker: ResMut<PickerWindow>,
) {
    let Some(g) = state.npc_picker else { return };
    let Ok(mut context) = contexts.single_mut() else { return };
    let settling = picker.frames_shown < 10;
    picker.frames_shown += 1;
    let ctx = context.get_mut().clone();
    let mut root = egui::Ui::new(ctx.clone(), "picker_root".into(), egui::UiBuilder::new().max_rect(ctx.viewport_rect()));
    let mut picked = None;
    egui::Frame::central_panel(&ctx.global_style()).show(&mut root, |ui| {
        ui.label("\u{2605} = fights with the stats in data/npcs.json; the others stand and cannot be fought.");
        ui.separator();
        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if settling {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show(ui, |ui| {
            egui::Grid::new("npc_models").spacing([12.0, 10.0]).show(ui, |ui| {
                for (i, n) in catalog.group_members(g).enumerate() {
                    let def = n.cache_id.and_then(|id| catalog.npc_defs.get(&id));
                    ui.vertical(|ui| {
                        let size = egui::vec2(SIZE as f32, SIZE as f32);
                        let texture = def.and_then(|d| thumbs.get(&ctx, d, state.frame));
                        let response = match &texture {
                            Some(tex) => ui.add(egui::Image::new(tex).fit_to_exact_size(size).sense(egui::Sense::click())),
                            None => {
                                let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                                let text = if def.is_some() { "\u{2026}" } else { "no model" };
                                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(14.0), egui::Color32::GRAY);
                                response
                            }
                        };
                        if editor.brush == Some(Brush::Npc(n.key.clone())) {
                            ui.painter().rect_stroke(response.rect, 2.0, egui::Stroke::new(2.0, egui::Color32::YELLOW), egui::StrokeKind::Outside);
                        }
                        let id = n.cache_id.map_or_else(|| "curated".to_string(), |i| format!("#{i}"));
                        ui.label(format!("{id}{}", if n.has_stats { " \u{2605}" } else { "" }));
                        let response = match def {
                            Some(d) => {
                                let options: Vec<&str> = d.actions.iter().map(String::as_str).filter(|a| !a.is_empty()).collect();
                                let options = if options.is_empty() { "none".to_string() } else { options.join(", ") };
                                response.on_hover_text(format!("size {}  options: {options}", d.size.max(1)))
                            }
                            None => response,
                        };
                        if response.clicked() {
                            picked = Some(n.key.clone());
                        }
                    });
                    if (i + 1) % COLUMNS == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    });
    if let Some(key) = picked {
        editor.brush = Some(Brush::Npc(key));
        editor.tool = Tool::Place;
        state.npc_picker = None;
    }
}
