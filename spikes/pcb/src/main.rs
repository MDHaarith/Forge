mod fab;
mod footprint;
mod model;
mod pour;
mod route;

use std::{error::Error, fs, path::Path};

fn report(log: &mut String, line: String) {
    println!("{line}");
    log.push_str(&line);
    log.push('\n');
}

fn main() -> Result<(), Box<dyn Error>> {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut log = String::from(
        "Forge native PCB spike; units mm; release measurements exclude compilation.\n",
    );
    footprint::compare(&mut log)?;
    let led = route::led();
    let led_routes = route::benchmark("LED", &led, &mut log);
    let generated = route::generated();
    let generated_routes = route::benchmark("generated-30", &generated, &mut log);
    let pour = pour::benchmark(&led, &led_routes, &mut log)?;
    fab::export(&led, &led_routes, &pour, out, &mut log)?;
    fab::generated_preview(&generated, &generated_routes, out)?;
    model::export(&led, &led_routes, out, &mut log)?;
    let mut preview=String::from("<!doctype html><html lang='en'><meta charset='utf-8'><meta name='viewport' content='width=device-width,initial-scale=1'><title>Forge PCB spike outputs</title><style>body{margin:0;background:#101923;color:#e9f1f6;font:16px system-ui;padding:24px}h1{font-size:22px}h2{font-size:18px}section{margin:24px 0}svg{width:100%;height:auto;max-height:720px}p{color:#bac8d4}.grid{display:grid;grid-template-columns:1fr 1fr;gap:24px}@media(max-width:700px){.grid{grid-template-columns:1fr}}</style><h1>Forge PCB spike: generated files</h1><p>Orange: top copper. Blue: bottom copper. Pink rims: drills. Bottom panel keeps top-view coordinates.</p>");
    for (i, (file, title)) in [
        ("gerber-preview.svg", "Gerber + Excellon readback"),
        (
            "generated-router.svg",
            "30-part routing: red pads belong to unrouted nets",
        ),
        ("assembly.svg", "Cadrum board slab and package boxes"),
    ]
    .iter()
    .enumerate()
    {
        if i == 1 {
            preview.push_str("<div class='grid'>");
        }
        let svg = fs::read_to_string(out.join(file))?;
        preview.push_str(&format!(
            "<section><h2>{title}</h2>{}</section>",
            &svg[svg.find("<svg").ok_or("missing SVG root")?..]
        ));
    }
    preview.push_str("</div></html>");
    fs::write(out.join("preview.html"), preview)?;
    report(&mut log,"plan arithmetic: clearance/2 = 0.2/2 = 0.1 mm; LED grid = 301*201*2 = 121002 states; generated grid = 501*401*2 = 401802 states".into());
    report(&mut log,"plan arithmetic: keepout [-3,16,32,8] spans x=[-19,13], y=[12,20]; board y=[-10,10]; intersection=0 mm2 (12 > 10)".into());
    report(&mut log,"plan arithmetic: GND terminals = LED1.K + J1.2 + U1.8 = 3; needs 3-1 = 2 links; VCC has 1 terminal, so 0 links".into());
    report(&mut log,"plan arithmetic: 0603 pad difference = 0.875-0.800 = 0.075 mm; centre difference = 1.650-1.575 = 0.075 mm; exceeds 0.05 by 0.025 mm; lock component tolerances before freezing tests".into());
    report(&mut log,format!("plan limit: generated board has 30 < 60 parts, 430 pads and {} unrouted nets; part count alone does not predict grid router success",generated_routes.routed.iter().filter(|&&v|!v).count()));
    report(&mut log,"all spike geometry/export assertions passed; numerical contract mismatches and routing failures remain reported above".into());
    fs::write(out.join("results.log"), log)?;
    Ok(())
}
