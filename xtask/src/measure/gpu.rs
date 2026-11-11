//! GPU memory a process holds, as the kernel's DRM drivers report it per
//! client in `/proc/<pid>/fdinfo` (`drm-total-vram`, `drm-resident-gtt`,
//! ...; drivers such as amdgpu, i915/xe and msm report it).
//!
//! A process may hold several descriptors of one DRM client (Vulkan opens
//! the render node more than once); each client is counted once, by its
//! `drm-client-id`. This memory is not in the process's resident set: on
//! a GPU that shares system RAM, "VRAM" is RAM set aside for the GPU.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// One DRM client's memory, in KiB, by region (`vram`, `gtt`, `cpu`, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Client {
    /// The kernel driver (`amdgpu`).
    pub(crate) driver: String,
    /// `drm-total-<region>`: memory of every buffer the client has a handle to.
    pub(crate) total: BTreeMap<String, u64>,
    /// `drm-resident-<region>`.
    pub(crate) resident: BTreeMap<String, u64>,
}

/// Reads the DRM clients behind `pid`'s descriptors, by client id.
pub(crate) fn read(pid: u32) -> BTreeMap<String, Client> {
    let dir = Path::new("/proc").join(pid.to_string());
    let Ok(entries) = fs::read_dir(dir.join("fd")) else {
        return BTreeMap::new();
    };
    let mut texts = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let is_drm = fs::read_link(entry.path()).is_ok_and(|target| target.starts_with("/dev/dri"));
        if !is_drm {
            continue;
        }
        if let Ok(text) = fs::read_to_string(dir.join("fdinfo").join(entry.file_name())) {
            texts.push(text);
        }
    }
    clients(texts.iter().map(String::as_str))
}

/// The clients in the `fdinfo` texts of a process's DRM descriptors, one
/// entry per `drm-client-id`. A text without a client id (a driver that
/// reports no usage) is skipped.
pub(crate) fn clients<'a>(texts: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, Client> {
    let mut clients = BTreeMap::new();
    for text in texts {
        let mut id = None;
        let mut client = Client::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if key == "drm-client-id" {
                id = Some(value.to_owned());
            } else if key == "drm-driver" {
                value.clone_into(&mut client.driver);
            } else if let Some(region) = key.strip_prefix("drm-total-") {
                if let Some(kib) = kib(value) {
                    client.total.insert(region.to_owned(), kib);
                }
            } else if let Some(region) = key.strip_prefix("drm-resident-")
                && let Some(kib) = kib(value)
            {
                client.resident.insert(region.to_owned(), kib);
            }
        }
        if let Some(id) = id {
            clients.entry(id).or_insert(client);
        }
    }
    clients
}

/// Memory summed over `clients`, by region: `(total, resident)` KiB.
pub(crate) fn sum(clients: &BTreeMap<String, Client>) -> BTreeMap<String, (u64, u64)> {
    let mut regions: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for client in clients.values() {
        for (region, kib) in &client.total {
            regions.entry(region.clone()).or_default().0 += kib;
        }
        for (region, kib) in &client.resident {
            regions.entry(region.clone()).or_default().1 += kib;
        }
    }
    regions.retain(|_, (total, resident)| *total > 0 || *resident > 0);
    regions
}

/// A size as fdinfo writes it: `1234 KiB`, `5 MiB`, or a bare byte count.
fn kib(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        None => Some(number.div_ceil(1024)),
        Some("KiB") => Some(number),
        Some("MiB") => Some(number * 1024),
        Some("GiB") => Some(number * 1024 * 1024),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMDGPU: &str = "pos:\t0\nflags:\t02100002\ndrm-driver:\tamdgpu\n\
        drm-client-id:\t1963\ndrm-pdev:\t0000:c7:00.0\ndrm-total-cpu:\t0\n\
        drm-total-gtt:\t14680 KiB\ndrm-resident-gtt:\t80988 KiB\n\
        drm-total-vram:\t195872 KiB\ndrm-shared-vram:\t1956 KiB\n\
        drm-resident-vram:\t129564 KiB\ndrm-total-gds:\t0\n";

    #[test]
    fn one_client_behind_two_descriptors_counts_once() {
        let clients = clients([AMDGPU, AMDGPU]);
        assert_eq!(clients.len(), 1);
        let client = &clients["1963"];
        assert_eq!(client.driver, "amdgpu");
        assert_eq!(client.total["vram"], 195_872);
        assert_eq!(client.resident["gtt"], 80_988);
        let regions = sum(&clients);
        assert_eq!(regions["vram"], (195_872, 129_564));
        assert_eq!(regions["gtt"], (14_680, 80_988));
        assert!(!regions.contains_key("gds"), "empty regions are left out");
        assert!(!regions.contains_key("cpu"));
    }

    #[test]
    fn two_clients_add_up() {
        let other = AMDGPU.replace("1963", "2000");
        let regions = sum(&clients([AMDGPU, other.as_str()]));
        assert_eq!(regions["vram"].0, 2 * 195_872);
    }

    #[test]
    fn a_descriptor_without_a_client_is_skipped() {
        assert!(clients(["pos:\t0\nflags:\t02\n"]).is_empty());
    }

    #[test]
    fn sizes_in_every_unit() {
        assert_eq!(kib("14680 KiB"), Some(14_680));
        assert_eq!(kib("3 MiB"), Some(3072));
        assert_eq!(kib("1 GiB"), Some(1_048_576));
        assert_eq!(kib("1025"), Some(2), "bytes round up");
        assert_eq!(kib("0"), Some(0));
        assert_eq!(kib("12 parsecs"), None);
    }
}
