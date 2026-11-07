//! Model for the launcher's Mirror view: turns what the media device list
//! reports into the rows the view lists and cycles through.
//!
//! A device arrives as `{ id, description, grey_only }`. `description` is the
//! V4L2 card name, which uvcvideo cuts at 31 bytes and usually writes as
//! "<product>: <interface>" ("ASUS FHD webcam: ASUS FHD webca" for the colour
//! sensor, "ASUS FHD webcam: ASUS IR camera" for the IR one, and "Integrated
//! Camera: Integrated I" on boards that truncate the interface name).
//! `grey_only` is true when every pixel format the device offers is Y8 or Y16,
//! which is how an IR sensor presents itself even when its card name says
//! nothing.

use fs_js as js;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static IR_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)(ir|infrared)(?-u:\b)").unwrap());
static TRAILING_DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([0-9]+)$").unwrap());

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Device {
    pub id: String,
    pub description: String,
    pub grey_only: bool,
}

pub(crate) fn is_ir(device: &Device) -> bool {
    device.grey_only || IR_WORD.is_match(&device.description)
}

/// The product half of a "<product>: <interface>" card name, unless the
/// interface half names the sensor itself (an IR one). The interface half is a
/// truncated copy of the product name often enough that using it would print
/// "ASUS FHD webca".
pub(crate) fn name(description: &str) -> String {
    let text = js::trim(description);
    let Some(at) = text.find(": ") else {
        return text.to_string();
    };
    let product = js::trim(&text[..at]);
    let iface = js::trim(&text[at + 2..]);
    if product.is_empty() {
        return iface.to_string();
    }
    if !iface.is_empty() && IR_WORD.is_match(iface) {
        return iface.to_string();
    }
    product.to_string()
}

pub fn label(description: &str, ir: bool) -> String {
    let mut text = name(description);
    if text.is_empty() {
        text = "Camera".into();
    }
    if ir && !IR_WORD.is_match(&text) {
        text.push_str(" IR");
    }
    text
}

/// /dev/video2 sorts after /dev/video10 as a string.
fn number(id: &str) -> f64 {
    TRAILING_DIGITS.captures(id).map_or(9_007_199_254_740_991.0, |m| js::parse_int(&m[1]))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub ir: bool,
    pub label: String,
}

/// Colour cameras first, then IR, each in node order, so the first row is the
/// one a mirror should open on. Two rows with the same label get a running
/// number, which keeps a pair of identical webcams apart.
pub fn rows(devices: &[Device]) -> Vec<Row> {
    let mut out: Vec<Row> = devices
        .iter()
        .map(|d| {
            let ir = is_ir(d);
            Row { id: d.id.clone(), ir, label: label(&d.description, ir) }
        })
        .collect();
    out.sort_by(|a, b| {
        a.ir.cmp(&b.ir).then_with(|| number(&a.id).partial_cmp(&number(&b.id)).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut seen: HashMap<String, u32> = HashMap::new();
    for row in &mut out {
        let count = seen.entry(row.label.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            row.label.push_str(&format!(" {count}"));
        }
    }
    out
}

pub(crate) fn index_of(list: &[Row], id: &str) -> Option<usize> {
    list.iter().position(|r| r.id == id)
}

/// The camera to show after the list changed: the current one while it is still
/// plugged in, otherwise the first row, "" when there is none.
pub fn pick(list: &[Row], current_id: &str) -> String {
    if index_of(list, current_id).is_some() {
        return current_id.to_string();
    }
    list.first().map_or_else(String::new, |r| r.id.clone())
}

/// The id `delta` places after `current_id`, wrapping both ways. A list of one
/// stays on its only camera.
pub fn step(list: &[Row], current_id: &str, delta: i64) -> String {
    let Some(first) = list.first() else {
        return String::new();
    };
    let Some(at) = index_of(list, current_id) else {
        return first.id.clone();
    };
    let len = list.len() as i64;
    let mut next = (at as i64 + delta) % len;
    if next < 0 {
        next += len;
    }
    list[next as usize].id.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two card names the g815's uvcvideo reports (v4l2-ctl on the machine,
    /// 2026-09-29): one USB device, a colour and an IR interface.
    fn asus_colour() -> Device {
        dev("/dev/video0", "ASUS FHD webcam: ASUS FHD webca", false)
    }

    fn asus_ir() -> Device {
        dev("/dev/video2", "ASUS FHD webcam: ASUS IR camera", true)
    }

    fn dev(id: &str, description: &str, grey_only: bool) -> Device {
        Device { id: id.into(), description: description.into(), grey_only }
    }

    fn ids(list: &[Row]) -> Vec<&str> {
        list.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn label_takes_the_product_when_the_interface_is_a_truncated_copy() {
        assert_eq!(label(&asus_colour().description, false), "ASUS FHD webcam");
    }

    #[test]
    fn label_takes_the_interface_when_it_names_the_ir_sensor() {
        assert_eq!(label(&asus_ir().description, true), "ASUS IR camera");
    }

    #[test]
    fn label_marks_an_ir_camera_whose_name_does_not() {
        assert_eq!(label("Integrated Camera: Integrated I", true), "Integrated Camera IR");
    }

    #[test]
    fn label_leaves_a_plain_name_alone() {
        assert_eq!(label("Logitech BRIO", false), "Logitech BRIO");
        assert_eq!(label("", false), "Camera");
    }

    #[test]
    fn rows_put_colour_before_ir_whatever_the_node_order() {
        let list = rows(&[asus_ir(), asus_colour()]);
        assert_eq!(ids(&list), ["/dev/video0", "/dev/video2"]);
        assert_eq!(list.iter().map(|r| r.ir).collect::<Vec<_>>(), [false, true]);
    }

    #[test]
    fn rows_treat_a_grey_only_device_as_ir_without_a_name_hint() {
        let list = rows(&[dev("/dev/video4", "Integrated Camera: Integrated I", true)]);
        assert!(list[0].ir);
        assert_eq!(list[0].label, "Integrated Camera IR");
    }

    #[test]
    fn rows_treat_an_ir_name_as_ir_even_with_colour_formats() {
        let list = rows(&[dev("/dev/video2", "HP IR Camera", false)]);
        assert!(list[0].ir);
    }

    #[test]
    fn rows_sort_nodes_numerically() {
        let list = rows(&[dev("/dev/video10", "B", false), dev("/dev/video2", "A", false)]);
        assert_eq!(ids(&list), ["/dev/video2", "/dev/video10"]);
    }

    #[test]
    fn rows_number_identical_labels() {
        let list = rows(&[dev("/dev/video0", "USB Camera", false), dev("/dev/video2", "USB Camera", false)]);
        assert_eq!(list.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["USB Camera", "USB Camera 2"]);
    }

    #[test]
    fn rows_of_nothing_is_empty() {
        assert_eq!(rows(&[]).len(), 0);
    }

    #[test]
    fn pick_keeps_the_current_camera_while_it_is_listed() {
        let list = rows(&[asus_colour(), asus_ir()]);
        assert_eq!(pick(&list, "/dev/video2"), "/dev/video2");
    }

    #[test]
    fn pick_falls_back_to_the_first_row_or_nothing() {
        let list = rows(&[asus_ir(), asus_colour()]);
        assert_eq!(pick(&list, "/dev/video9"), "/dev/video0");
        assert_eq!(pick(&[], "/dev/video0"), "");
    }

    #[test]
    fn step_cycles_forward_and_wraps() {
        let list = rows(&[asus_colour(), asus_ir()]);
        assert_eq!(step(&list, "/dev/video0", 1), "/dev/video2");
        assert_eq!(step(&list, "/dev/video2", 1), "/dev/video0");
    }

    #[test]
    fn step_cycles_backward_and_wraps() {
        let list = rows(&[asus_colour(), asus_ir()]);
        assert_eq!(step(&list, "/dev/video0", -1), "/dev/video2");
    }

    #[test]
    fn step_stays_on_a_single_camera() {
        let list = rows(&[asus_colour()]);
        assert_eq!(step(&list, "/dev/video0", 1), "/dev/video0");
    }

    #[test]
    fn step_starts_at_the_first_row_from_an_unknown_id() {
        let list = rows(&[asus_colour(), asus_ir()]);
        assert_eq!(step(&list, "", 1), "/dev/video0");
        assert_eq!(step(&[], "", 1), "");
    }
}
