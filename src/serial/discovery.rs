use std::{
    collections::HashSet,
    fs,
    ops::RangeInclusive,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

pub(super) fn scan(ports: Vec<String>) -> Vec<String> {
    let drivers = fs::read_to_string("/proc/tty/drivers").unwrap_or_default();
    collect_ports(
        ports.into_iter().map(PathBuf::from),
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
    known: impl Iterator<Item = PathBuf>,
    dev: &Path,
    tty_class: &Path,
    drivers: &str,
) -> Vec<String> {
    let ranges: Vec<_> = drivers.lines().filter_map(serial_range).collect();
    let onboard = entries(tty_class)
        .filter(|entry| is_serial_tty(&entry.path(), &ranges))
        .map(|entry| dev.join(entry.file_name()));
    let mut nodes: Vec<_> = known.chain(onboard).collect();
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
            if uart_type(&tty_class.join(target.file_name()?)) == Some(0) {
                return None;
            }
            let path = path.into_os_string().into_string().ok()?;
            seen.insert(id).then_some(path)
        })
        .collect()
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

        let ports = collect_ports([dev.join("ttyUSB0")].into_iter(), &dev, &tty_class, drivers);
        assert_eq!(
            ports,
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
