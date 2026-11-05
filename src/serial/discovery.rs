use std::{
    collections::{HashMap, HashSet},
    fs,
    ops::RangeInclusive,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

use super::{PortInfo, PortKind};

pub(super) fn scan(ports: Vec<serialport::SerialPortInfo>) -> Vec<PortInfo> {
    let drivers = fs::read_to_string("/proc/tty/drivers").unwrap_or_default();
    let known = ports
        .into_iter()
        .map(|port| {
            (
                PathBuf::from(port.port_name),
                PortKind::from_serialport(&port.port_type),
            )
        })
        .collect();
    collect_ports(
        &known,
        Path::new("/dev"),
        Path::new("/sys/class/tty"),
        &drivers,
    )
}

pub(super) fn device_id(path: &Path) -> Option<u64> {
    let node = fs::metadata(path).ok()?;
    node.file_type().is_char_device().then(|| node.rdev())
}

fn collect_ports(
    known: &HashMap<PathBuf, PortKind>,
    dev: &Path,
    tty_class: &Path,
    drivers: &str,
) -> Vec<PortInfo> {
    let ranges: Vec<_> = drivers.lines().filter_map(serial_range).collect();
    let onboard = entries(tty_class)
        .filter(|entry| is_serial_tty(&entry.path(), &ranges))
        .map(|entry| dev.join(entry.file_name()));
    let mut nodes: Vec<_> = known.keys().cloned().chain(onboard).collect();
    nodes.sort_unstable();

    // Prefer a stable device identifier, then a physical connector, then the kernel name.
    let mut seen = HashSet::new();
    ["serial/by-id", "serial/by-path"]
        .into_iter()
        .flat_map(|dir| {
            let mut aliases: Vec<_> = entries(&dev.join(dir)).map(|entry| entry.path()).collect();
            aliases.sort_unstable();
            aliases
        })
        .chain(nodes)
        .filter_map(|path| {
            let target = path.canonicalize().ok()?;
            let id = device_id(&target)?;
            let tty = tty_class.join(target.file_name()?);
            if uart_type(&tty) == Some(0) {
                return None;
            }
            let kind = known.get(&target).copied().unwrap_or(PortKind::Unknown);
            let path = path.into_os_string().into_string().ok()?;
            seen.insert(id).then(|| PortInfo {
                node: target.to_str().unwrap_or(&path).into(),
                path,
                kind: if kind == PortKind::Unknown {
                    sysfs_kind(&tty, &ranges)
                } else {
                    kind
                },
            })
        })
        .collect()
}

fn sysfs_kind(tty: &Path, ranges: &[(u32, RangeInclusive<u32>)]) -> PortKind {
    let Ok(device) = tty.canonicalize() else {
        return PortKind::Unknown;
    };
    for ancestor in device.ancestors() {
        let Ok(subsystem) = fs::read_link(ancestor.join("subsystem")) else {
            continue;
        };
        match subsystem.file_name().and_then(|name| name.to_str()) {
            Some("usb" | "usb-serial") => return PortKind::Usb,
            Some("pci") => return PortKind::Pci,
            Some("platform" | "amba") if is_serial_tty(tty, ranges) => {
                return PortKind::OnboardUart;
            }
            Some("i2c" | "spi") => break,
            _ => {}
        }
    }
    PortKind::Unknown
}

fn entries(path: &Path) -> impl Iterator<Item = fs::DirEntry> {
    fs::read_dir(path).into_iter().flatten().flatten()
}

fn uart_type(tty: &Path) -> Option<u32> {
    fs::read_to_string(tty.join("type"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn is_serial_tty(tty: &Path, ranges: &[(u32, RangeInclusive<u32>)]) -> bool {
    // Serial core exposes the UART type. Type zero is PORT_UNKNOWN (an empty slot).
    if let Some(kind) = uart_type(tty) {
        return kind != 0;
    }
    let Ok(device) = fs::read_to_string(tty.join("dev")) else {
        return false;
    };
    let Some((major, minor)) = device.trim().split_once(':') else {
        return false;
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return false;
    };
    ranges
        .iter()
        .any(|(driver, minors)| *driver == major && minors.contains(&minor))
}

fn serial_range(line: &str) -> Option<(u32, RangeInclusive<u32>)> {
    let mut fields = line.split_whitespace().rev();
    if fields.next()? != "serial" {
        return None;
    }
    let minors = fields.next()?;
    let (first, last) = minors.split_once('-').unwrap_or((minors, minors));
    let major = fields.next()?.parse().ok()?;
    Some((major, first.parse().ok()?..=last.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn discovers_onboard_ports_and_merges_device_aliases_without_opening_nodes() {
        let root = tempfile::tempdir().unwrap();
        let dev = root.path().join("dev");
        let tty_class = root.path().join("tty");
        let by_id = dev.join("serial/by-id");
        let by_path = dev.join("serial/by-path");
        fs::create_dir_all(&by_id).unwrap();
        fs::create_dir_all(&by_path).unwrap();

        for (name, node, number, kind) in [
            ("ttyS4", "/dev/null", "4:68", None),
            ("ttyS5", "/dev/full", "4:69", Some("0")),
            ("ttyAMA0", "/dev/random", "204:64", Some("32")),
            ("tty0", "/dev/tty", "4:0", None),
            ("ttyUSB0", "/dev/zero", "188:0", None),
            ("ttyACM0", "/dev/urandom", "166:0", None),
        ] {
            let tty = tty_class.join(name);
            fs::create_dir_all(&tty).unwrap();
            fs::write(tty.join("dev"), number).unwrap();
            if let Some(kind) = kind {
                fs::write(tty.join("type"), kind).unwrap();
            }
            symlink(node, dev.join(name)).unwrap();
        }
        symlink("../../ttyUSB0", by_id.join("usb-adapter")).unwrap();
        symlink("../../ttyUSB0", by_path.join("usb-connector")).unwrap();
        symlink("../../ttyACM0", by_path.join("acm-connector")).unwrap();
        symlink("../../missing", by_id.join("unplugged")).unwrap();
        fs::write(by_id.join("not-a-device"), "").unwrap();
        let drivers = "serial /dev/ttyS 4 64-95 serial\n\
                       console /dev/tty 4 0 console\n\
                       usbserial /dev/ttyUSB 188 0 serial\n\
                       acm /dev/ttyACM 166 0-255 serial";

        let known = HashMap::from([(dev.join("ttyUSB0"), PortKind::Usb)]);
        let ports = collect_ports(&known, &dev, &tty_class, drivers);
        assert_eq!(
            ports.into_iter().map(|port| port.path).collect::<Vec<_>>(),
            [
                by_id.join("usb-adapter"),
                by_path.join("acm-connector"),
                dev.join("ttyAMA0"),
                dev.join("ttyS4"),
            ]
            .map(|path| path.into_os_string().into_string().unwrap())
        );
    }
}
