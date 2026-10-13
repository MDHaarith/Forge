use super::ToolError;

pub(super) const CLEARANCE: &[(&str, [f64; 3])] = &[
    ("M1.6", [1.7, 1.8, 2.0]),
    ("M2", [2.2, 2.4, 2.6]),
    ("M2.5", [2.7, 2.9, 3.1]),
    ("M3", [3.2, 3.4, 3.6]),
    ("M4", [4.3, 4.5, 4.8]),
    ("M5", [5.3, 5.5, 5.8]),
    ("M6", [6.4, 6.6, 7.0]),
    ("M8", [8.4, 9.0, 10.0]),
    ("M10", [10.5, 11.0, 12.0]),
    ("M12", [13.0, 13.5, 14.5]),
];

pub(super) const FITS: &[(&str, usize)] = &[("close", 0), ("normal", 1), ("loose", 2)];

pub(super) const TAP_DRILL: &[(&str, f64)] = &[
    ("M1.6", 1.25),
    ("M2", 1.6),
    ("M2.5", 2.05),
    ("M3", 2.5),
    ("M4", 3.3),
    ("M5", 4.2),
    ("M6", 5.0),
    ("M8", 6.8),
    ("M10", 8.5),
    ("M12", 10.2),
];

pub(super) const PITCH: &[(&str, f64)] = &[
    ("M2", 0.4),
    ("M2.5", 0.45),
    ("M3", 0.5),
    ("M4", 0.7),
    ("M5", 0.8),
    ("M6", 1.0),
    ("M8", 1.25),
    ("M10", 1.5),
    ("M12", 1.75),
];

pub(super) const SOCKET_HEAD: &[(&str, [f64; 2])] = &[
    ("M2", [3.8, 2.0]),
    ("M2.5", [4.5, 2.5]),
    ("M3", [5.5, 3.0]),
    ("M4", [7.0, 4.0]),
    ("M5", [8.5, 5.0]),
    ("M6", [10.0, 6.0]),
    ("M8", [13.0, 8.0]),
    ("M10", [16.0, 10.0]),
    ("M12", [18.0, 12.0]),
];

pub(super) const COUNTERSUNK_HEAD: &[(&str, f64)] = &[
    ("M3", 6.72),
    ("M4", 8.96),
    ("M5", 11.2),
    ("M6", 13.44),
    ("M8", 17.92),
    ("M10", 22.4),
    ("M12", 26.88),
];

pub(super) const INSERT: &[(&str, [f64; 2])] = &[
    ("M2", [3.2, 4.0]),
    ("M2.5", [3.6, 5.7]),
    ("M3", [4.0, 5.7]),
    ("M4", [5.6, 8.1]),
    ("M5", [6.4, 9.5]),
];

pub(super) const MATERIALS: &[(&str, f64)] = &[
    ("PLA", 1.24),
    ("PETG", 1.27),
    ("ABS", 1.04),
    ("ASA", 1.07),
    ("Nylon PA12", 1.01),
    ("Polycarbonate", 1.20),
    ("Acrylic", 1.18),
    ("POM (Delrin)", 1.41),
    ("Aluminium 6061", 2.70),
    ("Aluminium 7075", 2.81),
    ("Steel", 7.85),
    ("Stainless 304", 8.00),
    ("Brass", 8.50),
    ("Copper", 8.96),
    ("Titanium Ti-6Al-4V", 4.43),
    ("Plywood", 0.60),
];

pub(super) fn lookup<T: Copy>(table: &[(&str, T)], size: &str, what: &str) -> Result<T, ToolError> {
    let size = size.to_uppercase().replace(' ', "");
    let key = if size.starts_with('M') {
        size
    } else {
        format!("M{size}")
    };
    table
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
        .ok_or_else(|| {
            ToolError::expected(format!(
                "no {what} data for {key}; known sizes: {}",
                table
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

pub(super) fn clearance(size: &str, fit: &str) -> Result<f64, ToolError> {
    let diameters = lookup(CLEARANCE, size, "clearance hole")?;
    let index = FITS
        .iter()
        .find(|(name, _)| *name == fit)
        .map(|(_, index)| *index)
        .ok_or_else(|| ToolError::expected("fit must be close, normal or loose"))?;
    Ok(diameters[index])
}

pub(super) fn material_density(material: &str) -> Result<f64, ToolError> {
    MATERIALS
        .iter()
        .find(|(name, _)| *name == material)
        .map(|(_, rho)| *rho)
        .ok_or_else(|| {
            ToolError::expected(format!(
                "unknown material \"{material}\"; valid names: {}",
                MATERIALS
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

pub(super) fn density(material: &str) -> Result<f64, ToolError> {
    MATERIALS
        .iter()
        .find(|(name, _)| {
            name.eq_ignore_ascii_case(material)
                || name
                    .split(' ')
                    .next()
                    .unwrap()
                    .eq_ignore_ascii_case(material)
        })
        .map(|(_, rho)| *rho)
        .ok_or_else(|| {
            ToolError::expected(format!(
                "unknown material '{material}'; known: {}",
                MATERIALS
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}
