use super::*;

fn bounds(x: i32, y: i32, width: i32, height: i32) -> p::CoordinateBounds {
    p::CoordinateBounds {
        schema_version: p::SchemaVersion(1),
        min_x: x,
        min_y: y,
        max_x_exclusive: x + width,
        max_y_exclusive: y + height,
    }
}
fn monitor(id: u32, x: i32, y: i32) -> MonitorGeometry {
    MonitorGeometry {
        id,
        x,
        y,
        width: 1920,
        height: 1080,
    }
}
fn installed() -> p::SurfaceRef {
    p::SurfaceRef(crate::installed::INSTALLED_DESKTOP_SURFACE.into())
}

#[test]
fn installed_surface_resolves_exact_region_on_primary_or_negative_coordinate_monitor() {
    let monitors = [monitor(41, 0, 0), monitor(92, -1920, -240)];
    assert_eq!(
        resolve_capture_region(&installed(), bounds(32, 48, 512, 256), &monitors).unwrap(),
        CaptureRegion {
            monitor_index: 0,
            x: 32,
            y: 48,
            width: 512,
            height: 256
        }
    );
    assert_eq!(
        resolve_capture_region(&installed(), bounds(-1888, -192, 512, 256), &monitors).unwrap(),
        CaptureRegion {
            monitor_index: 1,
            x: 32,
            y: 48,
            width: 512,
            height: 256
        }
    );
    assert_eq!(
        resolve_capture_region(&installed(), bounds(-1920, -240, 1920, 1080), &monitors).unwrap(),
        CaptureRegion {
            monitor_index: 1,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080
        }
    );
}

#[test]
fn installed_surface_refuses_spanning_outside_missing_or_ambiguous_monitors() {
    let monitors = [monitor(41, 0, 0), monitor(92, -1920, 0)];
    for region in [
        bounds(-1, 0, 2, 1),
        bounds(1910, 0, 20, 10),
        bounds(0, 1080, 1, 1),
        bounds(0, 0, 0, 1),
    ] {
        assert!(resolve_capture_region(&installed(), region, &monitors).is_err());
    }
    assert!(resolve_capture_region(&installed(), bounds(0, 0, 10, 10), &[]).is_err());
    assert!(resolve_capture_region(
        &installed(),
        bounds(0, 0, 10, 10),
        &[monitor(1, 0, 0), monitor(2, 0, 0)]
    )
    .is_err());
    assert!(resolve_capture_region(
        &installed(),
        bounds(0, 0, 10, 10),
        &[monitor(1, 0, 0), monitor(1, -1920, 0)]
    )
    .is_err());
}

#[test]
fn explicit_monitor_surface_keeps_exact_identity_without_fallback() {
    let monitors = [monitor(41, 0, 0), monitor(92, -1920, 0)];
    let request = bounds(-1800, 20, 100, 80);
    assert_eq!(
        resolve_capture_region(
            &p::SurfaceRef("surface:monitor:92".into()),
            request,
            &monitors
        )
        .unwrap()
        .monitor_index,
        1
    );
    for name in [
        "surface:monitor:41",
        "surface:monitor:404",
        "surface:monitor:-1",
        "surface:monitor:invalid",
        "surface:other",
    ] {
        assert!(resolve_capture_region(&p::SurfaceRef(name.into()), request, &monitors).is_err());
    }
}

#[test]
fn extreme_native_origins_do_not_overflow_region_arithmetic() {
    let monitors = [MonitorGeometry {
        id: 1,
        x: i32::MIN,
        y: i32::MIN,
        width: u32::MAX,
        height: u32::MAX,
    }];
    let region = resolve_capture_region(
        &installed(),
        bounds(i32::MAX - 1, i32::MAX - 1, 1, 1),
        &monitors,
    )
    .unwrap();
    assert_eq!(
        (region.x, region.y, region.width, region.height),
        (u32::MAX - 1, u32::MAX - 1, 1, 1)
    );
}
