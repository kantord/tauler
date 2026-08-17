pub struct Cva {
    pub base: &'static str,
    pub variants: &'static [(&'static str, &'static [(&'static str, &'static str)])],
    pub defaults: &'static [(&'static str, &'static str)],
}

impl Cva {
    pub fn resolve(&self, selections: &[(&str, Option<&str>)], extra_tw: &str) -> String {
        let mut result = self.base.to_string();

        for (axis, value_opt) in selections {
            let effective = value_opt.or_else(|| {
                self.defaults
                    .iter()
                    .find(|(d_axis, _)| d_axis == axis)
                    .map(|(_, d_val)| *d_val)
            });

            if let Some(value) = effective {
                if let Some((_, axis_variants)) = self.variants.iter().find(|(a, _)| a == axis) {
                    if let Some((_, classes)) = axis_variants.iter().find(|(v, _)| *v == value) {
                        if !classes.is_empty() {
                            result.push(' ');
                            result.push_str(classes);
                        }
                    }
                }
            }
        }

        if !extra_tw.is_empty() {
            result.push(' ');
            result.push_str(extra_tw);
        }

        result
    }
}
