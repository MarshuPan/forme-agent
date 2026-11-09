//! Resolve authorized desktop coordinates to one exact native capture region.
use forme_protocol as p;

pub(super) struct MonitorGeometry {
    pub(super) id: u32,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CaptureRegion {
    pub(super) monitor_index: usize,
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// The installed surface names the authenticated interactive desktop. A
/// screenshot must fit entirely on exactly one monitor; never crop, stitch,
/// enlarge to the whole screen, or choose an arbitrary fallback monitor.
pub(super) fn resolve_capture_region(
    surface: &p::SurfaceRef,
    bounds: p::CoordinateBounds,
    monitors: &[MonitorGeometry],
) -> p::Result<CaptureRegion> {
    bounds.validate()?;
    let explicit = if surface.0 == crate::installed::INSTALLED_DESKTOP_SURFACE {
        None
    } else {
        Some(
            surface
                .0
                .strip_prefix("surface:monitor:")
                .ok_or_else(invalid)?
                .parse::<u32>()
                .map_err(|_| invalid())?,
        )
    };
    let mut selected = None;
    let mut ids = std::collections::BTreeSet::new();
    for (monitor_index, monitor) in monitors.iter().enumerate() {
        if monitor.width == 0 || monitor.height == 0 || !ids.insert(monitor.id) {
            return Err(invalid());
        }
        if explicit.is_some_and(|id| id != monitor.id) {
            continue;
        }
        let left = i64::from(monitor.x);
        let top = i64::from(monitor.y);
        let right = left + i64::from(monitor.width);
        let bottom = top + i64::from(monitor.height);
        if i64::from(bounds.min_x) < left
            || i64::from(bounds.min_y) < top
            || i64::from(bounds.max_x_exclusive) > right
            || i64::from(bounds.max_y_exclusive) > bottom
        {
            continue;
        }
        let region = CaptureRegion {
            monitor_index,
            x: u32::try_from(i64::from(bounds.min_x) - left).map_err(|_| invalid())?,
            y: u32::try_from(i64::from(bounds.min_y) - top).map_err(|_| invalid())?,
            width: u32::try_from(i64::from(bounds.max_x_exclusive) - i64::from(bounds.min_x))
                .map_err(|_| invalid())?,
            height: u32::try_from(i64::from(bounds.max_y_exclusive) - i64::from(bounds.min_y))
                .map_err(|_| invalid())?,
        };
        if selected.replace(region).is_some() {
            return Err(invalid());
        }
    }
    selected.ok_or_else(invalid)
}

fn invalid() -> p::Error {
    p::Error("native screenshot surface or exact monitor region is unavailable".into())
}

#[cfg(test)]
#[path = "computer_surface_tests.rs"]
mod tests;
