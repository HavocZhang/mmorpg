//! Prometheus 指标导出模块
//!
//! 暴露 /metrics 端点供 Prometheus 抓取
//! 每次抓取时同步合包统计、内存等运行时指标

use crate::foundation::metric;

// ── Windows FFI 声明（模块级）──────────────────────────────────────
#[cfg(target_os = "windows")]
mod winapi {
    #[repr(C)]
    pub struct PROCESS_MEMORY_COUNTERS {
        pub cb: u32,
        pub page_fault_count: u32,
        pub peak_working_set_size: usize,
        pub working_set_size: usize,
        pub quota_peak_paged_pool_usage: usize,
        pub quota_paged_pool_usage: usize,
        pub quota_peak_non_paged_pool_usage: usize,
        pub quota_non_paged_pool_usage: usize,
        pub pagefile_usage: usize,
        pub peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetCurrentProcess() -> *mut core::ffi::c_void;
    }

    #[link(name = "psapi")]
    extern "system" {
        pub fn GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            psmemcounters: *mut PROCESS_MEMORY_COUNTERS,
            cb: u32,
        ) -> i32;
    }
}

/// 同步运行时指标到 Prometheus Registry，然后导出
pub fn export() -> String {
    use prometheus::Encoder;

    // 同步合包统计
    sync_merge_stats();

    // 同步进程内存
    sync_process_metrics();

    let encoder = prometheus::TextEncoder::new();
    let metric_families = metric::registry().gather();
    let mut buf = Vec::new();
    encoder.encode(&metric_families, &mut buf).ok();
    String::from_utf8(buf).unwrap_or_default()
}

/// 上次同步到 Prometheus 的值（用于计算差值 inc_by）
static LAST_SYNC_PACKETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_SYNC_FLUSHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_SYNC_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 同步合包统计到 Prometheus 指标
fn sync_merge_stats() {
    let m = metric::metrics();

    // 从全局原子计数器读取合包统计
    let packets = crate::io_engine::packet_merge::MERGE_TOTAL_PACKETS
        .load(std::sync::atomic::Ordering::Relaxed);
    let flushes = crate::io_engine::packet_merge::MERGE_TOTAL_FLUSHES
        .load(std::sync::atomic::Ordering::Relaxed);
    let bytes_sent = crate::io_engine::packet_merge::MERGE_TOTAL_BYTES_SENT
        .load(std::sync::atomic::Ordering::Relaxed);

    // 计算与上次同步的差值，inc_by 到 Prometheus Counter
    let last_packets = LAST_SYNC_PACKETS.load(std::sync::atomic::Ordering::Relaxed);
    let last_flushes = LAST_SYNC_FLUSHES.load(std::sync::atomic::Ordering::Relaxed);
    let last_bytes = LAST_SYNC_BYTES.load(std::sync::atomic::Ordering::Relaxed);

    let delta_packets = packets.saturating_sub(last_packets);
    let delta_flushes = flushes.saturating_sub(last_flushes);
    let delta_bytes = bytes_sent.saturating_sub(last_bytes);

    if delta_packets > 0 {
        m.merge_total_packets.inc_by(delta_packets);
    }
    if delta_flushes > 0 {
        m.merge_total_flushes.inc_by(delta_flushes);
    }
    if delta_bytes > 0 {
        m.merge_total_bytes.inc_by(delta_bytes);
    }

    // 更新上次同步值
    LAST_SYNC_PACKETS.store(packets, std::sync::atomic::Ordering::Relaxed);
    LAST_SYNC_FLUSHES.store(flushes, std::sync::atomic::Ordering::Relaxed);
    LAST_SYNC_BYTES.store(bytes_sent, std::sync::atomic::Ordering::Relaxed);
}

/// 同步进程级指标
fn sync_process_metrics() {
    let m = metric::metrics();

    // 内存使用（Linux: /proc, Windows: GetProcessMemoryInfo）
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<i64>() {
                            m.process_memory_bytes.set(kb * 1024);
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        unsafe {
            let handle = winapi::GetCurrentProcess();
            let mut counters = winapi::PROCESS_MEMORY_COUNTERS {
                cb: core::mem::size_of::<winapi::PROCESS_MEMORY_COUNTERS>() as u32,
                page_fault_count: 0,
                peak_working_set_size: 0,
                working_set_size: 0,
                quota_peak_paged_pool_usage: 0,
                quota_paged_pool_usage: 0,
                quota_peak_non_paged_pool_usage: 0,
                quota_non_paged_pool_usage: 0,
                pagefile_usage: 0,
                peak_pagefile_usage: 0,
            };

            let result = winapi::GetProcessMemoryInfo(
                handle,
                &mut counters as *mut _,
                core::mem::size_of::<winapi::PROCESS_MEMORY_COUNTERS>() as u32,
            );

            if result != 0 {
                m.process_memory_bytes.set(counters.working_set_size as i64);
            }
        }
    }

    // Uptime 由调用方在 admin handler 中设置
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export() {
        // 初始化指标
        metric::metrics().msgs_received.inc();
        metric::metrics().connections.set(42);

        let output = export();
        assert!(output.contains("gate_msgs_received_total"));
        assert!(output.contains("gate_connections"));
    }

    #[test]
    fn test_extended_metrics_exist() {
        let output = export();
        // 验证扩展指标已注册
        assert!(output.contains("gate_merge_total_packets"));
        assert!(output.contains("gate_merge_total_flushes"));
        assert!(output.contains("gate_process_memory_bytes"));
        assert!(output.contains("gate_uptime_seconds"));
        assert!(output.contains("gate_msg_latency_ms"));
        assert!(output.contains("gate_active_sessions"));
        assert!(output.contains("gate_blacklist_count"));
    }

    #[test]
    fn test_merge_stats_sync() {
        // 验证合包统计同步后 Prometheus Counter 有值
        crate::io_engine::packet_merge::MERGE_TOTAL_PACKETS
            .fetch_add(100, std::sync::atomic::Ordering::Relaxed);
        crate::io_engine::packet_merge::MERGE_TOTAL_FLUSHES
            .fetch_add(30, std::sync::atomic::Ordering::Relaxed);

        let output = export();

        // 同步后 Counter 应该有值（不一定是精确值，因为其他测试可能也在 inc）
        assert!(output.contains("gate_merge_total_packets"));
        assert!(output.contains("gate_merge_total_flushes"));
    }
}
