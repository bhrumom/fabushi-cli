use mahayana_core::BuildProfile;
use mahayana_harness::{HarnessError, MahayanaHarness};
use mahayana_harness_protocol::HarnessApi;
use mahayana_harness_services::HarnessServices;

#[test]
fn from_parts_rejects_split_harness_state() {
    let core = MahayanaHarness::new(BuildProfile::DesktopFull);
    let services = HarnessServices::new(MahayanaHarness::new(BuildProfile::DesktopFull));
    assert!(matches!(
        HarnessApi::from_parts(core, services),
        Err(HarnessError::InvalidConfig(_))
    ));
}
