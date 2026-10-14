use super::{parse::{Analysis, Circuit, NONE}, solve::{self, Row, Solver}, Simulation};
use crate::jsfmt::number;
use std::fmt::Write;

struct Column { label: String, index: usize }
fn columns(c: &Circuit) -> Vec<Column> {
    let mut columns: Vec<_> = c.nodes.iter().enumerate().skip(1).filter(|(i, _)| c.visible[*i])
        .map(|(i, name)| Column { label: format!("v({name})"), index: i - 1 }).collect();
    columns.extend(c.elements.iter().filter(|e| e.branch != NONE)
        .map(|e| Column { label: format!("i({})", e.name), index: e.branch }));
    columns
}

pub(super) fn build(c: &Circuit, name: &str) -> Result<Simulation, String> {
    let needs_dc = c.analyses.iter().any(|a| !matches!(a, Analysis::Tran(t) if t.uic));
    solve::topology(c, needs_dc)?;
    let columns = columns(c);
    let mut files = vec![];
    let mut summaries = vec![];
    let mut tables = vec![];
    for analysis in &c.analyses {
        let mut solver = Solver::new(c);
        let (kind, axis, rows) = match analysis {
            Analysis::Op => {
                let x = solver.op(None, None, false, None)?;
                let path = format!("sim/{name}/{name}-op.txt");
                let mut table = "v(0) = 0 V\n".to_owned();
                for column in &columns {
                    writeln!(table, "{} = {} {}", column.label, number(x[column.index]), if column.label.starts_with('v') { "V" } else { "A" }).unwrap();
                }
                summaries.push(format!(".op: {} node voltages, {} branch currents -> {path}", c.visible.iter().skip(1).filter(|v| **v).count(), c.elements.iter().filter(|e| e.branch != NONE).count()));
                let mut short = table.lines().take(40).collect::<Vec<_>>().join("\n");
                if table.lines().count() > 40 { short.push_str("\n... (full table in -op.txt)"); }
                tables.push(short); files.push((path, table)); continue;
            }
            Analysis::Dc { source, start, stop, step } => {
                let count = (((stop - start) / step) + 1e-10).floor() as usize + 1;
                // shortcut: 2 million saved scalars, stream output for larger sweeps.
                if count.saturating_mul(c.size + 1) > 2_000_000 { return Err("simulation output too large (limit 2000000 values)".into()); }
                let mut rows: Vec<Row> = Vec::with_capacity(count);
                for i in 0..count {
                    let value = start + i as f64 * step;
                    let x = solver.op(rows.last().map(|r| r.x.as_slice()), Some((*source, value)), false, None)?;
                    rows.push(Row { axis: value, x });
                }
                ("dc", c.elements[*source].name.as_str(), rows)
            }
            Analysis::Tran(t) => ("tran", "time", solver.transient(t)?),
        };
        let csv_path = format!("sim/{name}/{name}-{kind}.csv");
        let svg_path = format!("sim/{name}/{name}-{kind}.svg");
        let mut csv = axis.to_owned();
        for column in &columns { csv.push(','); csv.push_str(&csv_field(&column.label)); }
        csv.push('\n');
        for row in &rows {
            write!(csv, "{:.12e}", clean(row.axis)).unwrap();
            for column in &columns { write!(csv, ",{:.12e}", clean(row.x[column.index])).unwrap(); }
            csv.push('\n');
        }
        let traces = c.traces.get(kind).or_else(|| c.traces.get("all"));
        let ground = Column { label: "v(0)".into(), index: NONE };
        let selected: Vec<_> = if let Some(traces) = traces {
            traces.iter().take(8).map(|label| if label == "v(0)" || label == "v(gnd)" { Ok(&ground) }
                else { columns.iter().find(|col| col.label.eq_ignore_ascii_case(label))
                .ok_or_else(|| format!("unknown trace {label}")) }).collect::<Result<_, _>>()?
        } else { columns.iter().filter(|col| col.label.starts_with('v')).take(8).collect() };
        let svg = plot(&c.title, kind, axis, &rows, &selected);
        summaries.push(format!(".{kind}: {} accepted points -> {csv_path}, {svg_path}", rows.len()));
        files.push((csv_path, csv)); files.push((svg_path, svg));
    }
    summaries.extend(tables);
    Ok(Simulation { text: summaries.join("\n"), files })
}
fn clean(x: f64) -> f64 { if x == 0. { 0. } else { x } }
fn csv_field(s: &str) -> String { if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() } }
fn escape(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }

fn plot(title: &str, kind: &str, axis: &str, rows: &[Row], traces: &[&Column]) -> String {
    let value = |row: &Row, trace: &Column| if trace.index == NONE { 0. } else { row.x[trace.index] };
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for row in rows {
        xmin = xmin.min(row.axis); xmax = xmax.max(row.axis);
        for trace in traces { ymin = ymin.min(value(row, trace)); ymax = ymax.max(value(row, trace)); }
    }
    if !xmin.is_finite() { xmin = 0.; xmax = 1.; }
    if xmin == xmax { xmax = xmin + 1.; }
    if !ymin.is_finite() { ymin = 0.; ymax = 1.; }
    if ymin == ymax { let span = ymin.abs().max(1.) * 0.05; ymin -= span; ymax += span; }
    else { let pad = (ymax - ymin) * 0.05; ymin -= pad; ymax += pad; }
    let x = |v: f64| 85. + (v - xmin) / (xmax - xmin) * 660.;
    let y = |v: f64| 395. - (v - ymin) / (ymax - ymin) * 330.;
    let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"460\" viewBox=\"0 0 960 460\"><title>{}</title><rect width=\"960\" height=\"460\" fill=\"white\"/><g font-family=\"sans-serif\" font-size=\"12\" fill=\"#222\"><text x=\"85\" y=\"28\" font-size=\"16\">{} — .{kind}</text><text x=\"85\" y=\"52\">V / A</text>", escape(title), escape(title));
    for i in 0..=5 {
        let vx = xmin + (xmax - xmin) * i as f64 / 5.;
        let vy = ymin + (ymax - ymin) * i as f64 / 5.;
        write!(svg, "<path d=\"M{:.3} 65V395M85 {:.3}H745\" stroke=\"#ddd\" fill=\"none\"/><text x=\"{:.3}\" y=\"416\" text-anchor=\"middle\">{:.3e}</text><text x=\"77\" y=\"{:.3}\" text-anchor=\"end\">{:.3e}</text>", x(vx), y(vy), x(vx), clean(vx), y(vy) + 4., clean(vy)).unwrap();
    }
    svg.push_str("<path d=\"M85 65V395H745\" stroke=\"#222\" fill=\"none\"/>");
    write!(svg, "<text x=\"415\" y=\"446\" text-anchor=\"middle\">{}</text>", escape(if axis == "time" { "time (s)" } else { axis })).unwrap();
    let colors = ["#1261a0", "#c43221", "#208148", "#8a39a4", "#b27300", "#007f88", "#b23a77", "#555555"];
    for (i, trace) in traces.iter().enumerate() {
        let color = colors[i];
        write!(svg, "<polyline fill=\"none\" stroke=\"{color}\" stroke-width=\"1.5\" points=\"").unwrap();
        for row in rows { write!(svg, "{:.3},{:.3} ", x(row.axis), y(value(row, trace))).unwrap(); }
        write!(svg, "\"/><path d=\"M765 {}h22\" stroke=\"{color}\" stroke-width=\"2\"/><text x=\"794\" y=\"{}\">{}</text>", 76 + i * 24, 80 + i * 24, escape(&trace.label)).unwrap();
    }
    svg.push_str("</g></svg>\n"); svg
}
