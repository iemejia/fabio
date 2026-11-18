mod common;

use common::fabio;

#[test]
fn test_retrieve_rejects_zero_token_budget() {
    fabio()
        .args([
            "microsoft-iq",
            "retrieve",
            "--text",
            "quarterly risks",
            "--token-budget",
            "0",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires a Fabric tenant with Microsoft IQ context"]
fn test_am_i_in_scope_live() {
    fabio()
        .args(["microsoft-iq", "am-i-in-scope"])
        .assert()
        .success();
}
