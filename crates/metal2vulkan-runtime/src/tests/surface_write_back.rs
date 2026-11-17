//! A pass's write through a shadowed texture over an IOSurface plane the CPU may write goes back by
//! the device's `SurfaceWriteBackPolicy` (this tree's own: `FORK.md`): under `Merge`, the default,
//! it is kept and merged back (`shadow_merge.rs`); under `CopyBack` a tracked texture's, as this
//! one is, is copied back, with no merge. With no other writer beside the pass, both leave the same
//! bytes: the pass's texels inside its scissor, and the plane's other texels and the bytes between
//! its rows as the CPU left them. A texture over the same host pages that is no plane's merges back
//! under `CopyBack` too: the policy reaches planes alone.
//!
//! The plane lies in a no-copy buffer's host pages, as a host imports a display server's
//! framebuffer, its rows padded past their texels; the device takes
//! `HostPageTexturePolicy::Shadow`, so every texture over host pages is a shadow. The pass loads
//! and stores the texture and draws a solid colour inside a scissor of its first rows (`tob_rect`
//! and `tob_solid` of `engine/tob_renders.ll`): it writes some texels and leaves the rest.
//!
//! Device tests: compiled only under the validation crate's GPU safety cfgs (`m2v_gpu`,
//! `m2v_cpu_icd`), on the kind of device the cfg selects.

use super::blit_holds::{options, over_host, Host, Scratch, WAIT};
use super::encoder_holds::{bytes_of, library};
use crate::{
    Device, IoSurfacePlane, LoadAction, PipelineOptions, RenderPassDescriptor, ScissorRect,
    StoreAction, TextureDescriptor,
};
use metal2vulkan::device::{HostPageTexturePolicy, SurfaceWriteBackPolicy};
use metal2vulkan::metal::{PixelFormat, PrimitiveType, TextureUsage};
use metal2vulkan::pipeline::{ColorAttachmentDescriptor, RenderPipelineDescriptor, StageSource};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

/// The plane: `WIDTH` texels a row, `HEIGHT` rows, rows of `PITCH` bytes, 64 past each row's
/// texels.
const WIDTH: u64 = 64;
const HEIGHT: u64 = 16;
const ROW: u64 = WIDTH * 4;
const PITCH: u64 = ROW + 64;
/// The rows the pass draws into, from the first.
const DRAWN: u64 = 4;
/// Every byte of the host pages before the pass, and every byte of a texel the pass draws.
const BEFORE: u8 = 0x11;
const PAINT: u8 = 0x99;

/// What the pass leaves on a device whose policy is `policy`, over a plane's texture where
/// `plane`, else a plain texture over the same bytes: the merge dispatches it planned, the texels
/// it copied back, and the bytes after it. `None` where the device imports no host memory.
fn pass(policy: SurfaceWriteBackPolicy, plane: bool) -> Option<(u64, u64, Vec<u8>)> {
    let host = Host::new();
    let scratch = Scratch::new(&format!("surface-write-back-{policy:?}-{plane}"));
    let mut options = options(&scratch.0);
    options.policy.host_page_textures = HostPageTexturePolicy::Shadow;
    options.policy.surface_write_back = policy;
    let device = Device::system_default_with(&options).unwrap();
    let runs = Arc::new(AtomicUsize::new(0));
    let buffer = over_host(&device, &host, &runs)?;
    let mut descriptor =
        TextureDescriptor::texture_2d(PixelFormat::BGRA8Unorm, WIDTH, HEIGHT, false);
    descriptor.usage = TextureUsage::SHADER_READ | TextureUsage::RENDER_TARGET;
    let texture = match plane {
        true => {
            let surface = IoSurfacePlane::new(0, PITCH, false);
            buffer.new_texture_with_iosurface_plane(&descriptor, surface)
        }
        false => buffer.new_texture(&descriptor, 0, PITCH),
    };
    let texture = texture.unwrap();
    assert!(
        texture.shadow().is_some(),
        "a render target over host pages"
    );
    host.fill(u32::from_ne_bytes([BEFORE; 4]));

    let renders = library(&device, "engine/tob_renders.ll");
    let vertex = renders.new_function_with_name("tob_rect").unwrap();
    let fragment = renders.new_function_with_name("tob_solid").unwrap();
    let mut solid =
        RenderPipelineDescriptor::new(StageSource::Function(vertex.function_descriptor()));
    solid.fragment = Some(StageSource::Function(fragment.function_descriptor()));
    solid.color_attachments[0] = Some(ColorAttachmentDescriptor::new(PixelFormat::BGRA8Unorm));
    let solid = device.new_render_pipeline_state(&solid, &PipelineOptions::default());
    let solid = solid.unwrap();

    let queue = device.new_command_queue().unwrap();
    let before = device.work_counts();
    let command_buffer = queue.command_buffer().unwrap();
    let mut descriptor = RenderPassDescriptor::new();
    let attachment = &mut descriptor.color_attachments[0];
    attachment.texture = Some(texture.clone());
    attachment.load_action = LoadAction::Load;
    attachment.store_action = StoreAction::Store;
    let mut encoder = command_buffer.render_command_encoder(&descriptor).unwrap();
    encoder.set_render_pipeline_state(&solid).unwrap();
    let scissor = ScissorRect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: DRAWN,
    };
    encoder.set_scissor_rect(scissor).unwrap();
    let corners = bytes_of(&[-1.0f32, -1.0, 1.0, 1.0], f32::to_le_bytes);
    encoder.set_vertex_bytes(&corners, 0).unwrap();
    let color = bytes_of(&[f32::from(PAINT) / 255.0; 4], f32::to_le_bytes);
    encoder.set_fragment_bytes(&color, 0).unwrap();
    (encoder.draw_primitives(PrimitiveType::Triangle, 0, 6, 1, 0)).unwrap();
    encoder.end_encoding().unwrap();
    command_buffer.commit().unwrap();
    command_buffer.wait_until_completed_for(WAIT).unwrap();
    assert_eq!(command_buffer.error(), None);
    let after = device.work_counts();
    let merges = after.shadow_merge_dispatches_planned - before.shadow_merge_dispatches_planned;
    let copied = after.shadow_texels_copied_back - before.shadow_texels_copied_back;
    let words = &host.words()[..(PITCH * HEIGHT / 4) as usize];
    let bytes = words.iter().flat_map(|word| word.to_ne_bytes()).collect();
    Some((merges, copied, bytes))
}

/// That `bytes` are what the pass leaves where nothing else writes the plane: its first `DRAWN`
/// rows' texels drawn, every other byte as the CPU left it.
fn assert_drawn(bytes: &[u8], what: &str) {
    let mut expected = vec![BEFORE; (PITCH * HEIGHT) as usize];
    for row in expected.chunks_mut(PITCH as usize).take(DRAWN as usize) {
        row[..ROW as usize].fill(PAINT);
    }
    assert_eq!(bytes.len(), expected.len());
    let differing = (0..expected.len()).filter(|&i| bytes[i] != expected[i]);
    let differing: Vec<usize> = differing.take(16).collect();
    assert!(differing.is_empty(), "{what}: bytes {differing:?} differ");
}

/// Under the default policy the plane's write is merged back, no texel copied back; under
/// `CopyBack` it is copied back, the texels inside the pass's scissor alone, with no merge; and
/// both leave the same bytes. A texture over host pages that is no plane's still merges back under
/// `CopyBack`.
#[test]
fn a_plane_is_merged_back_by_default_and_copied_back_under_copy_back() {
    let Some((merges, copied, bytes)) = pass(SurfaceWriteBackPolicy::default(), true) else {
        return;
    };
    assert!(merges > 0, "the default merges the plane back");
    assert_eq!(copied, 0, "and copies no texel back");
    assert_drawn(&bytes, "merged");

    let (merges, copied, bytes) = pass(SurfaceWriteBackPolicy::CopyBack, true).unwrap();
    assert_eq!(merges, 0, "CopyBack plans no merge");
    assert_eq!(copied, WIDTH * DRAWN, "it copies back the scissor's texels");
    assert_drawn(&bytes, "copied back");

    let (merges, copied, bytes) = pass(SurfaceWriteBackPolicy::CopyBack, false).unwrap();
    assert!(merges > 0, "no plane's texture merges back under CopyBack");
    assert_eq!(copied, 0);
    assert_drawn(&bytes, "no plane's, merged");
}
