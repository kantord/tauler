use tauler::ui::cva::Cva;

mod base_only {
    use super::*;

    const CVA: Cva = Cva {
        base: "bg-card text-foreground",
        variants: &[],
        defaults: &[],
    };

    #[test]
    fn resolve_returns_base() {
        assert_eq!(CVA.resolve(&[], ""), "bg-card text-foreground");
    }

    #[test]
    fn resolve_appends_extra_tw() {
        assert_eq!(
            CVA.resolve(&[], "extra-class"),
            "bg-card text-foreground extra-class"
        );
    }
}

mod single_axis {
    use super::*;

    const CVA: Cva = Cva {
        base: "inline-flex items-center",
        variants: &[(
            "variant",
            &[
                ("default", "bg-primary text-primary-foreground"),
                ("destructive", "bg-destructive text-destructive-foreground"),
            ],
        )],
        defaults: &[("variant", "default")],
    };

    fn resolve_variant(selection: Option<&str>) -> String {
        CVA.resolve(&[("variant", selection)], "")
    }

    #[test]
    fn explicit_selection_appends_variant_classes() {
        assert_eq!(
            resolve_variant(Some("destructive")),
            "inline-flex items-center bg-destructive text-destructive-foreground",
        );
    }

    #[test]
    fn none_falls_back_to_default() {
        assert_eq!(
            resolve_variant(None),
            "inline-flex items-center bg-primary text-primary-foreground",
        );
    }
}

mod multi_axis {
    use super::*;

    const CVA: Cva = Cva {
        base: "inline-flex items-center font-medium",
        variants: &[
            (
                "variant",
                &[
                    ("default", "bg-primary text-primary-foreground"),
                    ("destructive", "bg-destructive text-destructive-foreground"),
                ],
            ),
            (
                "size",
                &[
                    ("sm", "h-8 px-3 text-[12px]"),
                    ("lg", "h-12 px-6 text-[16px]"),
                ],
            ),
        ],
        defaults: &[("variant", "default"), ("size", "sm")],
    };

    #[test]
    fn each_axis_resolved_independently() {
        assert_eq!(
            CVA.resolve(
                &[("variant", Some("destructive")), ("size", Some("lg"))],
                ""
            ),
            "inline-flex items-center font-medium bg-destructive text-destructive-foreground h-12 px-6 text-[16px]",
        );
    }
}
