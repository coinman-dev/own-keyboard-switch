//! Popup coordinates in the desktop's logical monitor space.

/// Keep a window inside one monitor. The origin may be negative.
pub(crate) fn fit(point: [i32; 2], area: [i32; 4], size: [i32; 2]) -> [i32; 2] {
    let [x, y, width, height] = area;
    let width = width.max(1);
    let height = height.max(1);
    [
        point[0].clamp(
            x,
            x.saturating_add(width.saturating_sub(size[0].clamp(1, width))),
        ),
        point[1].clamp(
            y,
            y.saturating_add(height.saturating_sub(size[1].clamp(1, height))),
        ),
    ]
}

pub(crate) fn relative(point: [i32; 2], area: [i32; 4]) -> [i32; 2] {
    [
        point[0].saturating_sub(area[0]),
        point[1].saturating_sub(area[1]),
    ]
}

/// X11 input events are root-device pixels; GDK geometry uses application pixels.
pub(crate) fn x11_to_gtk(point: [i32; 2], scale: i32) -> [i32; 2] {
    let scale = scale.max(1);
    [point[0].div_euclid(scale), point[1].div_euclid(scale)]
}

pub(crate) fn gtk_to_x11(point: [i32; 2], scale: i32) -> [i32; 2] {
    let scale = scale.max(1);
    [
        point[0].saturating_mul(scale),
        point[1].saturating_mul(scale),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secondary_and_negative_monitor_origins_and_scale() {
        let second = [1920, -200, 1280, 900];
        assert_eq!(fit([2000, -100], second, [360, 240]), [2000, -100]);
        assert_eq!(fit([3199, 699], second, [360, 240]), [2840, 460]);
        assert_eq!(relative([2840, 460], second), [920, 660]);
        let left = [-1600, 0, 1600, 900];
        assert_eq!(fit([-2000, 800], left, [360, 240]), [-1600, 660]);
        assert_eq!(relative([-1600, 660], left), [0, 660]);
        assert_eq!(fit([300, 300], [0, 0, 200, 100], [360, 240]), [0, 0]);
        assert_eq!(x11_to_gtk([-1921, 601], 2), [-961, 300]);
        assert_eq!(gtk_to_x11([-961, 300], 2), [-1922, 600]);
    }
}
