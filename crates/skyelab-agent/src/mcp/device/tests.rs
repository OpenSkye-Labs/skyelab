use super::super::{Tool, tools};
use super::{DeviceKey, DeviceTool, MAX_COORD, MAX_TYPE};
use serde_json::{Value, json};

fn parse(name: &str, args: Value) -> Result<Tool, String> {
    tools::parse(name, &args)
}

fn err(name: &str, args: Value) -> String {
    parse(name, args).expect_err("refused")
}

#[test]
fn device_tools_parse() {
    let d = || "emulator-5554".to_string();
    assert_eq!(
        parse("device_list", Value::Null),
        Ok(Tool::Device(DeviceTool::List))
    );
    assert_eq!(
        parse("device_open", json!({"device_id": "emulator-5554"})),
        Ok(Tool::Device(DeviceTool::Open { device_id: d() }))
    );
    assert_eq!(
        parse(
            "device_tap",
            json!({"device_id": "emulator-5554", "x": 0, "y": MAX_COORD})
        ),
        Ok(Tool::Device(DeviceTool::Tap {
            device_id: d(),
            x: 0,
            y: MAX_COORD as u32
        }))
    );
    assert_eq!(
        parse(
            "device_type",
            json!({"device_id": "emulator-5554", "text": "hi there!"})
        ),
        Ok(Tool::Device(DeviceTool::Type {
            device_id: d(),
            text: "hi there!".into()
        }))
    );
    assert_eq!(
        parse(
            "device_press",
            json!({"device_id": "emulator-5554", "key": "back"})
        ),
        Ok(Tool::Device(DeviceTool::Press {
            device_id: d(),
            key: DeviceKey::Back
        }))
    );
    for (name, id) in [
        ("device_screenshot", "1A2B-3C4D"),
        ("device_close", "192.168.1.5:5555"),
        ("device_open", "Pixel_9_API_36"),
    ] {
        assert!(parse(name, json!({"device_id": id})).is_ok(), "{name} {id}");
    }
}

#[test]
fn bad_device_ids_are_refused() {
    for id in [
        json!("-g"),
        json!(""),
        json!("a b"),
        json!("a;b"),
        json!("x".repeat(129)),
        json!(5),
    ] {
        let e = err("device_open", json!({"device_id": id}));
        assert!(e.contains("device_id"), "{id}: {e}");
    }
    assert!(err("device_screenshot", json!({})).contains("device_id"));
}

#[test]
fn bad_input_is_refused_with_its_field() {
    let id = "emulator-5554";
    for (x, y) in [
        (json!(-1), json!(1)),
        (json!(1), json!(MAX_COORD + 1)),
        (json!(1.5), json!(1)),
        (json!("1"), json!(1)),
    ] {
        let e = err("device_tap", json!({"device_id": id, "x": x, "y": y}));
        assert!(e.contains("must be an integer from 0"), "{e}");
    }
    assert!(err("device_tap", json!({"device_id": id, "x": 1})).starts_with("y "));
    for text in [
        json!(""),
        json!("a\nb"),
        json!("caf\u{e9}"),
        json!("\u{1b}[2J"),
        json!("x".repeat(MAX_TYPE + 1)),
    ] {
        let e = err("device_type", json!({"device_id": id, "text": text}));
        assert!(e.contains("text"), "{text}: {e}");
    }
    for key in [json!("power"), json!(""), json!(null), json!(3)] {
        let e = err("device_press", json!({"device_id": id, "key": key}));
        assert!(e.contains("key"), "{key}: {e}");
    }
}

#[test]
fn device_tools_are_listed() {
    let list = tools::list();
    let names: Vec<&str> = list
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for name in [
        "device_list",
        "device_open",
        "device_screenshot",
        "device_tap",
        "device_type",
        "device_press",
        "device_close",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
}
