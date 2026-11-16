//! The watchdog's bound as a device policy (`DevicePolicy::watchdog`, this tree's own: FORK.md).
//!
//! - Under the default policy, with no test's bound in its place, a front that does not advance
//!   fails as Metal's timed-out command buffer (`GpuHang`) no sooner than the default bound.
//! - Under a policy of no bound (`None`), a front held past the default bound completes once what
//!   it waits for is signalled, with no timeout, no stop and no recovery.
//!
//! Neither sets the stall clock's test bound, so a device with a staging queue sends a probe at the
//! clock's own 10 s at most, and a device without one (lavapipe) sends none.

use super::*;
use metal2vulkan::device::DevicePolicy;

/// The default policy's bound.
fn default_bound() -> Duration {
    DevicePolicy::UNTRUSTED
        .watchdog
        .expect("the default policy has a bound")
}

/// The device the cfg selects under `policy`, with no test's hook set.
fn policy_device(cache: &Path, policy: DevicePolicy) -> Device {
    let options = DeviceOptions {
        kinds: match cfg!(m2v_cpu_icd) {
            true => vec![DeviceKind::Cpu],
            false => vec![
                DeviceKind::Integrated,
                DeviceKind::Discrete,
                DeviceKind::Virtual,
            ],
        },
        shader_cache: ShaderCacheOptions {
            directory: Some(cache.to_owned()),
            ..ShaderCacheOptions::default()
        },
        policy,
        ..DeviceOptions::default()
    };
    Device::system_default_with(&options).unwrap()
}

/// Under the default policy a front that does not advance fails as `GpuHang` no sooner than the
/// default bound, and counts as the watchdog's.
#[test]
fn the_default_policy_s_watchdog_fails_a_front_at_its_bound() {
    let bound = default_bound();
    let scratch = Scratch::new();
    let device = policy_device(&scratch.0, DevicePolicy::UNTRUSTED);
    // SAFETY: the functions of the generation that stops, used only to signal the event's
    // semaphore below, on the parked `VkDevice`, which is never destroyed.
    let parked = unsafe { device.raw() }.device.clone();
    let event = device.new_event().unwrap();
    // SAFETY: the stopping generation's semaphore of the event, signalled once from the host to
    // the reserved counter below.
    let semaphore = unsafe { event.vk_semaphore() };
    let buffer = device.new_buffer(4096, ResourceOptions::shared()).unwrap();
    let began = Instant::now();
    let (command_buffer, counter) = stuck(&device, &event, &buffer);
    assert!(
        command_buffer
            .wait_until_completed_for(bound + WAIT)
            .is_ok(),
        "the stuck command buffer did not fail in {:?}",
        bound + WAIT
    );
    let failed = began.elapsed();
    let counts = device.work_counts();
    eprintln!(
        "STALL-WATCHDOG-DEFAULT {}: failed after {failed:?}, the bound {bound:?}; \
         watchdog timeouts {}",
        device.name(),
        counts.watchdog_timeouts,
    );
    assert_eq!(
        (command_buffer.status(), command_buffer.error()),
        (
            CommandBufferStatus::Error,
            Some(CommandBufferError::GpuHang)
        )
    );
    assert!(failed >= bound, "failed after {failed:?}, before the bound");
    assert_eq!(counts.watchdog_timeouts, 1);
    let info = vk::SemaphoreSignalInfo::default()
        .semaphore(semaphore)
        .value(counter);
    // SAFETY: the parked `VkDevice` and its semaphore, both live (never destroyed); the counter
    // the reservation named, which nothing before it waits for.
    unsafe { parked.signal_semaphore(&info) }.unwrap();
}

/// Under a policy of no bound a front held past the default bound completes once its wait is
/// signalled: no timeout, no stop and no recovery.
#[test]
fn a_policy_of_no_watchdog_fails_no_front() {
    // Past the default bound by more than a waiter's slices on a loaded host, so a `None` read as
    // the default would fail the front before the signal.
    let held = default_bound() + Duration::from_secs(3);
    let scratch = Scratch::new();
    let mut policy = DevicePolicy::UNTRUSTED;
    policy.watchdog = None;
    let device = policy_device(&scratch.0, policy);
    let event = device.new_event().unwrap();
    let buffer = device.new_buffer(4096, ResourceOptions::shared()).unwrap();
    let (command_buffer, counter) = stuck(&device, &event, &buffer);
    let from = Instant::now();
    assert!(
        command_buffer.wait_until_completed_for(held).is_err(),
        "the held command buffer ended in {held:?}, before its wait was signalled"
    );
    let waited = from.elapsed();
    assert!(
        waited >= held,
        "the wait ended after {waited:?}, not {held:?}"
    );
    // SAFETY: the event's semaphore, signalled once from the host to the reserved counter, which
    // nothing before it waits for.
    let semaphore = unsafe { event.vk_semaphore() };
    let info = vk::SemaphoreSignalInfo::default()
        .semaphore(semaphore)
        .value(counter);
    // SAFETY: as above; the device is live.
    unsafe { device.shared().vk().signal_semaphore(&info) }.unwrap();
    assert!(command_buffer.wait_until_completed_for(WAIT).is_ok());
    let counts = device.work_counts();
    eprintln!(
        "STALL-NO-WATCHDOG {}: held {waited:?}; watchdog timeouts {}, wedges {}, recoveries {}",
        device.name(),
        counts.watchdog_timeouts,
        counts.wedges_declared,
        counts.device_recoveries,
    );
    assert_eq!(
        (command_buffer.status(), command_buffer.error()),
        (CommandBufferStatus::Completed, None)
    );
    assert_eq!(counts.watchdog_timeouts, 0);
    assert_eq!(counts.wedges_declared, 0);
    assert_eq!(counts.device_recoveries, 0);
}
