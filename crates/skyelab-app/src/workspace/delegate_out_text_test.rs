//! The refusal an agent reads says where the user is asked once, whatever the other machine's version words it.

use super::not_allowed_text;

#[test]
fn a_refusal_says_where_the_user_is_asked_once() {
    // A 1.8 peer from before this fix ended its reason with its own "the user is asked there".
    for why in [
        "this machine hasn't allowed delegation from that machine and account; the user is asked there",
        "this machine hasn't allowed delegation from that machine and account",
    ] {
        let text = not_allowed_text("desk", why);
        assert_eq!(
            text,
            "desk doesn't allow this: this machine hasn't allowed delegation from that machine and account. \
             The user is asked there (Settings → Sharing & access).",
        );
    }
}
