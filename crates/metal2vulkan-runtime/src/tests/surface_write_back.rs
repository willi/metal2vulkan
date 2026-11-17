//! A pass's write through a shadowed texture over an IOSurface plane the CPU may write goes back by
//! the device's `SurfaceWriteBackPolicy` (this tree's own: `FORK.md`): under `Merge`, the default,
//! it is kept and merged back (`shadow_merge.rs`); under `CopyBack` a tracked texture's, as this
//! one is, is copied back, with no merge. With no other writer beside the pass, both leave the same
//! bytes: the pass's texels inside its scissor, and the plane's other texels and the bytes between
//! its rows as the CPU left them. A texture over the same host pages that is no plane's merges back
//! under `CopyBack` too: the policy reaches planes alone. And a plane copied back is still copied
//! in for each command buffer: the CPU's write between two command buffers reaches the second.
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
    Buffer, CommandQueue, Device, IoSurfacePlane, LoadAction, PipelineOptions,
    RenderPassDescriptor, RenderPipelineState, ScissorRect, StoreAction, Texture,
    TextureDescriptor,
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
/// Every byte of the host pages as the CPU writes them between two command buffers.
const BETWEEN: u8 = 0x55;

/// A device whose policy is `policy`, a texture of the plane over host pages (a plain texture
/// over the same bytes where not `plane`) and the pass's pipeline. Its fields drop in order, the
/// host pages last, after the device.
struct Plane {
    queue: CommandQueue,
    solid: RenderPipelineState,
    texture: Texture,
    _buffer: Buffer,
    device: Device,
    _scratch: Scratch,
    host: Host,
}

/// What a pass left: the merge dispatches it planned, the texels it copied back, and the texels
/// of the plane its command buffer copied in first.
struct Counted {
    merges: u64,
    copied: u64,
    first: u64,
}

impl Plane {
    /// `None` where the device imports no host memory.
    fn new(policy: SurfaceWriteBackPolicy, plane: bool) -> Option<Plane> {
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
        Some(Plane {
            queue,
            solid,
            texture,
            _buffer: buffer,
            device,
            _scratch: scratch,
            host,
        })
    }

    /// One command buffer of one pass that loads and stores the texture and draws `PAINT` inside a
    /// scissor of its first `DRAWN` rows, in the texels left of `right` (normalized, -1 to 1).
    fn pass(&self, right: f32) -> Counted {
        let before = self.device.work_counts();
        let command_buffer = self.queue.command_buffer().unwrap();
        let mut descriptor = RenderPassDescriptor::new();
        let attachment = &mut descriptor.color_attachments[0];
        attachment.texture = Some(self.texture.clone());
        attachment.load_action = LoadAction::Load;
        attachment.store_action = StoreAction::Store;
        let mut encoder = command_buffer.render_command_encoder(&descriptor).unwrap();
        encoder.set_render_pipeline_state(&self.solid).unwrap();
        let scissor = ScissorRect {
            x: 0,
            y: 0,
            width: WIDTH,
            height: DRAWN,
        };
        encoder.set_scissor_rect(scissor).unwrap();
        let corners = bytes_of(&[-1.0f32, -1.0, right, 1.0], f32::to_le_bytes);
        encoder.set_vertex_bytes(&corners, 0).unwrap();
        let color = bytes_of(&[f32::from(PAINT) / 255.0; 4], f32::to_le_bytes);
        encoder.set_fragment_bytes(&color, 0).unwrap();
        (encoder.draw_primitives(PrimitiveType::Triangle, 0, 6, 1, 0)).unwrap();
        encoder.end_encoding().unwrap();
        command_buffer.commit().unwrap();
        command_buffer.wait_until_completed_for(WAIT).unwrap();
        assert_eq!(command_buffer.error(), None);
        let after = self.device.work_counts();
        Counted {
            merges: after.shadow_merge_dispatches_planned - before.shadow_merge_dispatches_planned,
            copied: after.shadow_texels_copied_back - before.shadow_texels_copied_back,
            first: after.shadow_texels_first_copied_in_surfaces
                - before.shadow_texels_first_copied_in_surfaces,
        }
    }

    /// The plane's bytes.
    fn bytes(&self) -> Vec<u8> {
        let words = &self.host.words()[..(PITCH * HEIGHT / 4) as usize];
        words.iter().flat_map(|word| word.to_ne_bytes()).collect()
    }
}

/// That `bytes` are the plane's after passes over a plane whose bytes were `under`: its first
/// `DRAWN` rows' texels left of column `drawn` `PAINT`, every other byte `under`.
fn assert_drawn(bytes: &[u8], under: u8, drawn: u64, what: &str) {
    let mut expected = vec![under; (PITCH * HEIGHT) as usize];
    for row in expected.chunks_mut(PITCH as usize).take(DRAWN as usize) {
        row[..(drawn * 4) as usize].fill(PAINT);
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
    let Some(merged) = Plane::new(SurfaceWriteBackPolicy::default(), true) else {
        return;
    };
    let counted = merged.pass(1.0);
    assert!(counted.merges > 0, "the default merges the plane back");
    assert_eq!(counted.copied, 0, "and copies no texel back");
    assert_drawn(&merged.bytes(), BEFORE, WIDTH, "merged");

    let copied = Plane::new(SurfaceWriteBackPolicy::CopyBack, true).unwrap();
    let counted = copied.pass(1.0);
    assert_eq!(counted.merges, 0, "CopyBack plans no merge");
    assert_eq!(
        counted.copied,
        WIDTH * DRAWN,
        "it copies back the scissor's texels"
    );
    assert_drawn(&copied.bytes(), BEFORE, WIDTH, "copied back");

    let other = Plane::new(SurfaceWriteBackPolicy::CopyBack, false).unwrap();
    let counted = other.pass(1.0);
    assert!(
        counted.merges > 0,
        "no plane's texture merges back under CopyBack"
    );
    assert_eq!(counted.copied, 0);
    assert_drawn(&other.bytes(), BEFORE, WIDTH, "no plane's, merged");
}

/// Under `CopyBack` a plane is copied in again for each command buffer: the CPU's write between
/// two reaches the second, whose copy back moves every texel of its scissor, the half its draw
/// leaves too. An image kept from the first command buffer would put the bytes the CPU replaced
/// back there.
#[test]
fn a_plane_copied_back_is_copied_in_for_each_command_buffer() {
    let Some(copied) = Plane::new(SurfaceWriteBackPolicy::CopyBack, true) else {
        return;
    };
    let half = WIDTH / 2;
    let counted = copied.pass(0.0);
    assert_eq!((counted.merges, counted.copied), (0, WIDTH * DRAWN));
    assert_drawn(&copied.bytes(), BEFORE, half, "the first command buffer");
    copied.host.fill(u32::from_ne_bytes([BETWEEN; 4]));
    let counted = copied.pass(0.0);
    assert!(
        counted.first > 0,
        "the second command buffer copies the plane in"
    );
    assert_eq!((counted.merges, counted.copied), (0, WIDTH * DRAWN));
    assert_drawn(&copied.bytes(), BETWEEN, half, "the second command buffer");
}
