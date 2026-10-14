mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::pcb;
use serde_json::{json, Value};

const DEFAULT: &str = r#"{"board":{"width":30,"height":20,"layers":4},"parts":[{"ref":"R1","kind":"resistor","footprint":"0402","at":[-5,0]},{"ref":"R2","kind":"resistor","footprint":"0402","at":[5,0],"side":"bottom"}],"nets":{"N":["R1.1","R2.1"]},"holes":[{"at":[0,5],"d":2}],"pours":[{"net":"GND","layer":"inner1"}]}"#;

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

#[test]
fn default_output_matches_0210() {
    let mut raw: Value = serde_json::from_str(DEFAULT).unwrap();
    raw["rules"] = json!({"beautify":false});
    let built = pcb::build(&raw.to_string(), "default", "pcb/default.pcb.json").unwrap();
    let old_report = built.report.replace("fab/default/default.ipc, ", "")
        .replace("fab/default/default-fab.svg, ", "");
    assert_eq!(hash(old_report.as_bytes()), 0x066720c643634129);
    let expected = [0x0be90ca493265f65, 0x7c43710f699dd942, 0x0c6edc93c24ebabc,
        0x2c64d4476bf1c441, 0xdd8f3824d5533598, 0x1d68a797e7a65ee5, 0x172a41aaa2bb9fab,
        0xb59ec3c696e1ccca, 0xec67f337a1fcd664, 0x91b15a812f886e79, 0x70d01c878bd2caf6,
        0x1928166cfcf60233, 0xd05cb8aac24e76f8, 0xf1cfc38dd324f389, 0xb2305f6aeb00ce3c,
        0x620e758a12ee479d, 0x7eae03a1f0cf9a54, 0xede2bfb9f1f90289, 0x0c7da867792f54b3,
        0x9ac16f19ed320131];
    let names = ["pcb/default.circuit.json", "pcb/default.board.json", "fab/default/default-B_Cu.gbr",
        "fab/default/default-F_Cu.gbr", "fab/default/default-In1_Cu.gbr", "fab/default/default-In2_Cu.gbr",
        "fab/default/default-F_Mask.gbr", "fab/default/default-F_Paste.gbr", "fab/default/default-F_Silkscreen.gbr",
        "fab/default/default-B_Mask.gbr", "fab/default/default-B_Paste.gbr", "fab/default/default-B_Silkscreen.gbr",
        "fab/default/default-Edge_Cuts.gbr", "fab/default/default-PTH.drl", "fab/default/default-NPTH.drl",
        "pcb/default-pcb.svg", "pcb/default-schematic.svg", "fab/default/default-pnp.csv",
        "fab/default/default-bom.csv", "pcb/default.step"];
    assert_eq!(built.files.len(), expected.len() + 2);
    for name in ["fab/default/default.ipc", "fab/default/default-fab.svg"] {
        assert!(built.files.iter().any(|(path, _)| path == name));
    }
    // Gerber headers carry the package version; pin it so the 0.21.0 hashes stay comparable.
    let version = concat!(",pcb,", env!("CARGO_PKG_VERSION"), "*");
    for (name, hash0210) in names.into_iter().zip(expected) {
        let data = &built.files.iter().find(|(path, _)| path == name).unwrap().1;
        assert_eq!(hash(data.replace(version, ",pcb,0.21.0*").as_bytes()), hash0210, "{name}");
    }
    assert_eq!(hash(&built.binary_files[0].1), 0xe26fce56bd5843a0);
    let mut explicit: Value = serde_json::from_str(DEFAULT).unwrap();
    explicit["rules"] = json!({"blind_vias":false,"teardrops":false,"beautify":false,"paste_reduction":0,"paste_windowpane":false});
    explicit["panel"] = json!(null);
    explicit["diffpairs"] = json!({});
    explicit["length_groups"] = json!({});
    explicit["board"]["er"] = json!(4.5);
    explicit["board"]["prepreg"] = json!(0.21);
    let off = pcb::build(&explicit.to_string(), "default", "pcb/default.pcb.json").unwrap();
    assert_eq!(built.report, off.report);
    assert_eq!(built.files, off.files);
    assert_eq!(built.binary_files, off.binary_files);
}

fn usb() -> Value {
    json!({"board":{"width":40,"height":25,"layers":4},"rules":{"clearance":0.15},"parts":[
        {"ref":"J1","kind":"header","footprint":"usb_micro_b","at":[-14,0],"rotation":-90},
        {"ref":"U1","kind":"chip","footprint":"soic8","at":[14,0]}],
        "nets":{"USB_DP":["J1.3","U1.2"],"USB_DN":["J1.2","U1.1"]},
        "diffpairs":{"USB":{"p":"USB_DP","n":"USB_DN","gap":0.15,"width":0.2}}})
}

fn circuit(b: &pcb::NativeBuild) -> Vec<Value> {
    serde_json::from_str::<Value>(&b.files[0].1).unwrap().as_array().unwrap().clone()
}

fn length(elements: &[Value], net: &str) -> f64 {
    elements.iter().filter(|e| e["type"] == "pcb_trace" && e["net"] == net).map(|e| {
        e["route"].as_array().unwrap().windows(2).map(|p| {
            (p[0]["x"].as_f64().unwrap() - p[1]["x"].as_f64().unwrap())
                .hypot(p[0]["y"].as_f64().unwrap() - p[1]["y"].as_f64().unwrap())
        }).sum::<f64>()
    }).sum()
}

async fn drc(spec: &Value, built: &pcb::NativeBuild) -> String {
    let dir = support::project("pcb-batch4", &[("pcb/test.pcb.json", spec.to_string()), ("pcb/test.circuit.json", built.files[0].1.clone())]);
    support::run("pcb_drc", json!({"board":"test"}), &dir.path).await.unwrap().text
}

#[tokio::test]
async fn usb_pair_couples_and_clears_an_obstacle() {
    for obstacle in [false, true] {
        let mut spec = usb();
        if obstacle { spec["parts"].as_array_mut().unwrap().push(json!({"ref":"J2","kind":"header","footprint":"pinheader_1x3","at":[0,0],"rotation":90})); }
        let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
        let elements = circuit(&built);
        let skew = (length(&elements, "USB_DP") - length(&elements, "USB_DN")).abs();
        let line = built.report.lines().find(|l| l.starts_with("Diffpair USB:")).unwrap();
        println!("{line}");
        let percent = line.split("coupled ").nth(1).unwrap().split('%').next().unwrap().parse::<f64>().unwrap();
        assert!(percent >= 80., "{}", built.report);
        assert!(skew <= 0.100001, "skew {skew}: {}", built.report);
        let report = drc(&spec, &built).await;
        assert!(report.ends_with("No DRC errors."), "{report}");
    }
}

#[tokio::test]
async fn usb_pair_changes_layers_together() {
    let mut spec = usb();
    spec["parts"][1]["side"] = json!("bottom");
    spec["nets"]["USB_DP"] = json!(["J1.3","U1.7"]);
    spec["nets"]["USB_DN"] = json!(["J1.2","U1.8"]);
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    let vias: Vec<_> = elements.iter().filter(|e| e["type"] == "pcb_via").collect();
    assert_eq!(vias.len(), 2, "{}", built.report);
    assert_eq!(vias[0]["layers"], vias[1]["layers"]);
    assert_ne!(vias[0]["net"], vias[1]["net"]);
    assert!(!built.report.contains("could not couple"), "{}", built.report);
    assert!((length(&elements, "USB_DP") - length(&elements, "USB_DN")).abs() <= 0.100001);
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn crossed_pair_falls_back_without_losing_connections() {
    let mut spec = usb();
    spec["nets"]["USB_DP"] = json!(["J1.3","U1.1"]);
    spec["nets"]["USB_DN"] = json!(["J1.2","U1.2"]);
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert!(built.report.contains("Diffpair USB: routed as separate nets (could not couple)"), "{}", built.report);
    let report = drc(&spec, &built).await;
    assert!(!report.contains("unrouted:"), "{report}");
}

#[tokio::test]
async fn multi_land_terminals_keep_all_connections_in_fallback() {
    let mut spec = usb();
    spec["nets"]["USB_DP"] = json!(["J1.6","U1.2"]);
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert!(built.report.contains("Diffpair USB: routed as separate nets (could not couple)"), "{}", built.report);
    let report = drc(&spec, &built).await;
    assert!(!report.contains("unrouted:"), "{report}");
}

#[tokio::test]
async fn drc_recomputes_skew_and_gap_from_saved_tracks() {
    let spec = usb();
    let mut built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let mut elements = circuit(&built);
    let t = elements.iter().find(|e| e["type"] == "pcb_trace" && e["net"] == "USB_DP").unwrap();
    let p = &t["route"][0];
    let x = p["x"].as_f64().unwrap();
    elements.push(json!({"type":"pcb_trace","width":0.2,"net":"USB_DP","route":[p.clone(),
        {"x":x+3.,"y":p["y"],"width":0.2,"layer":p["layer"]}]}));
    built.files[0].1 = serde_json::to_string(&elements).unwrap();
    let report = drc(&spec, &built).await;
    assert!(report.contains("diffpair: USB skew 3.00 mm exceeds 0.10 mm"), "{report}");
    elements.pop();
    for t in elements.iter_mut().filter(|e| e["type"] == "pcb_trace") {
        t["width"] = json!(0.3);
        for p in t["route"].as_array_mut().unwrap() { p["width"] = json!(0.3); }
    }
    built.files[0].1 = serde_json::to_string(&elements).unwrap();
    let report = drc(&spec, &built).await;
    assert!(report.contains("diffpair: USB gap 0.05 mm differs from 0.15 mm"), "{report}");
}

#[tokio::test]
async fn four_nets_gain_meanders_and_match_lengths() {
    let mut spec = json!({"board":{"width":50,"height":40},"parts":[],"nets":{},"length_groups":{"DATA":{"nets":["D0","D1","D2","D3"],"tolerance":0.5}}});
    for (i, x) in [0,4,8,12].into_iter().enumerate() {
        let y = -12 + i as i32 * 8;
        for (label, px) in [("A", -16), ("B", 8 + x)] {
            spec["parts"].as_array_mut().unwrap().push(json!({"ref":format!("{label}{i}"),"kind":"resistor","footprint":"0603","at":[px,y]}));
        }
        spec["nets"][format!("D{i}")] = json!([format!("A{i}.2"),format!("B{i}.1")]);
    }
    let matched = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&matched);
    let lengths: Vec<_> = (0..4).map(|i| length(&elements, &format!("D{i}"))).collect();
    let spread = lengths.iter().copied().fold(0., f64::max) - lengths.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(spread <= 0.500001, "{lengths:?}: {}", matched.report);
    let report = drc(&spec, &matched).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
    spec.as_object_mut().unwrap().remove("length_groups");
    let natural = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert!(elements.iter().filter(|e| e["type"] == "pcb_trace").count() > circuit(&natural).iter().filter(|e| e["type"] == "pcb_trace").count());
    spec["length_groups"] = json!({"DATA":{"nets":["D0","D1","D2","D3"],"tolerance":0.5}});
    let report = drc(&spec, &natural).await;
    assert!(report.contains("length: group DATA spread 12.00 mm exceeds tolerance 0.50 mm"), "{report}");
}

#[tokio::test]
async fn blind_plane_vias_have_physical_spans_and_split_drills() {
    let spec = json!({"board":{"width":20,"height":16,"layers":4},"rules":{"blind_vias":true},"parts":[
        {"ref":"R1","kind":"resistor","footprint":"0603","at":[-5,0]},
        {"ref":"R2","kind":"resistor","footprint":"0603","at":[5,0],"side":"bottom"}],
        "nets":{"GND":["R1.1","R2.1"]},"pours":[{"net":"GND","layer":"inner1"}]});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    let vias: Vec<_> = elements.iter().filter(|e| e["type"] == "pcb_via").collect();
    assert!(vias.iter().any(|v| v["layers"] == json!(["top","inner1"])));
    assert!(vias.iter().any(|v| v["layers"] == json!(["bottom","inner1","inner2"])));
    for suffix in ["PTH-1-2.drl", "PTH-2-4.drl"] {
        let file = built.files.iter().find(|(n, _)| n.ends_with(suffix)).unwrap();
        assert!(file.1.contains(",Blind\n"));
    }
    assert!(built.report.contains("(2 blind/buried)"));
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn blind_signal_vias_cross_only_the_needed_layers() {
    let spec = json!({"board":{"width":24,"height":16,"layers":4},"rules":{"blind_vias":true},"parts":[
        {"ref":"R1","kind":"resistor","footprint":"0603","at":[-7,0]},
        {"ref":"R2","kind":"resistor","footprint":"0603","at":[7,0]}],"nets":{"N":["R1.2","R2.1"]},
        "keepouts":[{"rect":[0,0,1,16],"layers":["top"]},{"rect":[0,0,24,16],"layers":["bottom"]}]});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    let vias: Vec<_> = elements.iter().filter(|e| e["type"] == "pcb_via").collect();
    assert_eq!(vias.len(), 2);
    assert!(vias.iter().all(|v| v["layers"] == json!(["top","inner1"])));
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn teardrops_are_saved_copper_and_default_off() {
    let mut spec = json!({"board":{"width":25,"height":18},"rules":{"teardrops":true},"parts":[
        {"ref":"R1","kind":"resistor","footprint":"0603","at":[-5,0]},
        {"ref":"R2","kind":"resistor","footprint":"0603","at":[5,0]}],"nets":{"N":["R1.2","R2.1"]},"pours":[{"net":"GND","layer":"top"}]});
    let on = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert!(circuit(&on).iter().any(|e| e["type"] == "pcb_trace_teardrop"));
    let report = drc(&spec, &on).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
    spec["rules"]["teardrops"] = json!(false);
    let off = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let regions = |b: &pcb::NativeBuild| b.files.iter().find(|(n, _)| n.ends_with("F_Cu.gbr")).unwrap().1.matches("G36*").count();
    assert!(regions(&on) > regions(&off));
    spec.as_object_mut().unwrap().remove("rules");
    let absent = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert_eq!(off.files, absent.files);
    assert_eq!(off.binary_files, absent.binary_files);
    assert_eq!(off.report, absent.report);
}

macro_rules! invalid {
    ($test:ident, $change:expr, $error:expr) => {
        #[test]
        fn $test() {
            let mut spec = usb();
            ($change)(&mut spec);
            assert_eq!(pcb::parse(&spec.to_string()).unwrap_err(), $error);
        }
    };
}
invalid!(netclass_exclusive, |s: &mut Value| s["netclasses"] = json!({"rf":{"nets":[],"trace":0.2,"impedance":50}}), "netclass rf: set trace or impedance, not both");
invalid!(netclass_needs_width, |s: &mut Value| s["netclasses"] = json!({"rf":{"nets":[]}}), "netclass rf: set trace or impedance");
invalid!(pair_unknown_net, |s: &mut Value| s["diffpairs"]["USB"]["p"] = json!("BAD"), "diffpair USB: unknown net BAD");
invalid!(pair_two_pins, |s: &mut Value| s["nets"]["USB_DP"] = json!(["J1.3"]), "diffpair USB: net USB_DP must connect exactly 2 pins");
invalid!(pair_unique_net, |s: &mut Value| s["diffpairs"]["Z"] = s["diffpairs"]["USB"].clone(), "diffpair Z: net USB_DP is already in another diffpair");
invalid!(pair_exclusive, |s: &mut Value| s["diffpairs"]["USB"]["impedance"] = json!(90), "diffpair USB: set width or impedance, not both");
invalid!(pair_gap_clearance, |s: &mut Value| s["diffpairs"]["USB"]["gap"] = json!(0.1), "diffpair USB: gap 0.1 is below clearance 0.15");
invalid!(group_unknown_net, |s: &mut Value| s["length_groups"] = json!({"DATA":{"nets":["BAD","USB_DP"],"tolerance":0.5}}), "length group DATA: unknown net BAD");
invalid!(group_two_nets, |s: &mut Value| s["length_groups"] = json!({"DATA":{"nets":["USB_DP"],"tolerance":0.5}}), "length group DATA: needs at least 2 nets");
invalid!(group_no_pair, |s: &mut Value| s["length_groups"] = json!({"DATA":{"nets":["USB_DP","USB_DN"],"tolerance":0.5}}), "length group DATA: net USB_DP is in diffpair USB");
invalid!(group_unique_net, |s: &mut Value| {
    s.as_object_mut().unwrap().remove("diffpairs");
    s["length_groups"] = json!({"A":{"nets":["USB_DP","USB_DN"],"tolerance":0.5},"Z":{"nets":["USB_DP","USB_DN"],"tolerance":0.5}});
}, "length group Z: net USB_DP is already in another length group");
invalid!(blind_needs_four_layers, |s: &mut Value| { s["board"]["layers"] = json!(2); s["rules"]["blind_vias"] = json!(true); }, "rules.blind_vias needs 4 layers");

#[test]
fn positive_fields_and_boolean_rules_stay_strict() {
    for field in ["er","prepreg"] {
        let mut spec = usb(); spec["board"][field] = json!(0);
        assert_eq!(pcb::parse(&spec.to_string()).unwrap_err(), format!("board.{field} must be positive"));
    }
    for field in ["gap","width","impedance","skew"] {
        for value in [0., -1000.] {
            let mut spec = usb();
            if field == "impedance" { spec["diffpairs"]["USB"].as_object_mut().unwrap().remove("width"); }
            spec["diffpairs"]["USB"][field] = json!(value);
            assert_eq!(pcb::parse(&spec.to_string()).unwrap_err(), format!("diffpair.{field} must be positive"));
        }
    }
    let mut spec = usb(); spec["netclasses"] = json!({"rf":{"nets":[],"impedance":0}});
    assert_eq!(pcb::parse(&spec.to_string()).unwrap_err(), "netclass.impedance must be positive");
    let mut spec = usb(); spec["length_groups"] = json!({"DATA":{"nets":["USB_DP","USB_DN"],"tolerance":0}});
    assert_eq!(pcb::parse(&spec.to_string()).unwrap_err(), "length_group.tolerance must be positive");
    for field in ["blind_vias","teardrops"] {
        for bad in [json!(1),json!("true"),json!(null)] {
            let mut spec = usb(); spec["rules"][field] = bad;
            assert!(pcb::parse(&spec.to_string()).is_err());
        }
    }
}

#[test]
fn impedance_targets_set_widths_and_report() {
    let mut spec = usb();
    spec["diffpairs"]["USB"].as_object_mut().unwrap().remove("width");
    spec["diffpairs"]["USB"]["impedance"] = json!(90);
    spec["netclasses"] = json!({"rf":{"nets":[],"impedance":50}});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert!(built.report.contains("rf 50 ohm -> 0.344 mm"), "{}", built.report);
    assert!(built.report.contains("USB 90 ohm differential ->"), "{}", built.report);
    let elements = circuit(&built);
    let width = pcb::prepare(&spec.to_string()).unwrap().nets[0].trace;
    assert!(elements.iter().filter(|e| e["type"] == "pcb_trace").all(|e| e["width"].as_f64().unwrap() == width));
    spec.as_object_mut().unwrap().remove("diffpairs");
    spec["netclasses"]["rf"]["nets"] = json!(["USB_DP","USB_DN"]);
    assert!(pcb::prepare(&spec.to_string()).unwrap().nets.iter().all(|n| n.trace == 0.344));
}
