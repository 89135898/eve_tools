pub fn sde_crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sde_crate_reports_ready() {
        assert!(sde_crate_ready());
    }
}
