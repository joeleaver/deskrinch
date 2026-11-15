//! [`fit`]: which buttons are drawn in the room the panel gives the
//! taskbar, and how.

use crate::Button;

/// The room, and the sizes buttons come in, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    /// The taskbar's width.
    pub width: f32,
    /// The narrowest a button with its title is drawn; narrower, it shows
    /// its icon alone.
    pub titled_min: f32,
    /// The widest a button with its title is drawn.
    pub titled_max: f32,
    /// A button with its icon alone.
    pub icon_only: f32,
    /// Between two buttons, and between the last and the count.
    pub gap: f32,
    /// The count of the windows left out, at the end.
    pub count_width: f32,
}

/// How the buttons drawn are drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Form {
    /// With their titles (cut to fit), each this wide.
    Titled {
        /// Each button's width.
        width: f32,
    },
    /// With their icons alone, each [`Room::icon_only`] wide.
    IconOnly,
}

/// The buttons drawn, and how many are left out.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit<Id> {
    /// How they are drawn.
    pub form: Form,
    /// The windows whose buttons are drawn, in the buttons' order.
    pub shown: Vec<Id>,
    /// How many are left out: the oldest that are not active. When it is
    /// not 0, the count is drawn after the buttons.
    pub left_out: usize,
}

/// Which of `buttons` fit in `room`, and how: each as wide as an equal share
/// of the room up to [`Room::titled_max`]; if that is below
/// [`Room::titled_min`], with their icons alone; and if those do not fit
/// either, as many icons as fit beside the count, the active window's always
/// among them and the rest the newest.
#[must_use]
pub fn fit<Id: Copy>(buttons: &[Button<Id>], room: Room) -> Fit<Id> {
    let all = || buttons.iter().map(|button| button.id).collect();
    if buttons.is_empty() {
        return Fit {
            form: Form::IconOnly,
            shown: Vec::new(),
            left_out: 0,
        };
    }
    let count = buttons.len();
    let share = (room.width - gaps(count, room.gap)) / count_f32(count);
    if share >= room.titled_min {
        return Fit {
            form: Form::Titled {
                width: share.min(room.titled_max),
            },
            shown: all(),
            left_out: 0,
        };
    }
    if icons(count, room) <= room.width {
        return Fit {
            form: Form::IconOnly,
            shown: all(),
            left_out: 0,
        };
    }
    let room_for = (0..count)
        .rev()
        .find(|&kept| icons(kept, room) + room.gap + room.count_width <= room.width)
        .unwrap_or(0);
    Fit {
        form: Form::IconOnly,
        shown: kept(buttons, room_for),
        left_out: count - room_for,
    }
}

/// `kept` of `buttons`, in their order: the active one, and the newest of
/// the others.
fn kept<Id: Copy>(buttons: &[Button<Id>], kept: usize) -> Vec<Id> {
    if kept == 0 {
        return Vec::new();
    }
    let active = buttons.iter().position(|button| button.active);
    let others = kept - usize::from(active.is_some());
    let mut chosen: Vec<usize> = (0..buttons.len())
        .rev()
        .filter(|&index| Some(index) != active)
        .take(others)
        .chain(active)
        .collect();
    chosen.sort_unstable();
    chosen.into_iter().map(|index| buttons[index].id).collect()
}

/// The width `count` icon-only buttons take, with the gaps between them.
fn icons(count: usize, room: Room) -> f32 {
    count_f32(count) * room.icon_only + gaps(count, room.gap)
}

/// The gaps between `count` buttons.
fn gaps(count: usize, gap: f32) -> f32 {
    count_f32(count.saturating_sub(1)) * gap
}

/// `count` as a width multiplier. A taskbar has tens of buttons, far below
/// where `f32` loses whole numbers.
fn count_f32(count: usize) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "tens of buttons")]
    let count = count as f32;
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buttons(count: u32, active: Option<u32>) -> Vec<Button<u32>> {
        (0..count)
            .map(|id| Button {
                id,
                title: format!("window {id}"),
                app_id: String::new(),
                active: Some(id) == active,
                minimized: false,
                attention: false,
            })
            .collect()
    }

    /// 100 to 200 pixels titled, 32 for an icon, 4 between, 40 for the
    /// count.
    fn room(width: f32) -> Room {
        Room {
            width,
            titled_min: 100.0,
            titled_max: 200.0,
            icon_only: 32.0,
            gap: 4.0,
            count_width: 40.0,
        }
    }

    #[test]
    fn nothing_to_show_fits() {
        let fit = fit::<u32>(&[], room(500.0));
        assert_eq!(fit.shown, Vec::<u32>::new());
        assert_eq!(fit.left_out, 0);
    }

    #[test]
    fn titled_buttons_share_the_room_up_to_their_widest() {
        let wide = fit(&buttons(2, None), room(1000.0));
        assert_eq!(wide.form, Form::Titled { width: 200.0 });
        assert_eq!(wide.shown, [0, 1]);
        // Three in 340: (340 - 8) / 3 = 110 each.
        let shared = fit(&buttons(3, None), room(340.0));
        assert!(matches!(shared.form, Form::Titled { width } if (width - 110.666).abs() < 0.01));
        assert_eq!(shared.left_out, 0);
    }

    #[test]
    fn below_the_narrowest_titled_they_show_their_icons() {
        // Four in 300: 73 each titled, too narrow; 4 icons take 140.
        let icons = fit(&buttons(4, None), room(300.0));
        assert_eq!(icons.form, Form::IconOnly);
        assert_eq!(icons.shown, [0, 1, 2, 3]);
        assert_eq!(icons.left_out, 0);
    }

    #[test]
    fn when_icons_do_not_fit_the_oldest_are_left_out_and_counted() {
        // Ten icons take 356; in 200, k icons and the count fit while
        // 36k - 4 + 44 <= 200: k = 4.
        let fit = fit(&buttons(10, None), room(200.0));
        assert_eq!(fit.form, Form::IconOnly);
        assert_eq!(fit.shown, [6, 7, 8, 9], "the newest");
        assert_eq!(fit.left_out, 6);
    }

    #[test]
    fn the_active_window_is_never_left_out() {
        let fit = fit(&buttons(10, Some(1)), room(200.0));
        assert_eq!(fit.shown, [1, 7, 8, 9], "the active one, then the newest");
        assert_eq!(fit.left_out, 6);
    }

    #[test]
    fn with_no_room_at_all_everything_is_counted() {
        let fit = fit(&buttons(3, Some(0)), room(10.0));
        assert_eq!(fit.shown, Vec::<u32>::new());
        assert_eq!(fit.left_out, 3);
    }
}
