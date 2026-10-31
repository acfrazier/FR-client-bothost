//! Common, allocation-free owner census for the drawing-memory A/B.
//!
//! Included capacities are the renderer's CPU `PixMap` pixels (draw area,
//! optional in-game areas, and title areas), optional minimap pixels, the
//! four minimap/compass mask vectors, tile-occupancy stamps, active map
//! function vectors and owned sprite pixels, the per-chat string buffers,
//! texture animation scratch, and title rune/icon storage. The map-function
//! sprite-slot and rune-slot fields measure their `Vec` element storage;
//! their nested pixel/palette fields measure only initialized sprites' owned
//! buffers. `renderer_inline_bytes` and `backend_inline_bytes` are scalar
//! layouts, not heap payload.
//!
//! This is deliberately a partial owner view. Shared `Media` allocations,
//! allocator metadata, `Pix3D`/`RenderWorld` internals, private retained
//! title/graphic caches, and backend-owned retained storage are excluded.
//! In particular it does not count the `graphic_sprites` cache because its
//! representation differs between the comparator and candidate. It never
//! treats a Rust descriptor or inline size as GPU payload. A CPU output is
//! measured only when the supplied `FrameOutput` is a `PixMap`; GPU texture
//! storage and handles are outside this census.

use std::mem::size_of;

use crate::graphics::{Pix32, Pix8, PixMap};
use crate::render::backend::{BackendKind, FrameOutput};
use crate::render::Renderer;

/// Fixed-storage, primitive-only snapshot of comparable drawing owners.
///
/// All byte capacities are actual `Vec::capacity()` values multiplied by
/// the element size, except `chat_string_capacity_bytes`, which sums the
/// byte capacities of the 50 owned chat strings. Optional owner fields are
/// `None` only when the corresponding owner is absent. Surface arrays use
/// `None` for absent `PixMap`s; their slot order is documented inline.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Census {
    /// Inline size of this fixed-storage result, useful for accounting for
    /// the diagnostic value itself when it is retained by the caller.
    pub diagnostic_inline_bytes: usize,
    /// `size_of::<Renderer>()`; includes inline descriptors and scalars,
    /// but is not a measure of pointees or other heap allocations.
    pub renderer_inline_bytes: usize,
    /// Concrete `RenderBackend` value size behind the renderer's box. This
    /// is a layout size only; it excludes the box allocation's metadata.
    pub backend_inline_bytes: usize,
    /// Renderer frame cycle at the instant of the census.
    pub scene_cycle: i32,
    /// Whether the renderer reports the GPU backend as selected.
    pub gpu_backend: bool,

    /// Capacity bytes for `Renderer::draw_area.pixels`.
    pub draw_area_pixel_capacity_bytes: usize,
    /// Capacities for `area_game`, `area_map`, `area_side`, `area_chat`,
    /// `area_backbase1`, `area_backbase2`, and `area_backhmid1`, in order.
    pub game_area_pixel_capacity_bytes: [Option<usize>; 7],
    /// Capacities for `image_title0` through `image_title8`, in order.
    pub title_area_pixel_capacity_bytes: [Option<usize>; 9],
    /// Capacity bytes for `minimap.data`, if the minimap exists.
    pub minimap_pixel_capacity_bytes: Option<usize>,

    /// Capacity bytes for the four per-row mask vectors.
    pub compass_mask_line_offsets_capacity_bytes: usize,
    pub compass_mask_line_lengths_capacity_bytes: usize,
    pub minimap_mask_line_offsets_capacity_bytes: usize,
    pub minimap_mask_line_lengths_capacity_bytes: usize,
    /// Capacity bytes for `tile_last_occupied_cycle`.
    pub tile_last_occupied_cycle_capacity_bytes: usize,

    /// Capacity bytes for the active map-function coordinate arrays.
    pub active_map_function_x_capacity_bytes: usize,
    pub active_map_function_z_capacity_bytes: usize,
    /// Capacity bytes of the `Vec<Option<Pix32>>` slot storage, including
    /// inline Rust descriptors but not nested pixel buffers.
    pub active_map_function_sprite_slots_capacity_bytes: usize,
    /// Sum of `Pix32::data` capacity bytes in initialized map-function
    /// sprite slots.
    pub active_map_function_sprite_pixels_capacity_bytes: usize,

    /// Sum of the allocated byte capacities of the 50 chat strings.
    pub chat_string_capacity_bytes: usize,
    /// Capacity bytes for the `i8` texture-animation scratch vector.
    pub texture_buffer_capacity_bytes: usize,

    /// Capacity bytes of the `Vec<Pix8>` rune slots, including inline
    /// descriptors, and of initialized rune pixel and palette buffers.
    pub image_runes_slots_capacity_bytes: usize,
    pub image_runes_data_capacity_bytes: usize,
    pub image_runes_palette_capacity_bytes: usize,
    /// Pixel-data and palette capacities of the optional title icons.
    pub image_titlebox_data_capacity_bytes: Option<usize>,
    pub image_titlebox_palette_capacity_bytes: Option<usize>,
    pub image_titlebutton_data_capacity_bytes: Option<usize>,
    pub image_titlebutton_palette_capacity_bytes: Option<usize>,

    /// CPU output measurements; all three are `None` for non-`PixMap`
    /// outputs, without inspecting backend-specific GPU handle fields.
    pub cpu_output_pixel_capacity_bytes: Option<usize>,
    pub cpu_output_width: Option<i32>,
    pub cpu_output_height: Option<i32>,
}

/// Read the common owner fields from both the comparator and candidate.
///
/// `backend_inline_bytes` is supplied by the feature-gated `Renderer`
/// attachment because its boxed backend field is private to that module.
pub(crate) fn census(
    renderer: &Renderer,
    output: &FrameOutput,
    backend_inline_bytes: usize,
) -> Census {
    let game_area_pixel_capacity_bytes = [
        renderer.area_game.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.area_map.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.area_side.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.area_chat.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer
            .area_backbase1
            .as_ref()
            .map(pixmap_pixel_capacity_bytes),
        renderer
            .area_backbase2
            .as_ref()
            .map(pixmap_pixel_capacity_bytes),
        renderer
            .area_backhmid1
            .as_ref()
            .map(pixmap_pixel_capacity_bytes),
    ];
    let title_area_pixel_capacity_bytes = [
        renderer.image_title0.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title1.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title2.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title3.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title4.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title5.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title6.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title7.as_ref().map(pixmap_pixel_capacity_bytes),
        renderer.image_title8.as_ref().map(pixmap_pixel_capacity_bytes),
    ];
    let cpu_output = match output {
        FrameOutput::PixMap(pixmap) => Some((
            pixmap_pixel_capacity_bytes(pixmap),
            pixmap.width,
            pixmap.height,
        )),
        _ => None,
    };

    Census {
        diagnostic_inline_bytes: size_of::<Census>(),
        renderer_inline_bytes: size_of::<Renderer>(),
        backend_inline_bytes,
        scene_cycle: renderer.scene_cycle,
        gpu_backend: renderer.backend_kind() == BackendKind::Gpu,
        draw_area_pixel_capacity_bytes: pixmap_pixel_capacity_bytes(&renderer.draw_area),
        game_area_pixel_capacity_bytes,
        title_area_pixel_capacity_bytes,
        minimap_pixel_capacity_bytes: renderer
            .minimap
            .as_ref()
            .map(|pixmap| capacity_bytes::<i32>(pixmap.data.capacity())),
        compass_mask_line_offsets_capacity_bytes: capacity_bytes::<i32>(
            renderer.compass_mask_line_offsets.capacity(),
        ),
        compass_mask_line_lengths_capacity_bytes: capacity_bytes::<i32>(
            renderer.compass_mask_line_lengths.capacity(),
        ),
        minimap_mask_line_offsets_capacity_bytes: capacity_bytes::<i32>(
            renderer.minimap_mask_line_offsets.capacity(),
        ),
        minimap_mask_line_lengths_capacity_bytes: capacity_bytes::<i32>(
            renderer.minimap_mask_line_lengths.capacity(),
        ),
        tile_last_occupied_cycle_capacity_bytes: capacity_bytes::<i32>(
            renderer.tile_last_occupied_cycle.capacity(),
        ),
        active_map_function_x_capacity_bytes: capacity_bytes::<i32>(
            renderer.active_map_function_x.capacity(),
        ),
        active_map_function_z_capacity_bytes: capacity_bytes::<i32>(
            renderer.active_map_function_z.capacity(),
        ),
        active_map_function_sprite_slots_capacity_bytes: capacity_bytes::<Option<Pix32>>(
            renderer.active_map_functions.capacity(),
        ),
        active_map_function_sprite_pixels_capacity_bytes: renderer
            .active_map_functions
            .iter()
            .flatten()
            .map(|pixmap| capacity_bytes::<i32>(pixmap.data.capacity()))
            .sum(),
        chat_string_capacity_bytes: renderer.chats.iter().map(String::capacity).sum(),
        texture_buffer_capacity_bytes: capacity_bytes::<i8>(renderer.texture_buffer.capacity()),
        image_runes_slots_capacity_bytes: capacity_bytes::<Pix8>(renderer.image_runes.capacity()),
        image_runes_data_capacity_bytes: renderer
            .image_runes
            .iter()
            .map(|sprite| capacity_bytes::<i8>(sprite.data.capacity()))
            .sum(),
        image_runes_palette_capacity_bytes: renderer
            .image_runes
            .iter()
            .map(|sprite| capacity_bytes::<i32>(sprite.bpal.capacity()))
            .sum(),
        image_titlebox_data_capacity_bytes: renderer
            .image_titlebox
            .as_ref()
            .map(|sprite| capacity_bytes::<i8>(sprite.data.capacity())),
        image_titlebox_palette_capacity_bytes: renderer
            .image_titlebox
            .as_ref()
            .map(|sprite| capacity_bytes::<i32>(sprite.bpal.capacity())),
        image_titlebutton_data_capacity_bytes: renderer
            .image_titlebutton
            .as_ref()
            .map(|sprite| capacity_bytes::<i8>(sprite.data.capacity())),
        image_titlebutton_palette_capacity_bytes: renderer
            .image_titlebutton
            .as_ref()
            .map(|sprite| capacity_bytes::<i32>(sprite.bpal.capacity())),
        cpu_output_pixel_capacity_bytes: cpu_output.map(|(capacity, _, _)| capacity),
        cpu_output_width: cpu_output.map(|(_, width, _)| width),
        cpu_output_height: cpu_output.map(|(_, _, height)| height),
    }
}

fn capacity_bytes<T>(capacity: usize) -> usize {
    capacity * size_of::<T>()
}

fn pixmap_pixel_capacity_bytes(pixmap: &PixMap) -> usize {
    capacity_bytes::<i32>(pixmap.pixels.capacity())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{Pix32, Pix8};

    #[test]
    fn census_reports_common_owner_capacities_and_cpu_output() {
        let mut renderer = Renderer::new_prefer(false, false);
        renderer.area_game = Some(PixMap::new(3, 5));
        renderer.image_title4 = Some(PixMap::new(4, 6));
        renderer.minimap = Some(Pix32::new(7, 8));
        renderer.texture_buffer = Vec::with_capacity(11);
        renderer.image_runes = vec![Pix8::new(2, 3, vec![0; 4])];
        let output = FrameOutput::PixMap(PixMap::new(9, 10));

        let sample = renderer.memory_case_census(&output);

        assert_eq!(sample.diagnostic_inline_bytes, size_of::<Census>());
        assert_eq!(sample.renderer_inline_bytes, size_of::<Renderer>());
        assert_eq!(sample.scene_cycle, renderer.scene_cycle);
        assert!(!sample.gpu_backend);
        assert_eq!(
            sample.game_area_pixel_capacity_bytes[0],
            Some(capacity_bytes::<i32>(
                renderer.area_game.as_ref().unwrap().pixels.capacity()
            ))
        );
        assert_eq!(
            sample.title_area_pixel_capacity_bytes[4],
            Some(capacity_bytes::<i32>(
                renderer.image_title4.as_ref().unwrap().pixels.capacity()
            ))
        );
        assert_eq!(
            sample.minimap_pixel_capacity_bytes,
            Some(capacity_bytes::<i32>(renderer.minimap.as_ref().unwrap().data.capacity()))
        );
        assert_eq!(
            sample.texture_buffer_capacity_bytes,
            capacity_bytes::<i8>(renderer.texture_buffer.capacity())
        );
        assert_eq!(sample.cpu_output_width, Some(9));
        assert_eq!(sample.cpu_output_height, Some(10));
        assert_eq!(
            sample.cpu_output_pixel_capacity_bytes,
            Some(capacity_bytes::<i32>(
                match &output {
                    FrameOutput::PixMap(pixmap) => pixmap.pixels.capacity(),
                    _ => unreachable!(),
                }
            ))
        );
    }
}
