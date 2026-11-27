//! Who an account is signed in as, precisely enough to tell it apart on another machine: provider, email and org id
//! from `claude auth status`. Only Claude logins have one; other providers never prove an identity.

use crate::login_status::claude_status;
use anyhow::Result;
use skyelab_core::delegation_remote::{Login, MAX_EMAIL, MAX_ORG};
use skyelab_core::{Account, Provider};
use std::path::Path;

/// `account`'s login identity, `None` when it is signed out, has no email or org id (an API key), or isn't a Claude
/// account. Blocking: call it off the UI thread.
pub fn login_identity(bin: &Path, account: &Account) -> Result<Option<Login>> {
    if account.provider != Provider::Claude {
        return Ok(None);
    }
    Ok(identity(&claude_status(bin, account)?))
}

fn identity(json: &serde_json::Value) -> Option<Login> {
    if json["loggedIn"].as_bool() != Some(true) {
        return None;
    }
    let field = |k: &str, max: usize| {
        json[k]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= max)
            .map(str::to_owned)
    };
    Some(Login {
        provider: Provider::Claude,
        email: field("email", MAX_EMAIL)?.to_ascii_lowercase(),
        org_id: field("orgId", MAX_ORG)?,
    })
}

#[cfg(test)]
mod tests {
    use super::identity;
    use crate::login_status::claude_json;

    #[test]
    fn reads_email_and_org_id() {
        let real = "{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\",\n  \"email\": \"Me@Example.com\",\n  \"orgId\": \"00000000-0000-0000-0000-000000000000\",\n  \"orgName\": \"Acme\"\n}\n";
        let login = identity(&claude_json(real).expect("json")).expect("identity");
        assert_eq!(login.email, "me@example.com");
        assert_eq!(login.org_id, "00000000-0000-0000-0000-000000000000");
        for none in [
            r#"{"loggedIn":true,"authMethod":"apiKey"}"#,
            r#"{"loggedIn":true,"email":"me@example.com"}"#,
            r#"{"loggedIn":false,"email":"me@example.com","orgId":"o"}"#,
        ] {
            assert_eq!(identity(&claude_json(none).expect("json")), None, "{none}");
        }
    }
}

#[cfg(all(test, unix))]
mod fake_cli_tests {
    use super::login_identity;
    use skyelab_core::{Account, Provider};
    use std::path::Path;

    #[test]
    fn the_fake_reports_its_org_file() {
        let home = tempfile::tempdir().expect("tempdir");
        let dir = home.path().join("a");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let account = Account {
            id: "work".into(),
            label: "Work".into(),
            provider: Provider::Claude,
            config_dir: dir.clone(),
            gh_config_dir: None,
            ssh_agent: false,
        };
        let claude = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../skyelab-app/tests/fixtures/fake-claude.sh");
        std::fs::write(dir.join("fake-login"), "me@acme.com\n").expect("sign in");
        assert_eq!(
            login_identity(&claude, &account).expect("status"),
            None,
            "no org yet"
        );
        std::fs::write(dir.join("fake-org"), "org-acme\n").expect("org");
        let login = login_identity(&claude, &account)
            .expect("status")
            .expect("identity");
        assert_eq!(
            (login.email.as_str(), login.org_id.as_str()),
            ("me@acme.com", "org-acme")
        );
    }
}
