//! Native macOS physical memory accounting, without subprocesses or root access.
//! Anonymous non-purgeable pages + wired pages + physical compressor pages are
//! used memory. File-backed/purgeable cache is reported separately. This is an
//! Activity Monitor-style estimate, not a sum of process RSS (shared pages).
use anyhow::{ensure, Result};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug)]
pub struct Pages {
    pub size: u64,
    pub internal: u64,
    pub purgeable: u64,
    pub wired: u64,
    pub compressed: u64,
    pub external: u64,
}

pub fn pressure(level: Option<i32>) -> Option<&'static str> {
    match level {
        Some(1) => Some("normal"),
        Some(2) => Some("warning"),
        Some(4) => Some("critical"),
        _ => None,
    }
}

pub fn accounting(total: u64, pages: Pages, level: Option<i32>) -> Result<Value> {
    ensure!(
        total > 0 && matches!(pages.size, 4096 | 16384),
        "Invalid macOS memory sample"
    );
    let bytes = |n: u64| {
        n.checked_mul(pages.size)
            .ok_or_else(|| anyhow::anyhow!("Memory counter overflow"))
    };
    // Purgeable counters can race anonymous counters; use a saturating subtraction.
    let app = bytes(pages.internal.saturating_sub(pages.purgeable))?;
    let wired = bytes(pages.wired)?;
    let compressed = bytes(pages.compressed)?;
    let used = app
        .checked_add(wired)
        .and_then(|v| v.checked_add(compressed))
        .filter(|v| *v <= total)
        .ok_or_else(|| anyhow::anyhow!("Inconsistent macOS memory sample"))?;
    let cache = bytes(
        pages
            .external
            .checked_add(pages.purgeable)
            .ok_or_else(|| anyhow::anyhow!("Memory counter overflow"))?,
    )?;
    ensure!(cache <= total, "Inconsistent macOS cache sample");
    Ok(json!({"mem_total_bytes":total,"mem_used_bytes":used,
        "mem_available_bytes":total-used,"mem_app_bytes":app,"mem_wired_bytes":wired,
        "mem_compressed_bytes":compressed,"mem_cache_bytes":cache,
        "memory_basis":"macos-native","memory_pressure":pressure(level)}))
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc's fixed Mach ABI; no extra runtime dependency.
pub fn sample() -> Result<Value> {
    use std::{ffi::CStr, mem};
    extern "C" {
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
    fn sysctl<T: Copy>(name: &CStr) -> Option<T> {
        let mut value = mem::MaybeUninit::<T>::uninit();
        let mut size = mem::size_of::<T>();
        // Fixed scalar/struct kernel ABI, never a user-provided sysctl name.
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (rc == 0 && size == mem::size_of::<T>()).then(|| unsafe { value.assume_init() })
    }
    let total = sysctl::<u64>(c"hw.memsize")
        .ok_or_else(|| anyhow::anyhow!("macOS memory total unavailable"))?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    ensure!(page_size > 0, "macOS page size unavailable");
    let mut stat: libc::vm_statistics64 = unsafe { mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let host = unsafe { libc::mach_host_self() };
    let rc = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&mut stat as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    // mach_host_self returns a send right on every sample. Release it even when
    // the statistics query fails, so long-running services do not leak ports.
    #[allow(deprecated)]
    unsafe {
        mach_port_deallocate(libc::mach_task_self(), host)
    };
    // New libc headers include statistics added after macOS 26. Older kernels
    // successfully return a shorter revision. Require only the complete stable
    // prefix that we read, rather than rejecting valid older OS statistics.
    let required_count = ((mem::offset_of!(libc::vm_statistics64, internal_page_count)
        + mem::size_of::<libc::natural_t>())
        / mem::size_of::<libc::integer_t>()) as u32;
    ensure!(
        rc == libc::KERN_SUCCESS && count >= required_count,
        "macOS VM statistics unavailable (result {rc}, count {count})"
    );
    let mut memory = accounting(
        total,
        Pages {
            size: page_size as u64,
            internal: stat.internal_page_count.into(),
            purgeable: stat.purgeable_count.into(),
            wired: stat.wire_count.into(),
            compressed: stat.compressor_page_count.into(),
            external: stat.external_page_count.into(),
        },
        sysctl::<i32>(c"kern.memorystatus_vm_pressure_level"),
    )?;
    let swap = sysctl::<libc::xsw_usage>(c"vm.swapusage").filter(|s| s.xsu_used <= s.xsu_total);
    memory["swap_total_bytes"] = json!(swap.as_ref().map(|s| s.xsu_total));
    memory["swap_used_bytes"] = json!(swap.as_ref().map(|s| s.xsu_used));
    Ok(memory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn native_query_succeeds() {
        sample().unwrap();
    }
    #[test]
    fn cache_and_compression_are_not_counted_twice() {
        for size in [4096, 16384] {
            let m = accounting(
                1000 * size,
                Pages {
                    size,
                    internal: 500,
                    purgeable: 50,
                    wired: 100,
                    compressed: 80,
                    external: 200,
                },
                Some(2),
            )
            .unwrap();
            assert_eq!(m["mem_app_bytes"], 450 * size);
            assert_eq!(m["mem_used_bytes"], 630 * size);
            assert_eq!(m["mem_cache_bytes"], 250 * size);
            assert_eq!(m["mem_available_bytes"], 370 * size);
            assert_eq!(m["memory_pressure"], "warning");
        }
    }
    #[test]
    fn invalid_and_missing_readings_stay_unknown() {
        assert_eq!(pressure(None), None);
        assert_eq!(pressure(Some(0)), None);
        assert_eq!(pressure(Some(4)), Some("critical"));
        let p = Pages {
            size: 4096,
            internal: 500,
            purgeable: 0,
            wired: 100,
            compressed: 80,
            external: 200,
        };
        assert!(accounting(4096, p, None).is_err());
        assert!(accounting(
            u64::MAX,
            Pages {
                internal: u64::MAX,
                ..p
            },
            None
        )
        .is_err());
        assert!(accounting(4096 * 1000, Pages { size: 0, ..p }, None).is_err());
        assert!(accounting(4096 * 1000, p, None).unwrap()["memory_pressure"].is_null());
    }
}
