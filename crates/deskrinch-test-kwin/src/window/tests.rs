use super::*;

/// What the script API gave, on KWin 6.6.7 in `cargo xtask session kwin`
/// (2026-11-23), for the test client's toplevel (title "Probe toplevel",
/// app id `org.deskrinch.probe.Toplevel`): every property, of which the
/// record reads its own.
const TOPLEVEL: &str = r#"{"objectName":"","bufferGeometry":[480,240,320,240],"clientGeometry":[480,240,320,240],"pos":"QPointF(480, 240)","size":[320,240],"x":480,"y":240,"width":320,"height":240,"opacity":1,"output":"Virtual-0","rect":[0,0,320,240],"resourceName":"overlay-probe","resourceClass":"org.deskrinch.probe.Toplevel","windowRole":"","desktopWindow":false,"dock":false,"toolbar":false,"menu":false,"normalWindow":true,"dialog":false,"splash":false,"utility":false,"dropdownMenu":false,"popupMenu":false,"tooltip":false,"notification":false,"criticalNotification":false,"appletPopup":false,"onScreenDisplay":false,"comboBox":false,"dndIcon":false,"windowType":0,"managed":true,"deleted":false,"skipsCloseAnimation":false,"popupWindow":false,"outline":false,"internalId":"{a3ee3d7d-ff6a-4d67-813f-a086cc3f00a5}","pid":4165930,"stackingOrder":0,"fullScreen":false,"fullScreenable":true,"active":false,"desktops":"KWin::VirtualDesktop(0x59ee0317b310)","onAllDesktops":false,"activities":"","skipTaskbar":false,"skipPager":false,"skipSwitcher":false,"closeable":true,"icon":"QVariant(QIcon, QIcon(\"wayland\",availableSizes[normal,Off]=QList(QSize(16, 16), QSize(22, 22), QSize(32, 32), QSize(48, 48), QSize(64, 64), QSize(128, 128), QSize(256, 256)),cacheKey=0x400000000))","keepAbove":false,"keepBelow":false,"minimizable":true,"minimized":false,"iconGeometry":[0,0,0,0],"specialWindow":false,"demandsAttention":false,"caption":"Probe toplevel","captionNormal":"Probe toplevel","minSize":[20,20],"maxSize":[2147483647,2147483647],"wantsInput":true,"transient":false,"transientFor":null,"modal":false,"frameGeometry":[480,240,320,240],"move":false,"resize":false,"decorationHasAlpha":false,"noBorder":false,"providesContextHelp":false,"maximizable":true,"maximizeMode":0,"moveable":true,"moveableAcrossScreens":true,"resizeable":true,"desktopFileName":"org.deskrinch.probe.Toplevel","hasApplicationMenu":false,"applicationMenuActive":false,"unresponsive":false,"colorScheme":"kdeglobals","layer":2,"hidden":false,"tile":null,"inputMethod":false,"tag":"","description":"","excludeFromCapture":false}"#;

/// The same run, for the test client's overlay layer surface (namespace
/// `deskrinch-test-overlay`, 64 by 64).
const OVERLAY: &str = r#"{"objectName":"","bufferGeometry":[608,328,64,64],"clientGeometry":[608,328,64,64],"pos":"QPointF(608, 328)","size":[64,64],"x":608,"y":328,"width":64,"height":64,"opacity":1,"output":"Virtual-0","rect":[0,0,64,64],"resourceName":"overlay-probe","resourceClass":"overlay-probe","windowRole":"","desktopWindow":false,"dock":false,"toolbar":false,"menu":false,"normalWindow":true,"dialog":false,"splash":false,"utility":false,"dropdownMenu":false,"popupMenu":false,"tooltip":false,"notification":false,"criticalNotification":false,"appletPopup":false,"onScreenDisplay":false,"comboBox":false,"dndIcon":false,"windowType":0,"managed":true,"deleted":false,"skipsCloseAnimation":false,"popupWindow":false,"outline":false,"internalId":"{8a17bd34-4b46-456f-92f1-f27a2785baf8}","pid":4165930,"stackingOrder":1,"fullScreen":false,"fullScreenable":false,"active":true,"desktops":"","onAllDesktops":true,"activities":"","skipTaskbar":true,"skipPager":true,"skipSwitcher":true,"closeable":true,"icon":"QVariant(QIcon, QIcon(null))","keepAbove":false,"keepBelow":false,"minimizable":false,"minimized":false,"iconGeometry":[0,0,0,0],"specialWindow":false,"demandsAttention":false,"caption":"","captionNormal":"","minSize":[0,0],"maxSize":[2147483647,2147483647],"wantsInput":true,"transient":false,"transientFor":null,"modal":false,"frameGeometry":[608,328,64,64],"move":false,"resize":false,"decorationHasAlpha":false,"noBorder":false,"providesContextHelp":false,"maximizable":false,"maximizeMode":0,"moveable":false,"moveableAcrossScreens":false,"resizeable":false,"desktopFileName":"","hasApplicationMenu":false,"applicationMenuActive":false,"unresponsive":false,"colorScheme":"kdeglobals","layer":9,"hidden":false,"tile":null,"inputMethod":false,"tag":"","description":"","excludeFromCapture":false}"#;

#[test]
fn a_toplevel_reads_as_normal() {
    let window = Window::from_record(TOPLEVEL).unwrap();
    assert_eq!(window.caption, "Probe toplevel");
    assert_eq!(window.resource_class, "org.deskrinch.probe.Toplevel");
    assert_eq!(window.window_type, WindowType::Normal);
    assert_eq!(window.flags, BTreeSet::from([WindowType::Normal]));
    assert_eq!(window.layer, 2);
    assert_eq!(window.output.as_deref(), Some("Virtual-0"));
    assert_eq!(window.size(), (320.0, 240.0));
    assert!(window.skips.is_empty());
}

#[test]
fn a_layer_surface_has_no_caption_and_its_process_s_class() {
    let window = Window::from_record(OVERLAY).unwrap();
    assert_eq!(window.caption, "");
    assert_eq!(window.resource_class, "overlay-probe");
    assert_eq!(window.resource_name, "overlay-probe");
    assert_eq!(window.window_type, WindowType::Normal);
    assert_eq!(window.layer, 9);
    assert_eq!(window.size(), (64.0, 64.0));
    assert_eq!(
        window.skips,
        BTreeSet::from([Skip::Taskbar, Skip::Pager, Skip::Switcher])
    );
}

#[test]
fn kwin_s_numbers_name_its_types() {
    for (number, kind) in [
        (0, WindowType::Normal),
        (1, WindowType::Desktop),
        (2, WindowType::Dock),
        (5, WindowType::Dialog),
        (8, WindowType::Utility),
        (9, WindowType::Splash),
        (12, WindowType::Tooltip),
        (13, WindowType::Notification),
        (16, WindowType::OnScreenDisplay),
        (17, WindowType::CriticalNotification),
    ] {
        assert_eq!(WindowType::from_kwin(number), kind, "{number}");
    }
    assert_eq!(WindowType::from_kwin(6), WindowType::Other(6));
    assert_eq!(WindowType::from_kwin(-1), WindowType::Other(-1));
}

#[test]
fn a_record_that_is_not_one_is_an_error_naming_it() {
    let refusal = Window::from_record(r#"{"caption":"x"}"#).unwrap_err();
    assert!(
        matches!(&refusal, Error::Record { record, .. } if record == r#"{"caption":"x"}"#),
        "{refusal:?}"
    );
}
