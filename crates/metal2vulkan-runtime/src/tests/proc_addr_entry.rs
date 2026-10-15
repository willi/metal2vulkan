//! A device made through a caller's `vkGetInstanceProcAddr`, opening no loader itself: how a
//! process that cannot `dlopen` the loader (one bridged into another binary format) gets a device.

use crate::{Device, DeviceKind, DeviceOptions};
use ash::vk;

/// The `vkGetInstanceProcAddr` of the platform's loader, opened here rather than by the runtime.
fn resolved_proc_addr() -> vk::PFN_vkGetInstanceProcAddr {
    // SAFETY: the loader is opened once and never closed, so the pointer stays valid.
    unsafe {
        let library = libc::dlopen(c"libvulkan.so.1".as_ptr(), libc::RTLD_NOW);
        assert!(!library.is_null(), "libvulkan.so.1 does not open");
        let symbol = libc::dlsym(library, c"vkGetInstanceProcAddr".as_ptr());
        assert!(!symbol.is_null(), "no vkGetInstanceProcAddr");
        std::mem::transmute::<*mut libc::c_void, vk::PFN_vkGetInstanceProcAddr>(symbol)
    }
}

#[test]
fn a_device_is_made_through_the_callers_proc_addr_and_no_loader_is_opened() {
    // A loader that cannot open: were it tried, creation would fail with `LoaderNotFound`.
    let options = DeviceOptions {
        loader: Some("/nonexistent/m2v/libvulkan.so.1".into()),
        get_instance_proc_addr: Some(resolved_proc_addr()),
        kinds: match cfg!(m2v_cpu_icd) {
            true => vec![DeviceKind::Cpu],
            false => vec![],
        },
        ..DeviceOptions::default()
    };
    let device = Device::system_default_with(&options).unwrap();
    assert!(!device.name().is_empty());
}
