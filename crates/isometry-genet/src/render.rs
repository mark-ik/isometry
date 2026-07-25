//! The frame: layout, leaves, paint list, present.
//!
//! `redraw` is the whole per-frame path. The overmap's painted leaf is
//! registered here and only when its model changed, and the leaf-box walk runs
//! only while a leaf is live; both gates are load-bearing, so read the
//! 2026-07-20 perf plan before loosening either.
//!
//! Split out of `main.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl App {
    pub(crate) fn scale_factor(&self) -> f64 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    pub(crate) fn redraw(&mut self) {
        let (Some(window), Some(host), Some(runner)) = (
            self.window.as_ref(),
            self.host.as_ref(),
            self.runner.as_ref(),
        ) else {
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
                    // A fresh session cascades with its animation clock still at
                    // zero, so any @keyframes it starts is stamped `start_time =
                    // 0`. Our clock must share that origin, or the very next tick
                    // hands the engine a `now` seconds past the animation's end
                    // and a 420ms beat expires before its first frame. Rebasing
                    // here keeps the two in one timebase.
                    self.clock = Instant::now();
                }
            }
            // Advance the CSS animation clock. A transition or @keyframes run
            // *starts* on the restyle that sets its class (the `apply` above);
            // this re-interpolates it at the current time. On a still board the
            // animation set is empty and this returns `Applied::Unchanged`, so
            // an idle surface pays nothing for the clock existing.
            if let Some(layout) = self.layout.as_mut() {
                let now_s = self.clock.elapsed().as_secs_f64();
                let _ = layout.tick_animations(&*dom_ref, now_s);
            }
            // Register (or clear) the overmap's painted graph leaf so the view's
            // `<custom-leaf>` gets nodes + edges. The swatch is only *built*
            // while the surface is open (building it projects the world and runs
            // the force layout -- never pay that on an ordinary board frame), and
            // the leaf is only *re-registered* when the swatch model changed: a
            // fresh `GraphCanvas` is born dirty, so an unconditional insert would
            // defeat the leaf-tier retention gate and repaint every frame.
            if runner.state().overmap_open {
                match isometry_views::overmap_swatch(runner.state()) {
                    Some(swatch) => {
                        if self.last_overmap_swatch.as_ref() != Some(&swatch) {
                            self.leaves.insert(
                                isometry_views::OVERMAP_LEAF_KEY,
                                Box::new(swatch.paint_leaf(overmap_node_color)),
                            );
                            self.last_overmap_swatch = Some(swatch);
                        }
                    }
                    None => {
                        self.leaves.remove(&isometry_views::OVERMAP_LEAF_KEY);
                        self.last_overmap_swatch = None;
                    }
                }
            } else if self.last_overmap_swatch.is_some() {
                self.leaves.remove(&isometry_views::OVERMAP_LEAF_KEY);
                self.rendered_leaves.retain_keys(|_| false);
                self.last_overmap_swatch = None;
            }

            let layout = self.layout.as_ref().expect("layout just ensured");
            // Repaint each laid-out leaf whose box appears this frame, sizing it
            // from the completed layout, into the retained cache. Skipped whole
            // when no leaf is live: `custom_leaf_boxes` walks the box tree, and
            // an ordinary board frame should not pay that walk for nothing.
            if self.last_overmap_swatch.is_some() {
                let sizes: std::collections::HashMap<u64, (f32, f32)> =
                    layout.custom_leaf_boxes().into_iter().collect();
                self.leaves.render_into(
                    |key| {
                        sizes.get(&key).map(|&(width, height)| Size { width, height })
                    },
                    &mut self.rendered_leaves,
                );
            }
            let source = RenderedLeafSource(&self.rendered_leaves);
            let list = layout.emit_paint_list_with_leaves(
                &*dom_ref,
                &ScrollOffsets::default(),
                DeviceIntSize::new(lw as i32, lh as i32),
                &source,
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
        let (tex, view) = host.core().rasterize_scaled(
            &scene,
            pw,
            ph,
            ColorLoad::Clear(wgpu::Color::BLACK),
            scale,
        );
        if let Some(dir) = &self.capture_dir {
            let rgba = host
                .core()
                .renderer()
                .wgpu_device
                .read_rgba8_texture(&tex, pw, ph);
            let path = dir.join("isometry_capture.png");
            if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| {
                let file = std::fs::File::create(&path)?;
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), pw, ph);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let mut writer = enc.write_header().map_err(std::io::Error::other)?;
                writer
                    .write_image_data(&rgba)
                    .map_err(std::io::Error::other)?;
                Ok(())
            }) {
                eprintln!("[isometry] capture failed: {e}");
            }
        }
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

}
