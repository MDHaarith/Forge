use super::{standards::PITCH, Boolean, DVec3, Solid, Tessellation, ToolError};
use cadrum::{Edge, ProfileOrient};

pub(super) fn dimensions(size: &str, index: usize) -> Result<(f64, f64), ToolError> {
    PITCH
        .iter()
        .find(|(name, _)| *name == size)
        .map(|(name, pitch)| (name[1..].parse().unwrap(), *pitch))
        .ok_or_else(|| {
            ToolError::expected(format!(
                "step {index}: thread size must be one of M2, M2.5, M3, M4, M5, M6, M8, M10, M12"
            ))
        })
}

pub(super) fn rod(size: &str, length: f64, index: usize) -> Result<Solid, ToolError> {
    let (d, pitch) = dimensions(size, index)?;
    let failed = |err| ToolError::expected(format!("step {index}: thread failed: {err}"));
    let minor = d / 2.0 - 5.0 * (3.0_f64.sqrt() * pitch / 2.0) / 8.0;
    let base = minor - 0.01;
    let half_base = 3.0 * pitch / 8.0 + 0.01 / 3.0_f64.sqrt();
    let extended = length + 2.0 * pitch;
    if !extended.is_finite() {
        return Err(failed("length must be finite".to_owned()));
    }
    let helix = Edge::helix(base, pitch, extended, DVec3::Z, DVec3::X)
        .map_err(|err| failed(err.to_string()))?
        .translate(-DVec3::Z * pitch);
    let profile = Edge::polygon(&[
        DVec3::new(base, 0.0, -pitch - half_base),
        DVec3::new(d / 2.0, 0.0, -pitch - pitch / 16.0),
        DVec3::new(d / 2.0, 0.0, -pitch + pitch / 16.0),
        DVec3::new(base, 0.0, -pitch + half_base),
    ])
    .map_err(|err| failed(err.to_string()))?;
    let ridge = Solid::sweep(profile.iter(), [&helix], ProfileOrient::Up(DVec3::Z))
        .map_err(|err| failed(err.to_string()))?;
    // Both shapes must extend past the ends or OCCT can fuse only the ridge.
    let core = Solid::cylinder(minor, DVec3::Z * extended).translate(-DVec3::Z * pitch);
    let fused = (Boolean::<Solid>::default() + &core + &ridge)
        .build()
        .map_err(|err| failed(err.to_string()))?;
    let clip = Solid::cube(DVec3::new(-d, -d, 0.0), DVec3::new(d, d, length));
    (Boolean::<Solid>::from(&fused) * &clip)
        .build()
        .map_err(|err| failed(err.to_string()))
}

pub(super) fn mesh(solids: &[Solid], index: usize) -> Result<cadrum::Mesh, ToolError> {
    // OCCT uses loose helical surface bounds until the final solid is meshed.
    Solid::mesh(
        solids,
        Tessellation {
            deflection_linear: 0.005,
            deflection_angular: 0.1,
            relative_linear: false,
        },
    )
    .map_err(|err| ToolError::expected(format!("step {index}: thread failed: {err}")))
}
