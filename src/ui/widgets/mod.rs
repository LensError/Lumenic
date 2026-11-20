mod containers;
mod controls;
mod data;
mod feedback;
mod inputs;
mod markdown;

pub use data::{TableColumn, TableState};
pub use feedback::{ToastCloseStyle, ToastCorner, ToastItem, ToastLevel, ToastQueue, ToastStyle};

use core::ops::RangeInclusive;

use crate::gfx::image::GifFrame;
use crate::gfx::viewport::{Canvas2D, Canvas2DCmd, Scene3D, SceneSnapshot};
use crate::os::App;
use crate::ui::context::{AxisKind, LayoutNode, Style, StyleBuilder, UiContext};
use crate::ui::draw::{OwnedSpan, Role, SpanKind};
use crate::ui::text_edit::TextEditState;
use crate::ui::types::{Align, Camera3D, Color, Key, Modifiers, Rect, TextureId, Theme};

pub(crate) fn checkbox_mark_cmds(w: f32, h: f32, color: Color) -> Vec<Canvas2DCmd> {
    let mut c = Canvas2D::new([0.0, 0.0]);
    let stroke = (w.min(h) * (2.0 / 18.0)).max(1.5);
    let p0 = [w * (4.0 / 18.0), h * (9.5 / 18.0)];
    let p1 = [w * (7.8 / 18.0), h * (13.0 / 18.0)];
    let p2 = [w * (14.2 / 18.0), h * (5.2 / 18.0)];
    c.line(p0, p1, stroke, color);
    c.line(p1, p2, stroke, color);
    c.cmds
}

/// Computes the visible `[start, end)` row range from cumulative heights via `O(log N)` binary search.
pub fn vlist_range_binary_search(prefix: &[f32], scroll_y: f32, viewport_h: f32) -> (usize, usize) {
    if prefix.len() <= 1 {
        return (0, 0);
    }
    let count = prefix.len() - 1;
    let top = scroll_y.max(0.0);
    let bottom = (top + viewport_h.max(0.0)).max(top);
    if bottom <= top {
        return (0, 0);
    }
    let start = prefix[1..].partition_point(|&end_y| end_y <= top);
    let end = prefix[..count].partition_point(|&start_y| start_y < bottom);
    (start.min(count), end.max(start).min(count))
}

fn emit_vlist_spacer(ui: &mut UiContext, label: &str, height: f32) {
    if height <= 0.0 {
        return;
    }
    let id = ui.ids.id(label);
    ui.alloc_node(
        LayoutNode {
            id,
            role: Role::Container,
            kind: AxisKind::Leaf,
            style: Style {
                height: Some(height),
                ..Style::default()
            },
            label: String::from(label),
            semantic_value: None,
            text: None,
            rich_spans: None,
            rich_links: Vec::new(),
            sparkline_samples: None,
            image_tex: None,
            viewport_data: None,
            is_scroll: false,
            scroll_offset_y: 0.0,
            focusable: false,
            hit_testable: false,
            enabled: false,
            is_overlay: false,
            children: Vec::new(),
            intrinsic_w: 0.0,
            intrinsic_h: height,
            content_w: 0.0,
            content_h: height,
            rect: Rect::ZERO,
        },
        true,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChevronDir {
    Up,
    Down,
    Right,
}

fn emit_chevron_icon(ui: &mut UiContext, id_label: &str, dir: ChevronDir, size: f32, color: Color) {
    let mut c = Canvas2D::new([0.0, 0.0]);
    let s = size.max(8.0);
    let mid = s * 0.5;
    let d = s * 0.24;
    match dir {
        ChevronDir::Down => {
            let p0 = [mid - d, mid - d * 0.45];
            let p1 = [mid, mid + d * 0.55];
            let p2 = [mid + d, mid - d * 0.45];
            c.line(p0, p1, 1.6, color);
            c.line(p1, p2, 1.6, color);
        }
        ChevronDir::Up => {
            let p0 = [mid - d, mid + d * 0.45];
            let p1 = [mid, mid - d * 0.55];
            let p2 = [mid + d, mid + d * 0.45];
            c.line(p0, p1, 1.6, color);
            c.line(p1, p2, 1.6, color);
        }
        ChevronDir::Right => {
            let p0 = [mid - d * 0.45, mid - d];
            let p1 = [mid + d * 0.55, mid];
            let p2 = [mid - d * 0.45, mid + d];
            c.line(p0, p1, 1.6, color);
            c.line(p1, p2, 1.6, color);
        }
    }
    let cid = ui.ids.id(id_label);
    ui.alloc_node(
        LayoutNode {
            id: cid,
            role: Role::Viewport,
            kind: AxisKind::Leaf,
            style: Style {
                width: Some(s),
                height: Some(s),
                ..Style::default()
            },
            label: String::new(),
            semantic_value: None,
            text: None,
            rich_spans: None,
            rich_links: Vec::new(),
            sparkline_samples: None,
            image_tex: None,
            viewport_data: Some((
                c.cmds,
                SceneSnapshot {
                    camera: Camera3D::default(),
                    draws: Vec::new(),
                    msaa_samples: 4,
                },
            )),
            is_scroll: false,
            scroll_offset_y: 0.0,
            focusable: false,
            hit_testable: false,
            enabled: true,
            is_overlay: false,
            children: Vec::new(),
            intrinsic_w: s,
            intrinsic_h: s,
            content_w: s,
            content_h: s,
            rect: Rect::ZERO,
        },
        true,
    );
}

#[cfg(test)]
mod tests {
    use crate::gfx::viewport::Canvas2DCmd;
    use crate::ui::context::UiContext;
    use crate::ui::draw::DrawCmd;
    use crate::ui::types::{Color, Key, Modifiers, PointerButton};

    #[test]
    fn toggle_slider_keybind_and_split_drag() {
        let mut ui = UiContext::new();
        let mut enabled = false;
        let mut volume = 0.5_f32;
        let mut bind = Key::Char('k');
        let mut split = 0.5_f32;

        let run = |ui: &mut UiContext, en: &mut bool, vol: &mut f32, kb: &mut Key, sp: &mut f32| {
            ui.begin_frame(500.0, 300.0, 1.0);
            ui.hsplit(
                "main_split",
                sp,
                |ui| {
                    ui.toggle("Audio", en);
                    ui.slider("Vol", vol, 0.0..=1.0);
                    ui.keybind("Jump", kb);
                },
                |ui| {
                    ui.sparkline(&[0.1, 0.5, 0.3, 0.9]).w(160.0).h(40.0);
                },
            )
            .w(500.0)
            .h(300.0);
            ui.end_frame();
        };

        run(&mut ui, &mut enabled, &mut volume, &mut bind, &mut split);
        assert!(ui
            .draw_list()
            .base_cmds
            .iter()
            .any(|c| matches!(c, DrawCmd::Sparkline { .. })));

        // Drag the split divider from x ~ 250 to x = 350 (ratio ~ 0.70).
        let div_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "HSplitDivider")
            .expect("divider present")
            .bounds;
        ui.on_pointer_move(div_bounds.x + 2.0, div_bounds.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_move(350.0, div_bounds.y + 10.0);
        run(&mut ui, &mut enabled, &mut volume, &mut bind, &mut split);
        ui.on_pointer_up(PointerButton::Left);
        assert!(
            split > 0.65 && split < 0.75,
            "split={split}, div={div_bounds:?}"
        );

        // Toggle via click
        let toggle_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Audio")
            .expect("toggle present")
            .bounds;
        ui.on_pointer_move(toggle_bounds.x + 10.0, toggle_bounds.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut enabled, &mut volume, &mut bind, &mut split);
        assert!(enabled);

        // Keybind capture
        let bind_btn_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "K")
            .expect("keybind button present")
            .bounds;
        ui.on_pointer_move(bind_btn_bounds.x + 5.0, bind_btn_bounds.y + 5.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut enabled, &mut volume, &mut bind, &mut split);
        ui.on_key_down(Key::Space, Modifiers::default());
        run(&mut ui, &mut enabled, &mut volume, &mut bind, &mut split);
        assert_eq!(bind, Key::Space);
    }

    #[test]
    fn dropdown_overlay_above_scroll_and_modal_escape() {
        let mut ui = UiContext::new();
        let mut selected = 0usize;
        let mut modal_open = true;

        let run = |ui: &mut UiContext, sel: &mut usize, open: &mut bool| {
            ui.begin_frame(400.0, 300.0, 1.0);
            ui.scroll("scroller", |ui| {
                ui.dropdown("theme_dd", sel, &["Dark", "Light"]);
                ui.btn("Below").w(120.0).h(40.0);
            })
            .h(120.0);
            ui.modal("confirm", open, |ui| {
                ui.txt("Confirm?");
            });
            ui.end_frame();
        };

        run(&mut ui, &mut selected, &mut modal_open);
        assert!(ui
            .draw_list()
            .base_cmds
            .iter()
            .any(|c| matches!(c, DrawCmd::PushClip(_))));
        assert!(!ui.draw_list().overlay_cmds.is_empty());

        // Escape dismisses modal.
        ui.on_key_down(Key::Escape, Modifiers::default());
        run(&mut ui, &mut selected, &mut modal_open);
        assert!(!modal_open);

        // Open dropdown inside scroll and select option 1 via overlay hit-testing.
        let dd_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Dark" && n.role == crate::Role::Button)
            .expect("dropdown trigger")
            .bounds;
        ui.on_pointer_move(dd_bounds.x + 10.0, dd_bounds.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut selected, &mut modal_open);

        let opt1 = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Light" && n.is_overlay)
            .expect("overlay option")
            .bounds;
        ui.on_pointer_move(opt1.x + 10.0, opt1.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut selected, &mut modal_open);
        assert_eq!(selected, 1);

        // Verify modal centers below csd_titlebar, constrains tall content, and allows StyleBuilder overrides.
        let mut tall_open = true;
        ui.begin_frame(680.0, 500.0, 1.0);
        let _ = ui.csd_titlebar("App");
        ui.modal("tall_modal", &mut tall_open, |ui| {
            for i in 0..16 {
                ui.btn(&format!("Field {i}")).h(36.0);
            }
        });
        ui.end_frame();
        let modal_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "tall_modal")
            .expect("tall_modal node")
            .bounds;
        assert!(modal_bounds.y >= 40.0, "modal_bounds={modal_bounds:?}");
        assert!(
            modal_bounds.y + modal_bounds.h <= 500.0,
            "modal overflowed viewport: {modal_bounds:?}"
        );
        assert!(
            (modal_bounds.x - (680.0 - modal_bounds.w) * 0.5).abs() <= 1.5,
            "modal not horizontally centered: {modal_bounds:?}"
        );

        ui.begin_frame(680.0, 500.0, 1.0);
        ui.modal("custom_modal", &mut tall_open, |ui| {
            ui.txt("Custom sized modal");
        })
        .w(520.0)
        .h(260.0)
        .pos(45.0, 60.0);
        ui.end_frame();
        let custom_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "custom_modal")
            .expect("custom_modal node")
            .bounds;
        assert_eq!(custom_bounds.x, 45.0);
        assert_eq!(custom_bounds.y, 60.0);
        assert_eq!(custom_bounds.w, 520.0);
        assert_eq!(custom_bounds.h, 260.0);
    }

    #[test]
    fn idle_physics_settles_and_clears_dirty() {
        let mut ui = UiContext::new();
        for _ in 0..200 {
            ui.step_physics(0.016);
            ui.begin_frame(400.0, 200.0, 1.0);
            let _ = ui.spring("val", 10.0, 280.0, 26.0);
            ui.end_frame();
        }
        assert!(!ui.has_active_animations());
        assert!(ui.take_dirty());
        assert!(!ui.take_dirty());
    }

    #[test]
    fn virtual_list_dynamic_100k_binary_search() {
        use super::vlist_range_binary_search;

        let count = 100_000usize;
        let mut prefix = Vec::with_capacity(count + 1);
        prefix.push(0.0_f32);
        let mut acc = 0.0_f32;
        for i in 0..count {
            acc += if i % 2 == 0 { 24.0 } else { 40.0 };
            prefix.push(acc);
        }
        let (s, e) = vlist_range_binary_search(&prefix, 150_000.0, 300.0);
        assert!(s > 4000 && e > s && (e - s) < 20, "range=({s}, {e})");

        let mut ui = UiContext::new();
        let mut invoked = 0usize;
        ui.begin_frame(400.0, 220.0, 1.0);
        ui.vlist_dynamic(
            "log_rows",
            count,
            28.0,
            |i| if i % 3 == 0 { 42.0 } else { 24.0 },
            |ui, i| {
                invoked += 1;
                ui.txt(&format!("Row {i}"));
            },
        )
        .h(220.0);
        ui.end_frame();
        assert!(invoked > 0 && invoked < 30, "invoked={invoked}");
    }

    #[test]
    fn input_and_textarea_editing_ime_undo() {
        use crate::ui::text_edit::TextEditState;

        let mut ui = UiContext::new();
        let mut text = TextEditState::with_text("hi");
        let mut changed_flag = false;

        let run = |ui: &mut UiContext, state: &mut TextEditState, chg: &mut bool| {
            ui.begin_frame(400.0, 240.0, 1.0);
            *chg = ui
                .input("username", state)
                .placeholder("Enter username...")
                .w(240.0)
                .changed();
            ui.textarea("bio", &mut TextEditState::new())
                .placeholder("Write bio...")
                .h(90.0);
            ui.end_frame();
        };

        run(&mut ui, &mut text, &mut changed_flag);
        let bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "username")
            .expect("input present")
            .bounds;
        ui.on_pointer_move(bounds.x + 40.0, bounds.y + 12.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);

        ui.on_key_down(Key::Char('!'), Modifiers::default());
        run(&mut ui, &mut text, &mut changed_flag);
        assert!(changed_flag);
        assert_eq!(text.text(), "hi!");

        ui.on_key_down(
            Key::Char('z'),
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        );
        run(&mut ui, &mut text, &mut changed_flag);
        assert_eq!(text.text(), "hi");

        ui.on_ime_preedit(String::from("ka"));
        run(&mut ui, &mut text, &mut changed_flag);
        assert!(text.ime_preedit().is_some());

        ui.on_ime_commit(String::from("か"));
        run(&mut ui, &mut text, &mut changed_flag);
        assert_eq!(text.text(), "hiか");
        assert_eq!(text.ime_preedit(), None);

        let mut pw = TextEditState::with_text("s3cr");
        ui.begin_frame(400.0, 240.0, 1.0);
        let _ = ui.password_input("secret_pw", &mut pw);
        ui.end_frame();
        assert_eq!(pw.text(), "s3cr");
        let sem_pw = ui
            .semantics()
            .iter()
            .find(|n| n.label == "secret_pw")
            .expect("password semantic node");
        assert_eq!(sem_pw.value.as_deref(), Some("****"));
        let has_masked_draw = ui
            .draw_list()
            .base_cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text { text, .. } if text == "****"));
        assert!(has_masked_draw);
    }

    #[test]
    fn viewport_emits_canvas2d_and_scene3d() {
        use crate::gfx::viewport::{Material, MeshData, Transform3D};
        use crate::ui::types::Color;

        let mut ui = UiContext::new();
        let mut cube_mesh = None;

        ui.begin_frame(640.0, 480.0, 1.0);
        ui.viewport("scene_vp", |c2d, s3d| {
            c2d.rect(
                crate::ui::types::Rect::new(10.0, 10.0, 40.0, 20.0),
                Color::rgb(255, 100, 50),
                4.0,
            );
            c2d.line([0.0, 0.0], [100.0, 50.0], 2.0, Color::WHITE);
            let mesh = *cube_mesh.get_or_insert_with(|| s3d.register_mesh(MeshData::cube(1.0)));
            let root = s3d.create_node(None, Transform3D::from_translation(0.0, 0.0, 0.0));
            let child = s3d.create_node(Some(root), Transform3D::from_translation(1.0, 0.5, 0.0));
            s3d.draw_mesh(mesh, child, Material::default());
        })
        .w(320.0)
        .h(200.0);
        ui.end_frame();

        let vp = ui
            .draw_list()
            .base_cmds
            .iter()
            .find_map(|cmd| match cmd {
                DrawCmd::Viewport {
                    rect,
                    canvas,
                    scene,
                } => Some((
                    *rect,
                    canvas.len(),
                    scene.draws.len(),
                    scene.draws[0].model_mat,
                )),
                _ => None,
            })
            .expect("viewport cmd emitted");
        assert!(vp.0.w >= 320.0 && vp.1 == 2 && vp.2 == 1);
        assert!((vp.3[3][0] - 1.0).abs() < 1e-5 && (vp.3[3][1] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn checkbox_custom_toggle_disabled_button_and_context_menu() {
        use crate::ui::types::Color;

        let mut ui = UiContext::new();
        let mut checked = false;
        let mut tog = true;
        let mut menu_open = true;
        let mut btn_clicks = 0u32;

        let frame = |ui: &mut UiContext,
                     chk: &mut bool,
                     t: &mut bool,
                     m_open: &mut bool,
                     clicks: &mut u32| {
            ui.begin_frame(400.0, 300.0, 1.0);
            ui.checkbox("Select Account", chk)
                .box_size(16.0)
                .sz(12.0)
                .fg(Color::WHITE);
            ui.toggle("LensError", t)
                .switch_size(32.0, 18.0)
                .track_bg(Color::from_hex(0x2563EB))
                .thumb_bg(Color::WHITE)
                .sz(12.0);
            if ui.btn("Launch").enabled(false).clicked() {
                *clicks += 1;
            }
            ui.context_menu("row_menu", m_open, [380.0, 280.0], |ui| {
                ui.btn("Edit");
                ui.btn("Delete");
            })
            .w(140.0);
            ui.end_frame();
        };

        frame(
            &mut ui,
            &mut checked,
            &mut tog,
            &mut menu_open,
            &mut btn_clicks,
        );

        // Verify disabled button reports enabled: false in semantics and cannot be clicked.
        let launch_node = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Launch")
            .expect("Launch button");
        assert!(!launch_node.enabled);
        ui.on_pointer_move(launch_node.bounds.x + 5.0, launch_node.bounds.y + 5.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);

        // Verify context_menu clamped inside 400x300 viewport (anchor was [380, 280], width 140).
        let menu_bounds = ui
            .prev_rect_by_label("row_menu")
            .expect("row_menu prev_rect");
        assert!(
            menu_bounds.x + menu_bounds.w <= 400.0 && menu_bounds.y + menu_bounds.h <= 300.0,
            "context_menu overflowed viewport: {menu_bounds:?}"
        );

        // Outside click on Launch button position dismisses context_menu and does not click disabled button.
        frame(
            &mut ui,
            &mut checked,
            &mut tog,
            &mut menu_open,
            &mut btn_clicks,
        );
        assert_eq!(btn_clicks, 0);
        assert!(!menu_open);

        // Click checkbox to toggle it on.
        let chk_bounds = ui
            .prev_rect_by_label("Select Account")
            .expect("checkbox rect");
        ui.on_pointer_move(chk_bounds.x + 6.0, chk_bounds.y + 6.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        frame(
            &mut ui,
            &mut checked,
            &mut tog,
            &mut menu_open,
            &mut btn_clicks,
        );
        assert!(checked);
    }

    #[test]
    fn toggle_scoped_springs_disabled_thumb_scaled_checkbox_and_modal_wrap() {
        use crate::gfx::viewport::{Canvas2DCmd, Material, MeshData, Transform3D};
        use crate::ui::text_edit::TextEditState;
        use crate::ui::types::RectShape;

        let mut ui = UiContext::new();
        let mut t1 = true;
        let mut t2 = false;
        let mut t_dis = false;
        let mut chk_big = true;
        let mut modal_open = true;
        let mut note = TextEditState::new();
        let mut cube_mesh = None;

        for _ in 0..300 {
            ui.begin_frame(800.0, 600.0, 1.0);
            ui.toggle("Toggle One", &mut t1);
            ui.toggle("Toggle Two", &mut t2);
            ui.toggle("Disabled Toggle", &mut t_dis).enabled(false);
            ui.checkbox("Large Checkbox", &mut chk_big).box_size(24.0);
            ui.viewport("spin_vp", |_, s3d| {
                let m = *cube_mesh.get_or_insert_with(|| s3d.register_mesh(MeshData::cube(1.0)));
                let n = s3d.create_node(None, Transform3D::from_euler_xyz(0.1, 0.2, 0.3));
                s3d.draw_mesh(m, n, Material::default());
            });
            ui.modal("wrap_modal", &mut modal_open, |ui| {
                ui.txt("Modal Stress Card").sz(18.0);
                ui.txt("Centered below csd_titlebar. Press Escape or click Close to dismiss.")
                    .sz(13.0)
                    .wrap();
                ui.input("modal_in", &mut note);
                ui.row(|ui| {
                    ui.btn("Confirm");
                    ui.btn("Close");
                });
            })
            .w(420.0)
            .p(18.0)
            .gap(12.0);
            ui.end_frame();
            ui.step_physics(0.016);
        }

        // Verify Toggle One and Toggle Two have independent thumb positions (x of thumb circles).
        let circles: Vec<_> = ui
            .draw_list()
            .base_cmds
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::Rect {
                    rect,
                    bg,
                    shape: RectShape::Circle,
                    ..
                } => Some((*rect, *bg)),
                _ => None,
            })
            .collect();
        assert_eq!(circles.len(), 3);
        assert!(
            circles[0].0.x > circles[1].0.x + 12.0,
            "Toggle One thumb should be at right and Toggle Two at left: {circles:?}"
        );
        // Disabled toggle thumb uses muted color rather than bright white.
        assert_eq!(circles[2].1, ui.theme().text_muted);

        // Large Checkbox (24x24) checkmark scales beyond 18x18 coordinates (p2.x > 18.0).
        let scaled_mark = ui.draw_list().base_cmds.iter().any(|cmd| match cmd {
            DrawCmd::Viewport { canvas, .. } => canvas
                .iter()
                .any(|c| matches!(c, Canvas2DCmd::Segment { p1, .. } if p1[0] > 18.0)),
            _ => false,
        });
        assert!(scaled_mark, "expected 24x24 scaled checkmark segment");

        // Modal card fits Confirm button cleanly inside card bottom padding.
        let modal_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "wrap_modal")
            .expect("wrap_modal")
            .bounds;
        let confirm_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Confirm")
            .expect("Confirm")
            .bounds;
        assert!(
            confirm_rect.y + confirm_rect.h <= modal_rect.y + modal_rect.h - 17.5,
            "Confirm button clipped by modal: confirm={confirm_rect:?}, modal={modal_rect:?}"
        );
    }

    #[test]
    fn navigation_hierarchy_radio_and_combobox_widgets() {
        use crate::ui::text_edit::TextEditState;

        let mut ui = UiContext::new();
        let mut tab = 0usize;
        let mut seg = 1usize;
        let mut header_open = false;
        let mut tree_open = false;
        let mut radio_sel = 0usize;
        let mut combo_sel = 0usize;
        let mut combo_query = TextEditState::new();
        let opts = ["Alpha", "Beta", "Gamma", "Delta"];

        let run = |ui: &mut UiContext,
                   tab: &mut usize,
                   seg: &mut usize,
                   h_open: &mut bool,
                   t_open: &mut bool,
                   r_sel: &mut usize,
                   c_sel: &mut usize,
                   c_q: &mut TextEditState|
         -> (bool, bool, bool, bool) {
            ui.begin_frame(640.0, 480.0, 1.0);
            let t_chg = ui
                .tabs("main_tabs", tab, &["One", "Two", "Three"])
                .changed();
            let _ = ui.segmented_control("seg", seg, &["Day", "Week"]);
            ui.collapsing_header("Section", h_open, |ui| {
                ui.txt("Inside section");
            });
            ui.tree_view("fs_tree", |ui| {
                ui.tree_node("src", t_open, true, |ui| {
                    ui.tree_node("main.rs", &mut false, false, |_| {});
                });
            });
            let r_chg = ui.radio_group("rg", r_sel, &["Small", "Large"]).changed();
            let c_chg = ui.combobox("combo", c_sel, c_q, &opts).changed();
            ui.end_frame();
            (t_chg, *h_open, r_chg, c_chg)
        };

        run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );

        // Click tab "Two" then press Key::Right -> selects tab 2 ("Three").
        let two_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Two")
            .expect("tab Two")
            .bounds;
        ui.on_pointer_move(two_rect.x + 5.0, two_rect.y + 5.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        let (t_chg, _, _, _) = run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );
        assert!(t_chg);
        assert_eq!(tab, 1);

        ui.on_key_down(Key::Right, Modifiers::default());
        let (t_chg2, _, _, _) = run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );
        assert!(t_chg2);
        assert_eq!(tab, 2);

        // Toggle collapsing_header and tree_node.
        let sec_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Section")
            .expect("Section header")
            .bounds;
        ui.on_pointer_move(sec_rect.x + 12.0, sec_rect.y + 12.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );
        assert!(header_open);
        assert!(ui.semantics().iter().any(|n| n.label == "Inside section"));

        let tree_src = ui
            .semantics()
            .iter()
            .find(|n| n.label == "src")
            .expect("tree src")
            .bounds;
        ui.on_pointer_move(tree_src.x + 12.0, tree_src.y + 12.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );
        assert!(tree_open);
        let child_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "main.rs")
            .expect("child main.rs")
            .bounds;
        assert!(child_rect.x >= tree_src.x);

        // Radio group click.
        let large_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Large")
            .expect("Large radio")
            .bounds;
        ui.on_pointer_move(large_rect.x + 8.0, large_rect.y + 8.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        let (_, _, r_chg, _) = run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );
        assert!(r_chg);
        assert_eq!(radio_sel, 1);

        // Combobox filter and select.
        let combo_input = ui
            .semantics()
            .iter()
            .find(|n| n.label == "__combobox_input__")
            .expect("combobox input")
            .bounds;
        ui.on_pointer_move(combo_input.x + 8.0, combo_input.y + 8.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        ui.on_key_down(Key::Char('g'), Modifiers::default());
        ui.on_key_down(Key::Char('a'), Modifiers::default());
        run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );
        assert!(ui
            .semantics()
            .iter()
            .any(|n| n.label == "Gamma" && n.is_overlay));
        assert!(!ui
            .semantics()
            .iter()
            .any(|n| n.label == "Alpha" && n.is_overlay));

        let gamma_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Gamma" && n.is_overlay)
            .expect("Gamma option")
            .bounds;
        ui.on_pointer_move(gamma_rect.x + 8.0, gamma_rect.y + 8.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        let (_, _, _, c_chg) = run(
            &mut ui,
            &mut tab,
            &mut seg,
            &mut header_open,
            &mut tree_open,
            &mut radio_sel,
            &mut combo_sel,
            &mut combo_query,
        );
        assert!(c_chg);
        assert_eq!(combo_sel, 2);
    }

    #[test]
    fn scrollbar_drag_and_table_sort_resize() {
        use super::{TableColumn, TableState};

        let mut ui = UiContext::new();
        let cols = [
            TableColumn::new("Name", 140.0),
            TableColumn::new("Score", 100.0),
        ];
        let mut tbl = TableState::default();

        let run = |ui: &mut UiContext, tbl: &mut TableState| -> bool {
            ui.begin_frame(400.0, 320.0, 1.0);
            ui.scroll("long_scroll", |ui| {
                for i in 0..30 {
                    ui.txt(&format!("Item {i}")).h(24.0);
                }
            })
            .h(120.0);
            let sorted = ui
                .table("scores", &cols, tbl, 50, |ui, r, c| {
                    if c == 0 {
                        ui.txt(&format!("Row {r}"));
                    } else {
                        ui.txt(&format!("{}", r * 10));
                    }
                })
                .w(300.0)
                .h(160.0)
                .changed();
            ui.end_frame();
            sorted
        };

        run(&mut ui, &mut tbl);
        let sb = ui
            .scrollbar_rect("long_scroll")
            .expect("visible scrollbar rect when content overflows");
        assert!(sb.h > 40.0 && sb.w >= 6.0);

        // Drag scrollbar thumb down to bottom of track.
        ui.on_pointer_move(sb.x + sb.w * 0.5, sb.y + 4.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_move(sb.x + sb.w * 0.5, sb.y + sb.h - 4.0);
        run(&mut ui, &mut tbl);
        ui.on_pointer_up(PointerButton::Left);
        assert!(
            ui.scroll_offset("long_scroll") > 100.0,
            "expected dragging scrollbar to scroll: {}",
            ui.scroll_offset("long_scroll")
        );

        // Click "Name" header to sort ascending, then click again for descending.
        let name_hdr = ui
            .semantics()
            .iter()
            .find(|n| n.label.starts_with("Name"))
            .expect("Name header")
            .bounds;
        ui.on_pointer_move(name_hdr.x + 10.0, name_hdr.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        assert!(run(&mut ui, &mut tbl));
        assert_eq!(tbl.sort_col, Some(0));
        assert!(tbl.sort_asc);

        // Drag column 0 resize handle +30px wider.
        let resize0 = ui
            .semantics()
            .iter()
            .find(|n| n.label == "ColResize_0")
            .expect("ColResize_0")
            .bounds;
        ui.on_pointer_move(resize0.x + 2.0, resize0.y + 8.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_move(resize0.x + 32.0, resize0.y + 8.0);
        run(&mut ui, &mut tbl);
        ui.on_pointer_up(PointerButton::Left);
        assert!(
            (tbl.col_widths[0] - 170.0).abs() < 2.0,
            "w={}",
            tbl.col_widths[0]
        );
    }

    #[test]
    fn feedback_and_specialized_inputs() {
        use super::ToastQueue;
        use crate::ui::types::Color;

        let mut ui = UiContext::new();
        let mut toasts = ToastQueue::new();
        toasts.info("Saved!");
        toasts.error("Timeout");
        let mut qty = 5.0_f32;
        let mut low = 20.0_f32;
        let mut high = 80.0_f32;
        let mut tint = Color::from_hex(0x3B82F6);

        let run = |ui: &mut UiContext,
                   toasts: &mut ToastQueue,
                   qty: &mut f32,
                   low: &mut f32,
                   high: &mut f32,
                   tint: &mut Color| {
            ui.begin_frame(500.0, 400.0, 1.0);
            ui.progress_bar(0.6);
            ui.progress_bar_indeterminate();
            ui.spinner();
            ui.badge("NEW");
            ui.badge("");
            ui.number_input("qty", qty, 2.0, 0.0..=10.0);
            ui.range_slider("bounds", low, high, 0.0..=100.0);
            ui.color_picker("accent", tint);
            ui.toasts(toasts);
            ui.end_frame();
        };

        run(
            &mut ui,
            &mut toasts,
            &mut qty,
            &mut low,
            &mut high,
            &mut tint,
        );
        assert_eq!(toasts.len(), 2);
        assert!(ui.has_active_animations());

        // Step 4.0s (80 * 50ms) -> info toast (3.5s) auto-expires, error toast (5.5s) remains.
        for _ in 0..80 {
            ui.step_physics(0.05);
        }
        run(
            &mut ui,
            &mut toasts,
            &mut qty,
            &mut low,
            &mut high,
            &mut tint,
        );
        assert_eq!(toasts.len(), 1);

        // Click '+' on number_input -> qty becomes 7.0.
        let plus_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "+")
            .expect("+ button")
            .bounds;
        ui.on_pointer_move(plus_rect.x + 4.0, plus_rect.y + 4.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(
            &mut ui,
            &mut toasts,
            &mut qty,
            &mut low,
            &mut high,
            &mut tint,
        );
        assert!((qty - 7.0).abs() < 1e-4);

        // Drag range_slider near left (10%) -> moves `low` down toward ~10.0 while keeping `high` at 80.
        let rs_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "bounds" && n.value.is_some())
            .expect("range_slider track")
            .bounds;
        ui.on_pointer_move(rs_rect.x + rs_rect.w * 0.1, rs_rect.y + rs_rect.h * 0.5);
        ui.on_pointer_down(PointerButton::Left);
        run(
            &mut ui,
            &mut toasts,
            &mut qty,
            &mut low,
            &mut high,
            &mut tint,
        );
        ui.on_pointer_up(PointerButton::Left);
        assert!(
            low < 15.0 && (high - 80.0).abs() < 1e-3,
            "low={low}, high={high}"
        );

        // Open color_picker and verify overlay SV canvas appears.
        let cp_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "accent")
            .expect("color_picker")
            .bounds;
        ui.on_pointer_move(cp_rect.x + 10.0, cp_rect.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(
            &mut ui,
            &mut toasts,
            &mut qty,
            &mut low,
            &mut high,
            &mut tint,
        );
        assert!(ui
            .semantics()
            .iter()
            .any(|n| n.label == "ColorSV" && n.is_overlay));
    }

    #[test]
    fn phase6_grid_breakpoints_sticky_cursor_passthrough_and_typography() {
        use crate::ui::types::{Color, CursorIcon, TextAlign};

        let mut ui = UiContext::new();
        let mut acc_open = Some(0usize);
        let mut s_val = 0.5_f32;
        let mut bg_clicks = 0u32;

        let run = |ui: &mut UiContext, acc: &mut Option<usize>, sv: &mut f32, bgc: &mut u32| {
            ui.begin_frame(800.0, 600.0, 1.0);
            assert!(!ui.is_mobile());
            assert_eq!(ui.bp("sm", "md", "lg"), "md");

            ui.grid(2, |ui| {
                ui.txt("Cell 0")
                    .align(TextAlign::Center)
                    .tracking(0.04)
                    .line_height(1.5);
                ui.txt("Cell 1").aspect(16.0 / 9.0);
                ui.txt("Cell 2").w_frac(0.5).h_frac(0.1);
                ui.txt("Cell 3")
                    .selectable()
                    .selection_bg(Color::from_hex(0x22C55E));
            })
            .w(400.0)
            .gap(10.0);

            ui.grid_auto(120.0, |ui| {
                ui.btn("A");
                ui.btn("B");
                ui.btn("C");
            })
            .w(380.0)
            .gap(8.0);

            ui.scroll("sticky_scroll", |ui| {
                ui.txt("StickyHeader").sticky(0.0).h(28.0);
                for i in 0..12 {
                    ui.txt(&format!("SRow {i}")).h(30.0);
                }
            })
            .h(120.0);

            ui.zstack(|ui| {
                if ui.btn("UnderlyingBtn").w(120.0).h(40.0).clicked() {
                    *bgc += 1;
                }
                ui.viewport("bg_vp", |_, _| {})
                    .w(200.0)
                    .h(100.0)
                    .passthrough();
            });

            ui.txt("GrabMe").w(80.0).h(24.0).cursor(CursorIcon::Grab);
            ui.slider("CustomSlider", sv, 0.0..=1.0)
                .track_h(10.0)
                .thumb_size(14.0);
            ui.accordion("faq", acc, &["Q1", "Q2"], |ui, idx| {
                ui.txt(&format!("Ans {idx}"));
            });
            ui.end_frame();
        };

        run(&mut ui, &mut acc_open, &mut s_val, &mut bg_clicks);

        // Verify 2-column grid positions Cell 0 and Cell 1 on the same Y row and Cell 1 has 16:9 aspect.
        let c0 = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Cell 0")
            .unwrap()
            .bounds;
        let c1 = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Cell 1")
            .unwrap()
            .bounds;
        assert!((c0.y - c1.y).abs() < 1.5, "c0={c0:?}, c1={c1:?}");
        assert!((c1.w / c1.h - 16.0 / 9.0).abs() < 0.1, "aspect={c1:?}");

        // Verify typography fields landed on DrawCmd::Text.
        assert!(ui.draw_list().base_cmds.iter().any(|cmd| matches!(
            cmd,
            DrawCmd::Text {
                text,
                align: TextAlign::Center,
                tracking,
                line_height,
                ..
            } if text == "Cell 0" && (*tracking - 0.04).abs() < 1e-4 && (*line_height - 1.5).abs() < 1e-4
        )));

        // Verify passthrough viewport does not block click on UnderlyingBtn.
        let ubtn = ui
            .semantics()
            .iter()
            .find(|n| n.label == "UnderlyingBtn")
            .unwrap()
            .bounds;
        ui.on_pointer_move(ubtn.x + 10.0, ubtn.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut acc_open, &mut s_val, &mut bg_clicks);
        assert_eq!(bg_clicks, 1);

        // Verify custom cursor reporting.
        let grab_rect = ui
            .semantics()
            .iter()
            .find(|n| n.label == "GrabMe")
            .unwrap()
            .bounds;
        ui.on_pointer_move(grab_rect.x + 10.0, grab_rect.y + 10.0);
        assert_eq!(ui.active_cursor(), CursorIcon::Grab);

        // Verify sticky header stays pinned at top of scroll viewport after smooth scrolling down.
        let scroll_rect = ui.prev_rect_by_label("sticky_scroll").unwrap();
        ui.scroll_to_smooth("sticky_scroll", 150.0);
        for _ in 0..80 {
            ui.step_physics(0.02);
            run(&mut ui, &mut acc_open, &mut s_val, &mut bg_clicks);
        }
        let sh_node = ui
            .semantics()
            .iter()
            .find(|n| n.label == "StickyHeader")
            .unwrap()
            .clone();
        assert!(
            (sh_node.bounds.y - scroll_rect.y).abs() < 2.0,
            "sh={:?}, scroll={scroll_rect:?}",
            sh_node.bounds
        );
        assert!(ui.scroll_progress("sticky_scroll") > 0.4);
        assert!(ui.in_view_id(sh_node.id));
        assert!(ui.in_view("sticky_scroll"));

        // Verify accordion exclusivity: clicking Q2 closes Q1.
        let q2 = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Q2")
            .unwrap()
            .bounds;
        ui.on_pointer_move(q2.x + 10.0, q2.y + 10.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut acc_open, &mut s_val, &mut bg_clicks);
        assert_eq!(acc_open, Some(1));
        assert!(ui.semantics().iter().any(|n| n.label == "Ans 1"));
        assert!(!ui.semantics().iter().any(|n| n.label == "Ans 0"));
    }

    #[test]
    fn phase7_gradients_transforms_hit_testing_svg_icon_and_telemetry() {
        let mut ui = UiContext::new();
        let mut clicks = 0usize;
        let red = Color::from_hex(0xEF4444);
        let blue = Color::from_hex(0x3B82F6);
        let green = Color::from_hex(0x22C55E);
        let cyan = Color::from_hex(0x06B6D4);
        let yellow = Color::from_hex(0xFACC15);

        let draw = |ui: &mut UiContext, clicks: &mut usize| {
            ui.begin_frame(600.0, 400.0, 1.0);
            ui.col(|ui| {
                if ui
                    .btn("TransformedBtn")
                    .w(100.0)
                    .h(40.0)
                    .bg_grad(red, blue, 45.0)
                    .bd_grad(2.0, green, yellow, 0.0)
                    .translate(200.0, 0.0)
                    .scale(2.0)
                    .clicked()
                {
                    *clicks += 1;
                }
                ui.svg_icon("M 4 12 L 20 12", 24.0).fg(cyan);
            });
            ui.end_frame();
        };

        draw(&mut ui, &mut clicks);

        // Verify background & border gradients and transform appear in DrawCmd::Rect.
        let has_grad = ui.draw_list().base_cmds.iter().any(|c| {
            matches!(
                c,
                DrawCmd::Rect {
                    bg_grad: Some(_),
                    bd_grad: Some(_),
                    transform,
                    ..
                } if (transform[0] - 2.0).abs() < 1e-4 && (transform[2] - 200.0).abs() < 1e-4
            )
        });
        assert!(has_grad, "expected gradient + transform DrawCmd::Rect");

        // Verify svg_icon tinted with CYAN.
        let has_svg_cyan = ui.draw_list().base_cmds.iter().any(|c| match c {
            DrawCmd::Viewport { canvas, .. } => canvas
                .iter()
                .any(|cmd| matches!(cmd, Canvas2DCmd::Segment { color, .. } if *color == cyan)),
            _ => false,
        });
        assert!(has_svg_cyan, "expected svg_icon tinted with cyan");

        // Scaled 2x + translated +200x: test clicking near scaled edge.
        let r = ui
            .semantics()
            .iter()
            .find(|n| n.label == "TransformedBtn")
            .unwrap()
            .bounds;
        let hit_x = r.x + 200.0 + r.w * 0.5 + 80.0;
        let hit_y = r.y + r.h * 0.5;
        ui.on_pointer_move(hit_x, hit_y);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        draw(&mut ui, &mut clicks);
        assert_eq!(clicks, 1);

        // Verify extended GPU telemetry fields flow into PerfStats and F12 HUD.
        ui.update_gpu_perf(Some(1.25), 16384);
        assert_eq!(ui.perf_stats().gpu_time_ms, Some(1.25));
        assert_eq!(ui.perf_stats().gpu_buffer_bytes, 16384);
        ui.on_key_down(Key::F12, Modifiers::default());
        draw(&mut ui, &mut clicks);
        assert!(ui
            .semantics()
            .iter()
            .any(|n| n.label.contains("GPU 1.25ms | VBO 16.0 KiB")));
    }

    #[test]
    fn phase8_custom_font_syntax_highlighting_line_numbers_copyable_and_markdown() {
        let mut ui = UiContext::new();
        let snippet = "fn main() {\n    let x = 42; // hi\n}";
        let doc = "# Title\n## Section\n### Sub\n\nParagraph with **bold**, *italic*, `code`, and [docs](doc_id).\n\n- Alpha\n- Beta\n\n> Note here\n\n---\n\n```rust\nlet y = \"ok\";\n```";

        let run = |ui: &mut UiContext| {
            ui.begin_frame(800.0, 900.0, 1.0);
            ui.col(|ui| {
                ui.txt("Custom").font("Inter");
                ui.code_block(snippet)
                    .highlight("rust")
                    .line_numbers()
                    .copyable();
                ui.bullets(&["One", "**Two**"]);
                ui.markdown(doc);
            });
            ui.end_frame();
        };

        run(&mut ui);

        // Verify .font("Inter") propagates to DrawCmd::Text.
        assert!(ui.draw_list().base_cmds.iter().any(|c| matches!(
            c,
            DrawCmd::Text { text, font_family: Some(f), .. } if text == "Custom" && f == "Inter"
        )));

        // Verify highlighted code_block with line numbers emitted multi-colored RichText spans.
        let code_rich = ui.draw_list().base_cmds.iter().find_map(|c| match c {
            DrawCmd::RichText { spans, .. }
                if spans.iter().any(|s| s.text.contains(" 1 | "))
                    && spans.iter().any(|s| s.text == "fn") =>
            {
                Some(spans.clone())
            }
            _ => None,
        });
        let spans = code_rich.expect("expected highlighted + line-numbered code block RichText");
        let fn_col = spans.iter().find(|s| s.text == "fn").unwrap().color;
        let num_col = spans.iter().find(|s| s.text == "42").unwrap().color;
        assert_ne!(fn_col, num_col);

        // Verify Copy button copies clean un-numbered snippet to clipboard.
        let copy_btn = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Copy")
            .unwrap()
            .bounds;
        ui.on_pointer_move(copy_btn.x + 4.0, copy_btn.y + 4.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui);
        assert_eq!(crate::os::clipboard::get_text().as_deref(), Some(snippet));

        // Verify markdown headings, bullets, blockquote, and fenced code block appear in semantics.
        let sems = ui.semantics();
        assert!(sems.iter().any(|n| n.label == "Title"));
        assert!(sems.iter().any(|n| n.label == "Section"));
        assert!(sems.iter().any(|n| n.label == "Sub"));
        assert!(ui.draw_list().base_cmds.iter().any(|c| matches!(
            c,
            DrawCmd::RichText { spans, .. } if spans.iter().any(|s| s.text == "bold" && s.kind == crate::SpanKind::Bold)
        )));
    }

    #[test]
    fn input_click_places_caret_and_drag_selects_text() {
        use crate::ui::text_edit::TextEditState;
        let mut ui = UiContext::new();
        let mut st = TextEditState::with_text("hello world");

        let run = |ui: &mut UiContext, st: &mut TextEditState| {
            ui.begin_frame(400.0, 200.0, 1.0);
            ui.input("edit", st).w(240.0);
            ui.end_frame();
        };

        run(&mut ui, &mut st);
        let bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "edit")
            .unwrap()
            .bounds;

        // Click near the beginning of the text box -> caret moves near start.
        ui.on_pointer_move(bounds.x + 12.0, bounds.y + bounds.h * 0.5);
        ui.on_pointer_down(PointerButton::Left);
        run(&mut ui, &mut st);
        let start_cur = st.cursor();
        assert!(start_cur <= 2, "expected caret near start, got {start_cur}");

        // Drag to the right -> selects a substring.
        ui.on_pointer_move(bounds.x + 70.0, bounds.y + bounds.h * 0.5);
        run(&mut ui, &mut st);
        ui.on_pointer_up(PointerButton::Left);
        run(&mut ui, &mut st);
        assert!(
            st.selection_range().is_some(),
            "expected drag selection range"
        );
    }

    #[test]
    fn toast_vector_close_customization_and_toasts_with() {
        use super::{ToastCloseStyle, ToastQueue, ToastStyle};

        let mut ui = UiContext::new();
        let mut toasts = ToastQueue::new();
        toasts.success("Layout snapped cleanly");

        // 1. Default toast renders vector close icon inside "Dismiss" button (no literal "x")
        //    and dismisses on click.
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.toasts(&mut toasts);
        ui.end_frame();

        assert!(
            !ui.semantics().iter().any(|n| n.label == "x"),
            "default toast close button must not render literal 'x'"
        );
        let dismiss_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Dismiss")
            .expect("vector Dismiss button")
            .bounds;
        let icon_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "SvgIcon")
            .expect("centered vector close SvgIcon")
            .bounds;
        let btn_cx = dismiss_bounds.x + dismiss_bounds.w * 0.5;
        let btn_cy = dismiss_bounds.y + dismiss_bounds.h * 0.5;
        let icon_cx = icon_bounds.x + icon_bounds.w * 0.5;
        let icon_cy = icon_bounds.y + icon_bounds.h * 0.5;
        assert!((btn_cx - icon_cx).abs() <= 1.0 && (btn_cy - icon_cy).abs() <= 1.0);

        // Click the Dismiss button -> toast is removed.
        ui.on_pointer_move(btn_cx, btn_cy);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.toasts(&mut toasts);
        ui.end_frame();
        assert!(toasts.is_empty());

        // 2. Glass card styling + Hidden close style + Text close style.
        toasts.style = ToastStyle::default()
            .bg(Color::rgba(18, 22, 34, 210))
            .r(14.0)
            .p(12.0)
            .glass(true)
            .close_style(ToastCloseStyle::Hidden);
        toasts.info("Glass toast");
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.toasts(&mut toasts);
        ui.end_frame();
        assert!(!ui.semantics().iter().any(|n| n.label == "Dismiss"));
        assert!(ui
            .draw_list()
            .overlay_cmds
            .iter()
            .any(|c| matches!(c, DrawCmd::Rect { glass: true, .. })));

        toasts.style = toasts
            .style
            .clone()
            .close_style(ToastCloseStyle::Text(String::from("OK")));
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.toasts(&mut toasts);
        ui.end_frame();
        let ok_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "OK")
            .expect("text close button")
            .bounds;
        ui.on_pointer_move(ok_bounds.x + 4.0, ok_bounds.y + 4.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.toasts(&mut toasts);
        ui.end_frame();
        assert!(toasts.is_empty());

        // 3. Custom item renderer via `toasts_with`.
        toasts.warn("Custom rendered");
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.toasts_with(&mut toasts, |ui, item, accent| {
            let mut dismiss = false;
            ui.row(|ui| {
                ui.txt(&item.message).fg(accent);
                if ui.btn("Ack").clicked() {
                    dismiss = true;
                }
            });
            dismiss
        });
        ui.end_frame();
        let ack_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Ack")
            .expect("custom Ack button")
            .bounds;
        ui.on_pointer_move(ack_bounds.x + 4.0, ack_bounds.y + 4.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        ui.begin_frame(800.0, 600.0, 1.0);
        ui.toasts_with(&mut toasts, |ui, item, accent| {
            let mut dismiss = false;
            ui.row(|ui| {
                ui.txt(&item.message).fg(accent);
                if ui.btn("Ack").clicked() {
                    dismiss = true;
                }
            });
            dismiss
        });
        ui.end_frame();
        assert!(toasts.is_empty());
    }

    #[test]
    fn csd_titlebar_ghost_controls_and_custom_styling() {
        let mut ui = UiContext::new();
        ui.begin_frame(800.0, 600.0, 1.0);
        let _ = ui.csd_titlebar_styled("TaskMgr", 36.0, crate::ui::CsdStyle::ghost(), |ui| {
            ui.txt("Center").sz(12.0);
        });
        ui.end_frame();

        let close_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "Close")
            .expect("Close button in semantics")
            .bounds;

        // Idle ghost close button must NOT emit a bright red solid fill rect.
        let has_red_idle = ui.draw_list().base_cmds.iter().any(|c| match c {
            DrawCmd::Rect { rect, bg, .. } => {
                (rect.x - close_bounds.x).abs() < 1.0
                    && (rect.y - close_bounds.y).abs() < 1.0
                    && *bg == Color::from_hex(0x9E2A2B)
            }
            _ => false,
        });
        assert!(
            !has_red_idle,
            "ghost close button must not be red when idle"
        );

        // Hovering Close button activates the hover fill.
        ui.on_pointer_move(close_bounds.x + 4.0, close_bounds.y + 4.0);
        ui.begin_frame(800.0, 600.0, 1.0);
        let _ = ui.csd_titlebar_styled("TaskMgr", 36.0, crate::ui::CsdStyle::ghost(), |_| {});
        ui.end_frame();
        let has_hover_red = ui.draw_list().base_cmds.iter().any(|c| match c {
            DrawCmd::Rect { rect, bg, .. } => {
                (rect.x - close_bounds.x).abs() < 1.0
                    && (rect.y - close_bounds.y).abs() < 1.0
                    && *bg == Color::from_hex(0xC42B1C)
            }
            _ => false,
        });
        assert!(has_hover_red, "ghost close button should fill red on hover");
    }

    #[test]
    fn table_with_multiline_headers_dividers_and_full_row_selection() {
        let mut ui = UiContext::new();
        let cols = [
            super::TableColumn::new("Name", 160.0).resizable(false),
            super::TableColumn::new("CPU", 100.0).resizable(false),
        ];
        let mut state = super::TableState {
            header_height: Some(46.0),
            col_dividers: true,
            ..super::TableState::default()
        };

        let draw = |ui: &mut UiContext, state: &mut super::TableState| {
            ui.begin_frame(600.0, 400.0, 1.0);
            ui.table_with(
                "procs",
                &cols,
                state,
                3,
                |ui, c_idx, _| {
                    ui.col(|ui| {
                        ui.txt(if c_idx == 1 { "25%" } else { "" }).sz(13.0);
                        ui.txt(cols[c_idx].header).sz(11.0);
                    });
                },
                |ui, r_idx, c_idx| {
                    if c_idx == 0 {
                        let _ = ui.btn(&format!("Row {r_idx}"));
                    } else {
                        ui.txt(&format!("{}%", r_idx * 10));
                    }
                },
            );
            ui.end_frame();
        };

        draw(&mut ui, &mut state);
        assert!(ui.semantics().iter().any(|n| n.label == "25%"));

        // Clicking inside column 1 (non-button cell) of row 1 selects row 1.
        let pct_bounds = ui
            .semantics()
            .iter()
            .find(|n| n.label == "10%")
            .expect("row 1 col 1 text")
            .bounds;
        ui.on_pointer_move(pct_bounds.x + 2.0, pct_bounds.y + 2.0);
        ui.on_pointer_down(PointerButton::Left);
        ui.on_pointer_up(PointerButton::Left);
        draw(&mut ui, &mut state);
        assert_eq!(state.selected_row, Some(1));
    }
}
