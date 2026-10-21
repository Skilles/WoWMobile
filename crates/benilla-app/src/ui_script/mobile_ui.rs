//! Loads the mobile fork's interface ([`crate::mobile`]) after benilla's layer, in the glue and in
//! the world, when the client runs as the mobile client.

use benilla_ui::script::UiScript;

/// Load the files `manifest` (in `assets/mobile`) names, after telling the VM it is the mobile
/// client (`BENILLA_MOBILE`); nothing when [`crate::mobile::enabled`] says no.
pub(super) fn load(script: &UiScript, manifest: &str) -> Vec<String> {
    if !crate::mobile::enabled() {
        return Vec::new();
    }
    if let Err(e) = script.run("BENILLA_MOBILE = 1") {
        return vec![format!("benilla-mobile: {e}")];
    }
    if manifest == crate::mobile::GLUE_MANIFEST {
        if let Err(e) = crate::mobile::launcher::install(script) {
            return vec![format!("benilla-mobile: the launcher's functions: {e}")];
        }
    }
    let mobile = super::addons::Addon::mobile(manifest);
    mobile.load_files(script, &mobile.toc.files)
}

#[cfg(test)]
mod tests {
    use benilla_ui::script::UiScript;

    /// The options window with the mobile page over it, as the mobile client loads them.
    fn options_with_mobile_page() -> UiScript {
        let s = super::super::keybindings_tests::harness();
        super::super::test_ui::load_ui_strict(&s, "MobileOptions.xml");
        assert!(s.errors().is_empty(), "script errors: {:?}", s.errors());
        s
    }

    /// The stock action bar with the touch controls over it.
    fn controls() -> UiScript {
        let mut s = UiScript::new().unwrap();
        s.set_screen_size(1365.0, 768.0);
        super::super::action_bar_tests::load_action_bar(&s);
        super::super::test_ui::load_ui_strict(&s, "MobileControls.xml");
        assert!(s.errors().is_empty(), "script errors: {:?}", s.errors());
        let _ = &mut s;
        s
    }

    /// A share of the screen holds under the UI scale: `GetScreenWidth` is the UIParent units
    /// children are placed in.
    #[test]
    fn an_element_sits_at_its_share_of_the_screen_under_the_ui_scale() {
        benilla_formats::wow_data_or_skip!();
        let s = controls();
        s.run("UIParent:SetScale(0.9); BenillaMobile.seen = {}; BenillaMobile_Poll()")
            .unwrap();
        let share: f64 = s
            .eval("local x = BenillaMobileMenuButton:GetCenter(); return x / GetScreenWidth()")
            .unwrap();
        benilla_ui::assert_near!(share, 0.935, "the menu button's share of the width");
    }

    #[test]
    fn the_ring_slots_show_the_chosen_bar() {
        benilla_formats::wow_data_or_skip!();
        let s = controls();
        let id = |k: u32| -> i64 {
            s.eval(&format!(
                "return ActionButton_GetPagedID(BenillaMobileRingSlot{k})"
            ))
            .unwrap()
        };
        // The main bar pages.
        assert_eq!(id(1), 1);
        s.run("CURRENT_ACTIONBAR_PAGE = 2").unwrap();
        assert_eq!(id(1), 13);
        s.run("CURRENT_ACTIONBAR_PAGE = 1").unwrap();
        // The right bar, 25-36.
        s.run(r#"SetCVar("mobileRingBar", "3")"#).unwrap();
        assert_eq!(id(1), 25);
        assert_eq!(id(8), 32);
        // The stock bar's own buttons still page as before.
        assert_eq!(
            s.eval::<i64>("return ActionButton_GetPagedID(ActionButton1)")
                .unwrap(),
            1
        );
        assert!(s.errors().is_empty(), "script errors: {:?}", s.errors());
    }

    #[test]
    fn a_view_shows_its_controls() {
        benilla_formats::wow_data_or_skip!();
        let s = controls();
        s.run("BenillaMobile_Poll()").unwrap();
        let shown = |f: &str| -> bool { s.eval(&format!("return {f}:IsShown() == 1")).unwrap() };
        assert!(shown("BenillaMobileRing") && shown("BenillaMobileRingSlot1"));
        assert!(shown("BenillaMobileMenuButton") && shown("BenillaMobileButton1"));
        // Button 3 is hidden by its default layout.
        assert!(!shown("BenillaMobileButton3"));
        s.run(r#"BenillaMobile_SetView("explore")"#).unwrap();
        assert!(shown("BenillaMobileRing") && !shown("BenillaMobileRingSlot1"));
        assert!(!shown("BenillaMobileButton1") && shown("BenillaMobileButton2"));
        s.run(r#"BenillaMobile_SetView("minimal")"#).unwrap();
        assert!(!shown("BenillaMobileRing") && shown("BenillaMobileMenuButton"));
        s.run(r#"SetCVar("mobileTouchControls", "0"); BenillaMobile_Poll()"#)
            .unwrap();
        assert!(!shown("BenillaMobileMenuButton"));
        assert!(s.errors().is_empty(), "script errors: {:?}", s.errors());
    }

    #[test]
    fn the_layout_round_trips_through_its_cvar() {
        benilla_formats::wow_data_or_skip!();
        let s = controls();
        s.run(
            r#"BenillaMobileLayout_Set("menu", "x", 0.5)
               BenillaMobileLayout_Set("button1", "action", "map")
               BenillaMobileLayout_Set("button3", "hidden", 0)
               BenillaMobileLayout_Save()
               BenillaMobile.layout = {}
               BenillaMobile.layout = BenillaMobileLayout_Parse(GetCVar("mobileLayout"))"#,
        )
        .unwrap();
        let (x, action, hidden): (f64, String, Option<i64>) = s
            .eval(
                r#"local x = BenillaMobile_Get("menu")
                   local _, _, _, _, a = BenillaMobile_Get("button1")
                   local _, _, _, h = BenillaMobile_Get("button3")
                   return x, a, h"#,
            )
            .unwrap();
        benilla_ui::assert_near!(x, 0.5, "the menu's x");
        assert_eq!(action, "map");
        assert_eq!(hidden, Some(0));
        s.run("BenillaMobile_Poll()").unwrap();
        assert_eq!(
            s.eval::<String>("return BenillaMobileButton1Label:GetText()")
                .unwrap(),
            "Map"
        );
        assert!(s.errors().is_empty(), "script errors: {:?}", s.errors());
    }

    #[test]
    fn the_radial_menu_runs_its_entry_and_opens_the_editor() {
        benilla_formats::wow_data_or_skip!();
        let s = controls();
        s.run("BENILLA_TEST_MAP = nil; ToggleWorldMap = function() BENILLA_TEST_MAP = 1 end")
            .unwrap();
        s.run("BenillaMobileRadial_Toggle()").unwrap();
        assert!(s
            .eval::<bool>("return BenillaMobileRadial:IsShown() == 1")
            .unwrap());
        let map: i64 = s
            .eval(
                r#"for i = 1, 10 do
                       local b = getglobal("BenillaMobileRadialEntry" .. i)
                       if b.command == "map" then BenillaMobileRadial_Run(b.command) end
                   end
                   return BENILLA_TEST_MAP"#,
            )
            .unwrap();
        assert_eq!(map, 1);
        assert!(!s
            .eval::<bool>("return BenillaMobileRadial:IsShown() == 1")
            .unwrap());
        s.run("BenillaMobileLayout_Toggle()").unwrap();
        assert!(s
            .eval::<bool>(
                "return BenillaMobileEditor:IsShown() == 1 and BenillaMobileButton3:IsShown() == 1"
            )
            .unwrap());
        s.run("BenillaMobileLayout_Done()").unwrap();
        assert!(s.errors().is_empty(), "script errors: {:?}", s.errors());
    }

    #[test]
    fn the_mobile_page_opens_and_binds_its_cvars() {
        benilla_formats::wow_data_or_skip!();
        let s = options_with_mobile_page();
        s.run(r#"ShowUIPanel(BenillaOptionsFrame); BenillaOptionsFrame_SelectCategory("Mobile")"#)
            .unwrap();
        assert!(s.errors().is_empty(), "on page: {:?}", s.errors());
        assert!(s
            .eval::<bool>("return BenillaOptionsFrameContainerBodyMobile:IsVisible() == 1")
            .unwrap());
        assert_eq!(
            s.eval::<String>("return BenillaOptionsFrameContainerTitle:GetText()")
                .unwrap(),
            "Touch & Controller"
        );
        // A checkbox row shows its CVar: Touch Controls on by default.
        assert!(s
            .eval::<bool>(
                "return BenillaOptionsFrameContainerBodyMobileRowTouchCheck:GetChecked() == 1"
            )
            .unwrap());
    }

    #[test]
    fn the_ui_scale_reaches_160_percent_and_desktop_rows_grey_out() {
        benilla_formats::wow_data_or_skip!();
        let s = options_with_mobile_page();
        let (_, max): (f32, f32) = s
            .eval(
                "return BenillaOptionsFrameContainerBodyGraphicsRowUiScaleControlSlider                 :GetMinMaxValues()",
            )
            .unwrap();
        benilla_ui::assert_near!(max, 1.6, "the UI scale slider's top");
        s.run(
            r#"ShowUIPanel(BenillaOptionsFrame); BenillaOptionsFrame_SelectCategory("Graphics")"#,
        )
        .unwrap();
        assert!(s.errors().is_empty(), "on page: {:?}", s.errors());
        assert!(s
            .eval::<bool>(
                "return BenillaOptionsFrameContainerBodyGraphicsRowVerticalSyncCheck:IsEnabled()                  ~= 1"
            )
            .unwrap());
    }

    /// Every file the two mobile manifests name ships.
    #[test]
    fn the_mobile_manifests_name_shipped_files() {
        for manifest in [crate::mobile::WORLD_MANIFEST, crate::mobile::GLUE_MANIFEST] {
            let toc = super::super::addons::Addon::mobile(manifest).toc;
            assert!(!toc.files.is_empty(), "{manifest} names no files");
            for file in &toc.files {
                assert!(
                    super::super::content::read_mobile(file).is_some(),
                    "{manifest} names {file}, which assets/mobile lacks"
                );
            }
        }
    }
}
