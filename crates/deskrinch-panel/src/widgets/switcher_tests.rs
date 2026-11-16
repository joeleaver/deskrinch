use super::*;

fn button(id: u32, current: bool) -> Button<u32> {
    Button {
        id,
        label: (id + 1).to_string(),
        current,
        activatable: true,
    }
}

#[test]
fn runs_become_buttons_with_counts_and_gaps_between_runs() {
    let fitted = vec![
        Fitted {
            before: 2,
            shown: vec![button(2, true)],
            after: 1,
        },
        Fitted {
            before: 0,
            shown: vec![button(7, false)],
            after: 0,
        },
    ];
    let items = items(fitted);
    let keys: Vec<String> = items.iter().map(Item::key).collect();
    assert_eq!(keys, ["before 0", "2", "after 0", "gap 0", "7"]);
    let texts: Vec<String> = items.iter().map(item_text).collect();
    assert_eq!(texts, ["+2", "3", "+1", "", "8"]);
}

#[test]
fn the_current_button_is_marked_and_counts_are_no_buttons() {
    assert_eq!(
        item_class(&Item::Button(button(0, true))),
        "panel-button marked"
    );
    assert_eq!(item_class(&Item::Button(button(0, false))), "panel-button");
    let count: Item<u32> = Item::Count {
        left_out: 3,
        at: "after 0".to_owned(),
    };
    assert_eq!(item_class(&count), "");
}

#[test]
fn every_item_has_its_width_and_no_background_of_its_own() {
    let button = item_style(&Item::Button(button(0, false)));
    assert!(button.contains("width: 28px"), "{button}");
    assert!(!button.contains("background"), "{button}");
    let gap: Item<u32> = Item::Gap { after: 0 };
    assert!(item_style(&gap).contains("width: 4px"));
}
