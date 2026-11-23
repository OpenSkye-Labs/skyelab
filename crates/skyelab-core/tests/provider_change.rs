//! Review fix: an existing account keeps its provider. Its threads hold session ids of that provider,
//! so switching would resume a Claude session id in codex (or the reverse).

use skyelab_core::{Account, AccountsConfig, Provider};

fn account(provider: Provider) -> Account {
    Account {
        id: "acct".into(),
        label: "Acct".into(),
        provider,
        config_dir: "/home/me/.acct".into(),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

#[test]
fn updating_an_account_cannot_change_its_provider() {
    let mut c = AccountsConfig::default();
    c.add_account(account(Provider::Claude)).expect("add");
    let err = c
        .update_account(account(Provider::Codex))
        .expect_err("provider change");
    assert!(format!("{err:#}").contains("provider"), "{err:#}");
    assert_eq!(c.accounts[0].provider, Provider::Claude);
    let mut relabel = account(Provider::Claude);
    relabel.label = "New label".into();
    c.update_account(relabel).expect("other edits still work");
}
