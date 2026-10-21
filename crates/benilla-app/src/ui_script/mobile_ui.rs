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
