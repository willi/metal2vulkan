//! A kernel's `half` arithmetic on the device under `HalfRoundingPolicy::Exact`: each `float`
//! narrowed to `half` and their sum rounded back to `half` answer Metal's `half(float)` bit for bit
//! (to nearest even, a half denormal kept, an infinity from 65520 up, a zero's sign kept), whatever
//! the device's own conversion does. Under the default policy the answers are the device's
//! (`PrecisionNote::NativeHalfConversion` or `NativeHalfQuantize` records the difference): printed,
//! not checked.
//!
//! A device test: compiled only under the validation crate's GPU safety cfgs (`m2v_gpu`,
//! `m2v_cpu_icd`), on the kind of device the cfg selects.

use super::blit_holds::{options, Scratch, WAIT};
use crate::{CommandBufferStatus, Device, PipelineOptions, ResourceOptions, Size};
use metal2vulkan::device::HalfRoundingPolicy;

/// A kernel whose thread `i` narrows the two `float`s at `2i` and `2i + 1` to `half`, adds them in
/// `half`, and stores the sum widened over the first.
const SUMS: &str = r#"; Owned synthetic kernel.
source_filename = "half_rounding.metal"

define void @half_sums(ptr addrspace(1) noalias %io, i32 %i) {
entry:
  %j = zext i32 %i to i64
  %k = shl i64 %j, 1
  %pa = getelementptr inbounds float, ptr addrspace(1) %io, i64 %k
  %k1 = add i64 %k, 1
  %pb = getelementptr inbounds float, ptr addrspace(1) %io, i64 %k1
  %a = load float, ptr addrspace(1) %pa, align 4
  %b = load float, ptr addrspace(1) %pb, align 4
  %ha = fptrunc float %a to half
  %hb = fptrunc float %b to half
  %s = fadd half %ha, %hb
  %r = fpext half %s to float
  store float %r, ptr addrspace(1) %pa, align 4
  ret void
}

!air.kernel = !{!0}
!air.version = !{!30}
!air.language_version = !{!31}
!0 = !{ptr @half_sums, !1, !2}
!1 = !{}
!2 = !{!3, !4}
!3 = !{i32 0, !"air.buffer", !"air.location_index", i32 0, i32 1, !"air.read_write", !"air.address_space", i32 1, !"air.arg_type_size", i32 4, !"air.arg_type_align_size", i32 4, !"air.arg_type_name", !"float", !"air.arg_name", !"io"}
!4 = !{i32 1, !"air.thread_position_in_grid", !"air.arg_type_name", !"uint", !"air.arg_name", !"i"}
!30 = !{i32 2, i32 8, i32 0}
!31 = !{!"Metal", i32 4, i32 0, i32 0}
"#;

/// Each thread's two `float`s and Metal's `half(half(a) + half(b))`.
fn cases() -> [(f32, f32, f32); 8] {
    let ulp = |e: i32| 2f32.powi(e);
    [
        // `a` narrows up to 2: one `half` ulp below 2 is `2 - 2^-10`.
        (2.0 - ulp(-13), 0.0, 2.0),
        // A tie at the last bit of the sum, to even: down, then up.
        (1.0, ulp(-11), 1.0),
        (1.0, 3.0 * ulp(-11), 1.0 + ulp(-9)),
        // Below the tie at 65520, and at it.
        (65504.0, 8.0, 65504.0),
        (65504.0, 16.0, f32::INFINITY),
        // A half denormal, `3 · 2^-24`, from a value just above it.
        (3.0 * ulp(-24) + ulp(-27), 0.0, 3.0 * ulp(-24)),
        // `0x2E66`, the half nearest 0.1.
        (0.1, 0.0, f32::from_bits(0x3DCC_C000)),
        (-0.0, -0.0, -0.0),
    ]
}

/// [`SUMS`] over [`cases`] on `device`: each thread's stored sum.
fn sums(device: &Device) -> Vec<f32> {
    let air = metal2vulkan_devtools::llvm_assemble_text(SUMS).unwrap();
    let library = metal2vulkan::library::Library::from_air(&air).unwrap();
    let function = device
        .new_library_with_compile_library(library)
        .new_function_with_name("half_sums")
        .unwrap();
    let state = device
        .new_compute_pipeline_state_with_function(&function, &PipelineOptions::default())
        .unwrap();
    let cases = cases();
    let words: Vec<u8> = (cases.iter())
        .flat_map(|&(a, b, _)| [a, b])
        .flat_map(f32::to_le_bytes)
        .collect();
    let io = device
        .new_buffer(words.len() as u64, ResourceOptions::shared())
        .unwrap();
    let contents = io.contents().unwrap();
    // SAFETY: the new buffer's bytes, which no command uses yet.
    unsafe { std::ptr::copy_nonoverlapping(words.as_ptr(), contents.as_ptr(), words.len()) };
    let queue = device.new_command_queue().unwrap();
    let command_buffer = queue.command_buffer().unwrap();
    let mut encoder = command_buffer.compute_command_encoder().unwrap();
    encoder.set_compute_pipeline_state(&state).unwrap();
    encoder.set_buffer(Some(&io), 0, 0).unwrap();
    let threads = Size::new(cases.len() as u64, 1, 1);
    encoder.dispatch_threads(threads, threads).unwrap();
    encoder.end_encoding().unwrap();
    command_buffer.commit().unwrap();
    command_buffer.wait_until_completed_for(WAIT).unwrap();
    assert_eq!(command_buffer.status(), CommandBufferStatus::Completed);
    // SAFETY: the buffer's bytes, which the completed command buffer wrote.
    let bytes = unsafe { std::slice::from_raw_parts(contents.as_ptr(), words.len()) };
    (bytes.as_chunks::<8>().0.iter())
        .map(|pair| f32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]))
        .collect()
}

/// Under `HalfRoundingPolicy::Exact` every case answers Metal's `half`, bit for bit; under the
/// default, the device's answers are printed beside Metal's.
#[test]
fn metal_s_half_rounding_is_the_device_s_answer_under_exact() {
    for exact in [false, true] {
        let scratch = Scratch::new(if exact { "half-exact" } else { "half-native" });
        let mut options = options(&scratch.0);
        if exact {
            options.policy.half_rounding = HalfRoundingPolicy::Exact;
        }
        let device = Device::system_default_with(&options).unwrap();
        for (&(a, b, want), got) in cases().iter().zip(sums(&device)) {
            if exact {
                assert_eq!(
                    got.to_bits(),
                    want.to_bits(),
                    "half({a}) + half({b}): {got}"
                );
            } else {
                eprintln!("native: half({a}) + half({b}) = {got} (Metal's {want})");
            }
        }
    }
}
