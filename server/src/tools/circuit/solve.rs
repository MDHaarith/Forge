use super::parse::{Circuit, Element, Kind, Tran, NONE};
use std::{collections::{BTreeMap, VecDeque}, iter::Sum, ops::{Add, AddAssign, Sub, SubAssign, Mul, Div, DivAssign, Neg}};

pub(super) trait Scalar: Copy + PartialEq + From<f64> + Add<Output=Self> + Sub<Output=Self> + Mul<Output=Self> + Div<Output=Self> + Neg<Output=Self> + AddAssign + SubAssign + DivAssign + Sum {
    fn norm(self) -> f64;
    fn finite(self) -> bool;
}
impl Scalar for f64 {
    fn norm(self) -> f64 { self.abs() }
    fn finite(self) -> bool { self.is_finite() }
}

pub(super) struct Row { pub axis: f64, pub x: Vec<f64> }
struct State { time: f64, x: Vec<f64>, q: Vec<[f64; 4]>, dq: Vec<[f64; 4]>, gate_q: Vec<[f64; 4]> }
#[derive(Clone, Copy)]
struct Step<'a> { h: f64, trap: bool, previous: &'a State }
impl Step<'_> { fn factor(self) -> f64 { if self.trap { 2. / self.h } else { 1. / self.h } } }
#[derive(Clone, Copy)]
struct Bias<'a> { time: Option<f64>, source: Option<(usize, f64)>, scale: f64, gmin: f64, clamps: Option<&'a BTreeMap<usize, f64>> }

// shortcut: dense LU, upgrade to sparse LU above ~500 nodes.
pub(super) struct System<T = f64> { pub n: usize, pub a: Vec<T>, pub b: Vec<T>, previous: Vec<T>, lu: Vec<T>, pivots: Vec<usize> }
impl<T: Scalar> System<T> {
    pub fn new(n: usize) -> Self { Self { n, a: vec![T::from(0.); n * n], b: vec![T::from(0.); n], previous: vec![], lu: vec![], pivots: vec![] } }
    pub fn reset(&mut self) { self.a.fill(T::from(0.)); self.b.fill(T::from(0.)); }
    pub fn add(&mut self, row: usize, col: usize, value: T) { if row != NONE && col != NONE { self.a[row * self.n + col] += value; } }
    pub fn rhs(&mut self, row: usize, value: T) { if row != NONE { self.b[row] += value; } }
    pub fn solve(&mut self) -> Option<Vec<T>> {
        let n = self.n;
        if self.a != self.previous {
            self.previous.clear();
            self.lu.clone_from(&self.a); self.pivots.clear();
            for k in 0..n {
                let pivot = (k..n).max_by(|&a, &b| self.lu[a * n + k].norm().total_cmp(&self.lu[b * n + k].norm()))?;
                let diagonal = self.lu[pivot * n + k];
                if !diagonal.finite() || diagonal.norm() < 1e-300 { return None; }
                self.pivots.push(pivot);
                if pivot != k { for j in 0..n { self.lu.swap(k * n + j, pivot * n + j); } }
                let diagonal = self.lu[k * n + k];
                let (head, tail) = self.lu.split_at_mut((k + 1) * n);
                let upper = &head[k * n..(k + 1) * n];
                for row in tail.chunks_exact_mut(n) {
                    if row[k] == T::from(0.) { continue; }
                    row[k] /= diagonal;
                    let factor = row[k];
                    for (value, upper) in row[k + 1..].iter_mut().zip(&upper[k + 1..]) { *value -= factor * *upper; }
                }
            }
            self.previous.clone_from(&self.a);
        }
        let mut x = self.b.clone();
        for (k, &pivot) in self.pivots.iter().enumerate() { x.swap(k, pivot); }
        for i in 0..n {
            let row = &self.lu[i * n..(i + 1) * n];
            let sum: T = row[..i].iter().zip(&x[..i]).map(|(a, b)| *a * *b).sum();
            x[i] -= sum;
        }
        for i in (0..n).rev() {
            let row = &self.lu[i * n..(i + 1) * n];
            let sum: T = row[i + 1..].iter().zip(&x[i + 1..]).map(|(a, b)| *a * *b).sum();
            x[i] = (x[i] - sum) / row[i];
        }
        x.iter().all(|v| v.finite()).then_some(x)
    }
    /// Solve Aᵀx=b using the same pivoted factors as the forward solve.
    pub fn transpose(&self, b: &[T]) -> Option<Vec<T>> {
        let n = self.n;
        let mut x = b.to_vec();
        for i in 0..n {
            let sum: T = (0..i).map(|j| self.lu[j * n + i] * x[j]).sum();
            x[i] = (x[i] - sum) / self.lu[i * n + i];
        }
        for i in (0..n).rev() {
            let sum: T = (i + 1..n).map(|j| self.lu[j * n + i] * x[j]).sum();
            x[i] -= sum;
        }
        for (k, &pivot) in self.pivots.iter().enumerate().rev() { x.swap(k, pivot); }
        x.iter().all(|v| v.finite()).then_some(x)
    }
}
pub(super) fn index(node: usize) -> usize { if node == 0 { NONE } else { node - 1 } }
pub(super) fn conductance<T: Scalar>(s: &mut System<T>, a: usize, b: usize, g: T) {
    let (a, b) = (index(a), index(b));
    s.add(a, a, g); s.add(b, b, g); s.add(a, b, -g); s.add(b, a, -g);
}
pub(super) fn current<T: Scalar>(s: &mut System<T>, a: usize, b: usize, i: T) { s.rhs(index(a), -i); s.rhs(index(b), i); }
pub(super) fn branch<T: Scalar>(s: &mut System<T>, element: &Element) {
    let [a, b, _, _] = element.nodes;
    s.add(index(a), element.branch, T::from(1.)); s.add(index(b), element.branch, T::from(-1.));
    s.add(element.branch, index(a), T::from(1.)); s.add(element.branch, index(b), T::from(-1.));
}

pub(super) fn topology(c: &Circuit, dc: bool) -> Result<(), String> {
    let mut graph = vec![vec![]; c.nodes.len()];
    let mut ideal: Vec<Vec<(usize, String)>> = vec![vec![]; c.nodes.len()];
    for element in &c.elements {
        let n = element.nodes;
        if matches!(element.kind, Kind::V(_) | Kind::E(_)) || (dc && matches!(element.kind, Kind::L(_, _))) {
            let mut visited = vec![false; c.nodes.len()];
            let mut parent: Vec<Option<(usize, String)>> = vec![None; c.nodes.len()];
            let mut queue = VecDeque::from([n[0]]); visited[n[0]] = true;
            while let Some(node) = queue.pop_front() {
                for (next, name) in &ideal[node] { if !visited[*next] { visited[*next] = true; parent[*next] = Some((node, name.clone())); queue.push_back(*next); } }
            }
            if visited[n[1]] {
                let mut names = vec![element.name.clone()];
                let mut node = n[1];
                while let Some((previous, name)) = &parent[node] { names.push(name.clone()); node = *previous; }
                names.sort_by_key(|s| s.to_ascii_lowercase());
                return Err(format!("voltage source loop: {}", names.join(", ")));
            }
            ideal[n[0]].push((n[1], element.name.clone())); ideal[n[1]].push((n[0], element.name.clone()));
        }
        let pairs: Vec<_> = match &element.kind {
            Kind::R(_) | Kind::L(_, _) | Kind::V(_) | Kind::E(_) => vec![(n[0], n[1])],
            Kind::Device(model) if model.matches(b'm') => vec![(n[0], n[3]), (n[2], n[3])],
            Kind::Device(model) => (1..model.terminal_count()).map(|i| (n[0], n[i])).collect(),
            _ => vec![],
        };
        for (a, b) in pairs { graph[a].push(b); graph[b].push(a); }
    }
    if dc {
        let mut reached = vec![false; graph.len()];
        let mut queue = VecDeque::from([0]); reached[0] = true;
        while let Some(node) = queue.pop_front() { for &next in &graph[node] { if !reached[next] { reached[next] = true; queue.push_back(next); } } }
        if let Some(node) = (1..reached.len()).find(|&n| !reached[n]) { return Err(format!("node {} has no DC path to ground", c.nodes[node])); }
    }
    Ok(())
}

pub(super) struct Solver<'a> { c: &'a Circuit, system: System, nonlinear: bool, step: f64, stop: f64 }
impl<'a> Solver<'a> {
    pub fn new(c: &'a Circuit) -> Self { Self { c, system: System::new(c.size), nonlinear: c.elements.iter().any(|e| matches!(e.kind, Kind::Device(_))), step: 1., stop: 1. } }
    fn assemble(&mut self, x: &[f64], previous: &mut [[f64; 4]], bias: Bias<'_>, step: Option<Step<'_>>, limit: bool) -> bool {
        let s = &mut self.system;
        s.reset();
        let mut limited = false;
        for (id, e) in self.c.elements.iter().enumerate() {
            let [a, b, cp, cn] = e.nodes;
            match &e.kind {
                Kind::R(r) => conductance(s, a, b, 1. / r),
                Kind::C(cap, _) => if let Some(step) = step {
                    let factor = step.factor();
                    conductance(s, a, b, cap * factor);
                    current(s, a, b, -factor * step.previous.q[id][0] - if step.trap { step.previous.dq[id][0] } else { 0. });
                },
                Kind::V(source) | Kind::I(source) => {
                    let value = bias.source.filter(|(i, _)| *i == id).map(|(_, v)| v)
                        .unwrap_or_else(|| source.value(bias.time, self.step, self.stop)) * bias.scale;
                    if matches!(e.kind, Kind::V(_)) { branch(s, e); s.rhs(e.branch, value); }
                    else { current(s, a, b, value); }
                }
                Kind::L(inductance, _) => {
                    branch(s, e);
                    if let Some(step) = step {
                        let factor = step.factor();
                        s.add(e.branch, e.branch, -factor * inductance);
                        s.rhs(e.branch, -factor * step.previous.q[id][0] - if step.trap { step.previous.dq[id][0] } else { 0. });
                    }
                }
                Kind::E(gain) => { branch(s, e); s.add(e.branch, index(cp), -gain); s.add(e.branch, index(cn), *gain); }
                Kind::G(gain) => {
                    s.add(index(a), index(cp), *gain); s.add(index(a), index(cn), -gain);
                    s.add(index(b), index(cp), -gain); s.add(index(b), index(cn), *gain);
                }
                Kind::Device(model) => {
                    let mut eval = model.evaluate(e.voltages(x), previous[id], bias.gmin, limit);
                    limited |= eval.limited; previous[id] = eval.voltage;
                    if let Some(step) = step {
                        if let Some(q) = model.gate_charge(eval.voltage, e.voltages(&step.previous.x), step.previous.gate_q[id]) {
                            for (charge, gate) in eval.charge.iter_mut().zip(q) { *charge = *charge + gate; }
                        }
                    }
                    for terminal in 0..model.terminal_count() {
                        let mut i = eval.current[terminal];
                        if let Some(step) = step {
                            i = i + (eval.charge[terminal] - step.previous.q[id][terminal]) * step.factor()
                                - if step.trap { step.previous.dq[id][terminal] } else { 0. };
                        }
                        let row = index(e.nodes[terminal]);
                        let mut rhs = -i.v;
                        for k in 0..model.terminal_count() { s.add(row, index(e.nodes[k]), i.d[k]); rhs += i.d[k] * eval.voltage[k]; }
                        s.rhs(row, rhs);
                    }
                }
            }
        }
        // Gmin stepping temporarily shunts nodes as well as semiconductor junctions.
        let shunt = (bias.gmin - self.c.options.gmin).max(0.);
        if shunt > 0. { for n in 0..self.c.nodes.len() - 1 { s.add(n, n, shunt); } }
        if let Some(clamps) = bias.clamps {
            for (&node, &value) in clamps {
                if node == 0 { continue; }
                let row = node - 1;
                s.a[row * s.n..(row + 1) * s.n].fill(0.); s.a[row * s.n + row] = 1.; s.b[row] = value;
            }
        }
        limited
    }
    fn newton(&mut self, initial: &[f64], bias: Bias<'_>, step: Option<Step<'_>>, iterations: usize) -> Option<Vec<f64>> {
        let mut x = initial.to_vec();
        let mut previous: Vec<_> = self.c.elements.iter().map(|e| e.voltages(&x)).collect();
        let opts = &self.c.options;
        let mut polished = false;
        for _ in 0..iterations {
            let limited = self.assemble(&x, &mut previous, bias, step, true);
            let next = self.system.solve()?;
            if !self.nonlinear { return Some(next); }
            let converged = !limited && x.iter().zip(&next).enumerate().all(|(i, (a, b))| {
                let absolute = if i < self.c.nodes.len() - 1 { opts.vntol } else { opts.abstol };
                (a - b).abs() <= absolute + opts.reltol * a.abs().max(b.abs())
            });
            x = next;
            if converged {
                // Check the actual, unlimited KCL/KVL residual, then take one polishing iteration.
                self.assemble(&x, &mut previous, bias, step, false);
                let residual_ok = self.system.a.chunks_exact(self.c.size.max(1)).zip(&self.system.b).enumerate().all(|(row, (a, b))| {
                    let residual = a.iter().zip(&x).map(|(a, b)| a * b).sum::<f64>() - b;
                    let scale = a.iter().zip(&x).map(|(a, b)| (a * b).abs()).sum::<f64>();
                    let absolute = if row < self.c.nodes.len() - 1 { opts.abstol } else { opts.vntol };
                    residual.abs() <= absolute + opts.reltol * scale
                });
                if residual_ok && polished { return Some(x); }
                polished = residual_ok;
            } else { polished = false; }
        }
        None
    }
    pub fn op(&mut self, initial: Option<&[f64]>, source: Option<(usize, f64)>, clamp: bool, time: Option<f64>) -> Result<Vec<f64>, String> {
        let zero = vec![0.; self.c.size];
        let initial = initial.unwrap_or(&zero);
        let bias = Bias { time, source, scale: 1., gmin: self.c.options.gmin, clamps: clamp.then_some(&self.c.ic) };
        let iterations = self.c.options.itl1;
        if let Some(x) = self.newton(initial, bias, None, iterations) { return Ok(x); }
        let mut x = initial.to_vec();
        let mut gmin = 1e-2f64.max(bias.gmin * 10.);
        let mut success = true;
        for _ in 0..20 {
            if let Some(next) = self.newton(&x, Bias { gmin, ..bias }, None, iterations) { x = next; }
            else { success = false; break; }
            if gmin == bias.gmin { break; }
            gmin = (gmin * 0.1).max(bias.gmin);
            if gmin < 1e-18 { gmin = bias.gmin; }
        }
        if success { if let Some(x) = self.newton(&x, bias, None, iterations) { return Ok(x); } }
        x.fill(0.);
        if let Some(start) = self.newton(&x, Bias { scale: 0., ..bias }, None, iterations) {
            x = start;
            let mut scale = 0.; let mut increment: f64 = 0.1;
            for _ in 0..200 {
                let next_scale = (scale + increment).min(1.);
                if let Some(next) = self.newton(&x, Bias { scale: next_scale, ..bias }, None, iterations) {
                    x = next; scale = next_scale;
                    if scale == 1. { return Ok(x); }
                    increment = (increment * 1.5).min(0.2);
                } else { increment *= 0.5; if increment < 1e-6 { break; } }
            }
        }
        Err("operating point did not converge (gmin and source stepping failed)".into())
    }
    fn state(&self, time: f64, x: Vec<f64>, step: Option<Step<'_>>) -> State {
        let mut q = vec![[0.; 4]; self.c.elements.len()];
        let mut dq = q.clone();
        let mut gate_q = q.clone();
        for (id, e) in self.c.elements.iter().enumerate() {
            let v = e.voltages(&x);
            match &e.kind {
                Kind::C(cap, _) => { q[id][0] = cap * (v[0] - v[1]); q[id][1] = -q[id][0]; }
                Kind::L(inductance, _) => { q[id][0] = inductance * x[e.branch]; dq[id][0] = v[0] - v[1]; }
                Kind::Device(model) => {
                    q[id] = model.evaluate(v, v, self.c.options.gmin, false).charge.map(|q| q.v);
                    if let Some(step) = step {
                        if let Some(gate) = model.gate_charge(v, e.voltages(&step.previous.x), step.previous.gate_q[id]) {
                            gate_q[id] = gate.map(|q| q.v);
                            for (charge, gate) in q[id].iter_mut().zip(gate_q[id]) { *charge += gate; }
                        }
                    }
                }
                _ => {}
            }
            if !matches!(e.kind, Kind::L(_, _)) {
                if let Some(step) = step {
                    for k in 0..4 { dq[id][k] = step.factor() * (q[id][k] - step.previous.q[id][k]) - if step.trap { step.previous.dq[id][k] } else { 0. }; }
                }
            }
        }
        State { time, x, q, dq, gate_q }
    }
    pub fn transient(&mut self, tran: &Tran) -> Result<Vec<Row>, String> {
        self.step = tran.step; self.stop = tran.stop;
        let mut x = if tran.uic { vec![0.; self.c.size] } else { self.op(None, None, !self.c.ic.is_empty(), Some(0.))? };
        if tran.uic {
            for (&node, &value) in &self.c.ic { if node != 0 { x[node - 1] = value; } }
            for e in &self.c.elements {
                match &e.kind {
                    Kind::C(_, Some(ic)) => {
                        let v = e.voltages(&x);
                        if e.nodes[0] != 0 { x[e.nodes[0] - 1] = v[1] + ic; }
                        else if e.nodes[1] != 0 { x[e.nodes[1] - 1] = -ic; }
                    }
                    Kind::L(_, Some(ic)) => x[e.branch] = *ic,
                    Kind::V(source) if e.nodes[1] == 0 && e.nodes[0] != 0 => x[e.nodes[0] - 1] = source.value(Some(0.), tran.step, tran.stop),
                    Kind::V(source) if e.nodes[0] == 0 && e.nodes[1] != 0 => x[e.nodes[1] - 1] = -source.value(Some(0.), tran.step, tran.stop),
                    _ => {}
                }
            }
        }
        let mut history = VecDeque::from([self.state(0., x, None)]);
        let mut rows = vec![];
        if tran.start == 0. { rows.push(Row { axis: 0., x: history[0].x.clone() }); }
        let mut h = tran.max.min(tran.step) * 0.01;
        let floor = (tran.stop * 1e-14).max(1e-18);
        let mut restart = true;
        // shortcut: 200000 attempted steps and 2 million saved scalars, stream output for longer simulations.
        for _ in 0..200000 {
            let previous = &history[0];
            let t = previous.time;
            if t >= tran.stop { return Ok(rows); }
            let mut breakpoint = tran.stop;
            if t < tran.start { breakpoint = tran.start; }
            for e in &self.c.elements { if let Kind::V(source) | Kind::I(source) = &e.kind { breakpoint = breakpoint.min(source.breakpoint(t, tran.step, tran.stop)); } }
            let remaining = breakpoint - t;
            let at_break = h >= remaining * (1. - 1e-12);
            let dt = if at_break { remaining } else { h };
            let time = if at_break { breakpoint } else { t + dt };
            if dt < floor || time <= t { return Err(format!("timestep too small at t={}", crate::jsfmt::number(t))); }
            let step = Step { h: dt, trap: !restart, previous };
            let bias = Bias { time: Some(time), source: None, scale: 1., gmin: self.c.options.gmin, clamps: None };
            let Some(x) = self.newton(&previous.x, bias, Some(step), self.c.options.itl4) else {
                h = dt * 0.25; restart = true; continue;
            };
            let next = self.state(time, x, Some(step));
            let error = if restart {
                // Step doubling estimates backward Euler's LTE without assuming a device time constant.
                let half = Step { h: dt * 0.5, trap: false, previous };
                let Some(midpoint) = self.newton(&previous.x, Bias { time: Some(t + dt * 0.5), ..bias }, Some(half), self.c.options.itl4) else {
                    h = dt * 0.25; continue;
                };
                let midpoint = self.state(t + dt * 0.5, midpoint, Some(half));
                let half = Step { previous: &midpoint, ..half };
                let Some(fine) = self.newton(&midpoint.x, bias, Some(half), self.c.options.itl4) else {
                    h = dt * 0.25; continue;
                };
                let fine = self.state(time, fine, Some(half));
                let mut error: f64 = 0.;
                for (id, element) in self.c.elements.iter().enumerate() {
                    for k in 0..4 {
                        let scale = next.q[id][k].abs().max(fine.q[id][k].abs());
                        let tolerance = self.charge_tolerance(element, scale, dt);
                        if tolerance > 0. { error = error.max((next.q[id][k] - fine.q[id][k]).abs() / tolerance); }
                    }
                }
                error
            } else if history.len() >= 3 { self.truncation(&next, &history, dt) } else { 0. };
            if error > 1. { h = dt * (0.8 * error.powf(if restart { -0.5 } else { -1. / 3. })).clamp(0.1, 0.5); continue; }
            if time >= tran.start {
                if (rows.len() + 1) * (self.c.size + 1) > 2_000_000 { return Err("simulation output too large (limit 2000000 values)".into()); }
                rows.push(Row { axis: time, x: next.x.clone() });
            }
            history.push_front(next); history.truncate(3);
            restart = at_break;
            if restart { history.truncate(1); h = (dt * 0.1).min(tran.max); }
            else { h = (dt * if error > 0. { (0.9 * error.powf(-1. / 3.)).clamp(0.5, 2.) } else { 2. }).min(tran.max); }
        }
        Err("transient step limit exceeded (limit 200000)".into())
    }
    fn charge_tolerance(&self, element: &Element, magnitude: f64, h: f64) -> f64 {
        let absolute = match element.kind { Kind::L(l, _) => l * self.c.options.abstol, Kind::C(cap, _) => cap * self.c.options.vntol, _ => self.c.options.abstol * h };
        absolute + self.c.options.reltol * magnitude
    }
    fn truncation(&self, next: &State, history: &VecDeque<State>, h: f64) -> f64 {
        let [a, b, c] = [&history[0], &history[1], &history[2]];
        let mut error: f64 = 0.;
        for (id, element) in self.c.elements.iter().enumerate() {
            if !matches!(element.kind, Kind::C(_, _) | Kind::L(_, _) | Kind::Device(_)) { continue; }
            for k in 0..4 {
                let values = [next.q[id][k], a.q[id][k], b.q[id][k], c.q[id][k]];
                let times = [next.time, a.time, b.time, c.time];
                let mut dd = values;
                for order in 1..4 { for i in 0..4 - order { dd[i] = (dd[i] - dd[i + 1]) / (times[i] - times[i + order]); } }
                let lte = (dd[0] * h * h * h * 0.5).abs();
                let magnitude = values.iter().fold(0f64, |m, v| m.max(v.abs()));
                let tolerance = self.charge_tolerance(element, magnitude, h);
                if tolerance > 0. { error = error.max(lte / tolerance); }
            }
        }
        error
    }
}

// Keep this assertion close to the solver: pivoting must handle a zero leading diagonal.
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dense_lu_pivots_and_reuses_factors() {
        let mut s = System::new(3);
        s.a = vec![0., 2., 1., 1., -2., -3., 2., 3., 1.]; s.b = vec![7., -12., 11.];
        for _ in 0..2 { let x = s.solve().unwrap(); for (actual, expected) in x.iter().zip([1., 2., 3.]) { assert!((actual - expected).abs() < 1e-12); } }
    }
}
