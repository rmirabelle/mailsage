/**
 * The block list: sender addresses and sender domains whose mail goes to
 * the Junk folder. An entry is a full address (`ann@example.com`) or a
 * domain with the at sign in front (`@example.com`).
 *
 * The match uses the real address of the `From` line, never the display
 * name: a sender can write any name there.
 */
use anyhow::{bail, Result};

const MAX_ENTRY_LENGTH: usize = 320;

/** Makes a clean entry from what the window sends: no spaces around it, small letters. Refuses text that is not an address or a domain. */
pub fn normalize(entry: &str) -> Result<String> {
    let entry = entry.trim().to_lowercase();
    let Some((local, domain)) = entry.split_once('@') else {
        bail!("\"{entry}\" is not an email address or a domain");
    };
    let bad = |text: &str| text.chars().any(|character| character.is_whitespace() || character.is_control() || character == '@');
    if domain.is_empty() || bad(local) || bad(domain) || entry.len() > MAX_ENTRY_LENGTH {
        bail!("\"{entry}\" is not an email address or a domain");
    }
    Ok(entry)
}

/** The domain of an address, with the at sign in front: `@example.com`. */
fn domain_entry(address: &str) -> Option<&str> {
    address.rfind('@').map(|at| &address[at..])
}

/** True when the block list has the sender address, or its domain. Only the exact domain matches, not a domain under it. */
pub fn is_blocked(address: &str, entries: &[String]) -> bool {
    let address = address.trim().to_lowercase();
    if address.is_empty() {
        return false;
    }
    let domain = domain_entry(&address);
    entries.iter().any(|entry| *entry == address || (entry.starts_with('@') && Some(entry.as_str()) == domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| entry.to_string()).collect()
    }

    #[test]
    fn cleans_entries() {
        assert_eq!(normalize("  Ann@Example.COM ").unwrap(), "ann@example.com");
        assert_eq!(normalize("@Example.com").unwrap(), "@example.com");
    }

    #[test]
    fn refuses_text_that_is_not_an_address_or_a_domain() {
        for entry in ["", "example.com", "ann@", "@", "a b@example.com", "ann@exa mple.com", "a@b@c.com", "ann@example.com\r\nx"] {
            assert!(normalize(entry).is_err(), "{entry:?}");
        }
    }

    #[test]
    fn blocks_an_address_without_regard_to_letter_case() {
        let entries = list(&["ann@example.com"]);
        assert!(is_blocked("Ann@Example.com", &entries));
        assert!(!is_blocked("bob@example.com", &entries));
        assert!(!is_blocked("", &entries));
    }

    #[test]
    fn blocks_a_domain_but_not_a_domain_under_it_or_a_look_alike() {
        let entries = list(&["@shop.example"]);
        assert!(is_blocked("news@shop.example", &entries));
        assert!(is_blocked("NEWS@SHOP.EXAMPLE", &entries));
        assert!(!is_blocked("news@mail.shop.example", &entries));
        assert!(!is_blocked("news@notshop.example", &entries));
        assert!(!is_blocked("shop.example@other.example", &entries));
    }
}
