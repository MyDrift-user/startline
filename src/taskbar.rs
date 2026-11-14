//! Where the menu opens when it's aligned to the taskbar: where Windows' own Start menu would be. Right against the
//! taskbar on whichever edge it's on, and along it at its start or its middle, as Windows' taskbar alignment says.
//! Only arithmetic here, on rectangles in screen pixels as left, top, right and bottom; system.rs asks Windows for them.

/// The edge of the monitor the taskbar is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

/// Which edge a taskbar is on, from where it sits on its monitor. A taskbar that hides itself sits mostly off the
/// screen while hidden; its middle still tells the edge.
pub fn edge_of(bar: [i32; 4], monitor: [i32; 4]) -> Edge {
    let [l, t, r, b] = bar;
    let [ml, mt, mr, mb] = monitor;
    if r - l >= b - t {
        if t + b >= mt + mb { Edge::Bottom } else { Edge::Top }
    } else if l + r >= ml + mr {
        Edge::Right
    } else {
        Edge::Left
    }
}

/// Whether the menu goes at the start of the taskbar rather than its middle, from Windows' TaskbarAl setting: 0 is the
/// start, 1 the middle. Without the setting, Windows 11 centers and older Windows starts at the left.
pub fn at_start(taskbar_al: Option<u32>, windows11: bool) -> bool {
    match taskbar_al {
        Some(value) => value == 0,
        None => !windows11,
    }
}

/// The menu's place and size: `size` wide and high, `gap` away from the taskbar and the monitor's edges. Without a
/// taskbar on this monitor, the menu sits at the bottom, above whatever Windows keeps out of the work area there.
pub fn spot(monitor: [i32; 4], work: [i32; 4], bar: Option<[i32; 4]>, start: bool, size: [i32; 2], gap: i32) -> [i32; 4] {
    let [ml, mt, mr, mb] = monitor;
    let (edge, thick) = match bar {
        Some(bar) => {
            let thick = (bar[2] - bar[0]).min(bar[3] - bar[1]).clamp(0, (mr - ml).min(mb - mt) / 2);
            (edge_of(bar, monitor), thick)
        }
        None => (Edge::Bottom, (mb - work[3]).max(0)),
    };
    let horizontal = matches!(edge, Edge::Bottom | Edge::Top);
    // Room left beside the taskbar, with the gap on every side.
    let (room_w, room_h) = if horizontal { (mr - ml - 2 * gap, mb - mt - thick - 2 * gap) } else { (mr - ml - thick - 2 * gap, mb - mt - 2 * gap) };
    // Never outside the work area either, where other docked bars may take room too.
    let [wl, wt, wr, wb] = work;
    let (w, h) = (size[0].min(room_w).min(wr - wl - 2 * gap).max(1), size[1].min(room_h).min(wb - wt - 2 * gap).max(1));
    let along_x = if start { ml + gap } else { ml + (mr - ml - w) / 2 };
    let along_y = if start { mt + gap } else { mt + (mb - mt - h) / 2 };
    let (x, y) = match edge {
        Edge::Bottom => (along_x, mb - thick - gap - h),
        Edge::Top => (along_x, mt + thick + gap),
        Edge::Left => (ml + thick + gap, along_y),
        Edge::Right => (mr - thick - gap - w, along_y),
    };
    let x = x.clamp(wl + gap, (wr - gap - w).max(wl + gap));
    let y = y.clamp(wt + gap, (wb - gap - h).max(wt + gap));
    [x, y, w, h]
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: [i32; 4] = [0, 0, 2560, 1440];
    const MENU: [i32; 2] = [800, 900];

    #[test]
    fn the_menu_sits_against_the_taskbar_on_every_edge() {
        let cases = [
            // edge, taskbar, centered spot, spot at the start
            (Edge::Bottom, [0, 1392, 2560, 1440], [880, 480], [12, 480]),
            (Edge::Top, [0, 0, 2560, 48], [880, 60], [12, 60]),
            (Edge::Left, [0, 0, 62, 1440], [74, 270], [74, 12]),
            (Edge::Right, [2498, 0, 2560, 1440], [1686, 270], [1686, 12]),
        ];
        for (edge, bar, centered, first) in cases {
            assert_eq!(edge_of(bar, MONITOR), edge);
            let work = MONITOR;
            assert_eq!(spot(MONITOR, work, Some(bar), false, MENU, 12)[..2], centered, "{edge:?}, centered");
            assert_eq!(spot(MONITOR, work, Some(bar), true, MENU, 12)[..2], first, "{edge:?}, at the start");
        }
        // Windows 10's menu has no gap: flush against the taskbar and the left edge.
        assert_eq!(spot(MONITOR, MONITOR, Some([0, 1400, 2560, 1440]), true, [660, 640], 0), [0, 760, 660, 640]);
    }

    #[test]
    fn without_windows_saying_windows_11_centers_and_older_windows_starts_left() {
        assert!(!at_start(None, true));
        assert!(at_start(None, false));
        assert!(at_start(Some(0), true), "the setting wins");
        assert!(!at_start(Some(1), false));
        assert!(!at_start(Some(7), true), "anything else is the middle");
    }

    #[test]
    fn each_monitor_has_its_own_taskbar() {
        // A second monitor to the right with its own taskbar, and one to the left, higher up, with it on top.
        let right = [2560, 200, 4480, 1280];
        assert_eq!(spot(right, right, Some([2560, 1232, 4480, 1280]), false, MENU, 12), [3120, 320, 800, 900]);
        let left = [-1920, -300, 0, 780];
        assert_eq!(spot(left, left, Some([-1920, -300, 0, -252]), true, [600, 500], 12), [-1908, -240, 600, 500]);
    }

    #[test]
    fn a_hiding_taskbar_still_counts_where_it_shows() {
        // Hidden, it's 2 pixels on screen and the rest below; the menu leaves room for it all.
        let hidden = [0, 1438, 2560, 1486];
        assert_eq!(edge_of(hidden, MONITOR), Edge::Bottom);
        assert_eq!(spot(MONITOR, MONITOR, Some(hidden), false, MENU, 12), [880, 480, 800, 900]);
        // On the left, hidden off the side.
        let side = [-60, 0, 2, 1440];
        assert_eq!(edge_of(side, MONITOR), Edge::Left);
        assert_eq!(spot(MONITOR, MONITOR, Some(side), true, MENU, 12)[..2], [74, 12]);
    }

    #[test]
    fn other_docked_bars_keep_their_room() {
        // A bar docked at the top takes 100 pixels of the work area, above the taskbar at the bottom.
        let work = [0, 100, 2560, 1392];
        let bar = [0, 1392, 2560, 1440];
        assert_eq!(spot(MONITOR, work, Some(bar), false, MENU, 12), [880, 480, 800, 900], "room enough: unchanged");
        assert_eq!(spot(MONITOR, work, Some(bar), true, [800, 1400], 12), [12, 112, 800, 1268], "too tall: it stays below that bar");
        // A bar docked beside a taskbar on the left pushes the menu along.
        assert_eq!(spot(MONITOR, [162, 0, 2560, 1440], Some([0, 0, 62, 1440]), true, MENU, 12)[..2], [174, 12]);
    }

    #[test]
    fn without_a_taskbar_it_opens_at_the_bottom_and_always_fits() {
        let work = [0, 0, 2560, 1400];
        assert_eq!(spot(MONITOR, work, None, false, MENU, 12), [880, 488, 800, 900]);
        let small = [0, 0, 1024, 768];
        let [x, y, w, h] = spot(small, small, Some([0, 728, 1024, 768]), false, [1200, 900], 12);
        assert!(x >= 12 && y >= 12 && w <= 1000 && h <= 704 && y + h <= 716, "the menu shrinks to fit: {x} {y} {w} {h}");
    }
}
