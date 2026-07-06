//! Isometry's serval desktop host (bootstrap plan I1).
//!
//! A winit window presenting the board screen over live state:
//! `ServalAppRunner` diffs `isometry_views::board_root` into a
//! `ScriptedDom`, a retained `IncrementalLayout` lays it out at logical
//! size (incremental `apply` for attribute-only batches, so a camera pan
//! stays off the rebuild path), paint emission lowers to a
//! `netrender::Scene`, and `serval-winit-host`'s `SurfaceHost` rasterizes
//! and composites onto the backbuffer. Borrowed from the woodshed-serval
//! harness shape.
//!
//! `ISOMETRY_PROFILE=1` prints per-frame scene-build and raster times,
//! the probe P2 receipt hook.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use isometry_views::{board_css, board_root, demo_map, UiChild, UiState};
use layout_dom_api::{DomMutation, LayoutDomMut as _};
use netrender::{ColorLoad, ExternalTexturePlacement, NetrenderOptions};
use paint_list_api::{DeviceIntSize, PaintList as _};
use serval_layout::{
    Applied, IncrementalLayout, InteractionState, ScrollOffsets, SourceNodeId,
};
use serval_scripted_dom::{NodeId, ScriptedDom};
use serval_winit_host::SurfaceHost;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent as WinitKeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};
use winit::window::{Window, WindowId};
use xilem_serval::{PointerClick, Propagation, ServalAppRunner};

type Runner = ServalAppRunner<UiState, fn(&UiState) -> UiChild, UiChild>;

struct App {
    window: Option<Arc<Window>>,
    host: Option<SurfaceHost>,
    runner: Option<Runner>,
    /// Retained layout session in logical coordinates: hit-test target
    /// and incremental-apply subject.
    layout: Option<IncrementalLayout<NodeId>>,
    layout_size: (f32, f32),
    sheet: String,
    cursor: (f32, f32),
    last_hover: Option<u64>,
    last_focus: Option<u64>,
    profile: bool,
}

impl App {
    fn scale_factor(&self) -> f64 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    fn redraw(&mut self) {
        let (Some(window), Some(host), Some(runner)) =
            (self.window.as_ref(), self.host.as_ref(), self.runner.as_ref())
        else {
            return;
        };
        let size = window.inner_size();
        let (pw, ph) = (size.width.max(1), size.height.max(1));
        let scale = window.scale_factor() as f32;
        let (lw, lh) = (pw as f32 / scale, ph as f32 / scale);

        let t0 = std::time::Instant::now();
        let scene = {
            let dom = runner.dom();
            let mut muts: Vec<DomMutation<NodeId>> = Vec::new();
            dom.borrow_mut().drain_mutations(&mut muts);
            let dom_ref = dom.borrow();
            let sheets: Vec<&str> = vec![self.sheet.as_str()];
            let structural = muts
                .iter()
                .any(|m| !matches!(m, DomMutation::AttributeChanged { .. }));
            let size_changed = self.layout_size != (lw, lh);
            match self.layout.as_mut() {
                Some(layout) if !structural && !size_changed => {
                    if !muts.is_empty() {
                        let _ = layout.apply(&*dom_ref, &sheets, &muts);
                    }
                }
                _ => {
                    let mut layout = IncrementalLayout::new(&*dom_ref, &sheets, lw, lh);
                    if let Some(prev) = self.layout.as_ref() {
                        layout.set_element_scroll(prev.element_scroll().clone());
                    }
                    self.layout = Some(layout);
                    self.layout_size = (lw, lh);
                }
            }
            let layout = self.layout.as_ref().expect("layout just ensured");
            let list = layout.emit_paint_list(
                &*dom_ref,
                &ScrollOffsets::default(),
                DeviceIntSize::new(lw as i32, lh as i32),
            );
            let translated = paint_list_render::translate_paint_cmd_stream(
                list.viewport(),
                list.commands(),
                list.fonts(),
                list.images(),
            );
            translated.scene
        };
        let t_scene = t0.elapsed();

        let t1 = std::time::Instant::now();
        let (_tex, view) = host.core().rasterize_scaled(
            &scene,
            pw,
            ph,
            ColorLoad::Clear(wgpu::Color::BLACK),
            scale,
        );
        let Some(frame) = host.acquire() else { return };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        host.renderer().compose_external_texture(
            &view,
            &target,
            host.format(),
            pw,
            ph,
            ExternalTexturePlacement::new([0.0, 0.0, pw as f32, ph as f32]),
        );
        frame.present();
        if self.profile {
            eprintln!(
                "[isometry] scene {:.2}ms raster+present {:.2}ms",
                t_scene.as_secs_f64() * 1000.0,
                t1.elapsed().as_secs_f64() * 1000.0,
            );
        }
    }

    /// Drive `:hover` restyles on target change (engine `set_interaction`;
    /// `Unchanged` when nothing interaction-sensitive matched).
    fn hover(&mut self) {
        let (Some(runner), Some(layout)) = (self.runner.as_ref(), self.layout.as_mut()) else {
            return;
        };
        let (x, y) = self.cursor;
        let dom = runner.dom();
        let dom_ref = dom.borrow();
        let hovered = layout
            .hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
            .map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
        let focused = runner
            .focus()
            .map(|n| layout_dom_api::LayoutDom::opaque_id(&*dom_ref, n));
        if (hovered, focused) == (self.last_hover, self.last_focus) {
            return;
        }
        self.last_hover = hovered;
        self.last_focus = focused;
        let state = InteractionState {
            hovered: hovered.map(SourceNodeId),
            focused: focused.map(SourceNodeId),
            ..Default::default()
        };
        if layout.set_interaction(&*dom_ref, &state) != Applied::Unchanged {
            drop(dom_ref);
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn click(&mut self) {
        let (Some(runner), Some(layout)) = (self.runner.as_mut(), self.layout.as_ref()) else {
            return;
        };
        let (x, y) = self.cursor;
        let hit = {
            let dom = runner.dom();
            let dom_ref = dom.borrow();
            layout.hit_test(&*dom_ref, x, y, &ScrollOffsets::default())
        };
        let Some(node) = hit else { return };
        runner.dispatch_click(
            node,
            PointerClick {
                local: (0.0, 0.0),
                prop: Propagation::new(),
            },
        );
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn key(&mut self, event: &WinitKeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        let pan = match event.logical_key {
            WinitKey::Named(WinitNamedKey::ArrowLeft) => Some((-1.0, 1.0)),
            WinitKey::Named(WinitNamedKey::ArrowRight) => Some((1.0, -1.0)),
            WinitKey::Named(WinitNamedKey::ArrowUp) => Some((-1.0, -1.0)),
            WinitKey::Named(WinitNamedKey::ArrowDown) => Some((1.0, 1.0)),
            _ => None,
        };
        if let Some((dc, dr)) = pan {
            runner.update(|ui| ui.pan_tiles(dc, dr));
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Isometry")
                        .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0)),
                )
                .expect("create window"),
        );
        let size = window.inner_size();
        let host = SurfaceHost::boot(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            NetrenderOptions {
                tile_cache_size: Some(1024),
                enable_vello: true,
                ..Default::default()
            },
        )
        .expect("boot serval host");
        let mut ui = UiState::new(demo_map());
        // Start with the board roughly centered in the pane.
        ui.camera = (420.0, 140.0);
        let dom = Rc::new(RefCell::new(ScriptedDom::new()));
        let runner = Runner::new(dom, board_root as fn(&UiState) -> UiChild, ui);
        self.window = Some(window);
        self.host = Some(host);
        self.runner = Some(runner);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(host) = self.host.as_mut() {
                    host.resize(size.width.max(1), size.height.max(1));
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale_factor();
                self.cursor = (
                    (position.x / scale) as f32,
                    (position.y / scale) as f32,
                );
                self.hover();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.click(),
            WindowEvent::KeyboardInput { event, .. } => self.key(&event),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        window: None,
        host: None,
        runner: None,
        layout: None,
        layout_size: (0.0, 0.0),
        sheet: board_css(),
        cursor: (0.0, 0.0),
        last_hover: None,
        last_focus: None,
        profile: std::env::var_os("ISOMETRY_PROFILE").is_some(),
    };
    event_loop.run_app(&mut app).expect("run app");
}
