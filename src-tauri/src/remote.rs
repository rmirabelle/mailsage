/**
 * Remote images. They load only when the user asks for them, for one
 * message. Rust fetches them, not the window, so the fetch sends no cookie,
 * no referring page, and no detail of this computer but its network address.
 * The result reaches the window as image data inside the page.
 *
 * A message must not be able to make Mail Sage call a device on the local
 * network (a router, a printer). So each address must resolve to a public
 * network address, the connection goes to exactly that address, and each
 * redirect is checked the same way.
 */
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use url::Url;

use crate::preview;

const MAX_IMAGES: usize = 60;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 25 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const FETCHES_AT_ONE_TIME: usize = 6;
const TIME_LIMIT: Duration = Duration::from_secs(10);
/** A browser-like name with no detail, so that the fetch does not say "Mail Sage". */
const USER_AGENT: &str = "Mozilla/5.0";

fn is_public_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    !(address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_unspecified()
        || address.is_multicast()
        || address.is_documentation()
        || octets[0] == 0
        /* Shared address space for carrier networks (100.64.0.0/10). */
        || (octets[0] == 100 && (octets[1] & 0xc0) == 64)
        /* Protocol assignments (192.0.0.0/24) and benchmark networks (198.18.0.0/15). */
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 198 && (octets[1] & 0xfe) == 18)
        || octets[0] >= 240)
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_v4(mapped);
    }
    let first = address.segments()[0];
    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        /* Unique local (fc00::/7), link local (fe80::/10), and the documentation range (2001:db8::/32). */
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first == 0x2001 && address.segments()[1] == 0x0db8)
        /* Addresses that embed an IPv4 address (64:ff9b::/96): judged by that address. */
        || (first == 0x0064 && address.segments()[1] == 0xff9b && !is_public_v4(Ipv4Addr::from((address.segments()[6] as u32) << 16 | address.segments()[7] as u32))))
}

/** True for an address on the public internet: not this computer, not the local network. */
pub fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_v4(address),
        IpAddr::V6(address) => is_public_v6(address),
    }
}

/** The network address to connect to for a web address. Fails if any of the name's addresses is not public. */
async fn public_address(url: &Url) -> Result<(String, SocketAddr)> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("not a web address");
    }
    let host = url.host_str().context("no site name")?.to_string();
    let port = url.port_or_known_default().context("no port")?;
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.trim_matches(|character| character == '[' || character == ']'), port)).await?.collect();
    if addresses.is_empty() || !addresses.iter().all(|address| is_public(address.ip())) {
        bail!("the address is not on the public internet");
    }
    Ok((host, addresses[0]))
}

/** Fetches one image and returns its bytes. */
async fn fetch_image(address: &str) -> Result<Vec<u8>> {
    let mut url = Url::parse(address)?;
    for _ in 0..=MAX_REDIRECTS {
        let (host, socket) = public_address(&url).await?;
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIME_LIMIT)
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&host, socket)
            .build()?;
        let response = client.get(url.clone()).send().await?;
        if response.status().is_redirection() {
            let location = response.headers().get(reqwest::header::LOCATION).and_then(|value| value.to_str().ok()).context("a redirect with no address")?;
            url = url.join(location)?;
            continue;
        }
        if !response.status().is_success() {
            bail!("the server returned {}", response.status());
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk?);
            if bytes.len() > MAX_IMAGE_BYTES {
                bail!("the image is too large");
            }
        }
        return Ok(bytes);
    }
    bail!("too many redirects")
}

/**
 * Fetches the remote images of one message. The result has a `data:` address
 * for each image that loaded and is a real image; the others are left out.
 */
pub async fn fetch_images(addresses: &[String]) -> HashMap<String, String> {
    let fetched: Vec<(String, Result<Vec<u8>>)> = futures_util::stream::iter(addresses.iter().take(MAX_IMAGES).cloned())
        .map(|address| async move {
            let result = fetch_image(&address).await;
            (address, result)
        })
        .buffer_unordered(FETCHES_AT_ONE_TIME)
        .collect()
        .await;
    let mut images = HashMap::new();
    let mut total = 0;
    for (address, result) in fetched {
        let Ok(bytes) = result else { continue };
        if total + bytes.len() > MAX_TOTAL_BYTES {
            continue;
        }
        if let Some(data) = preview::data_address(&bytes) {
            total += bytes.len();
            images.insert(address, data);
        }
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(address: &str) -> bool {
        is_public(address.parse().unwrap())
    }

    #[test]
    fn refuses_addresses_of_this_computer_and_the_local_network() {
        for address in [
            "127.0.0.1", "10.0.0.5", "172.16.3.4", "192.168.1.1", "169.254.169.254", "0.0.0.0", "100.64.0.1", "224.0.0.1", "255.255.255.255",
            "198.18.0.1", "::1", "::", "fe80::1", "fc00::1", "fd12:3456::1", "::ffff:192.168.1.1", "::ffff:127.0.0.1", "64:ff9b::c0a8:101", "ff02::1",
        ] {
            assert!(!public(address), "{address}");
        }
    }

    #[test]
    fn accepts_public_addresses() {
        for address in ["8.8.8.8", "142.250.80.46", "2607:f8b0:4004:c07::8a", "::ffff:8.8.8.8"] {
            assert!(public(address), "{address}");
        }
    }

    #[tokio::test]
    async fn refuses_to_fetch_from_local_names_and_other_schemes() {
        for address in ["http://localhost/x.png", "http://127.0.0.1:8080/x.png", "https://[::1]/x.png", "http://192.168.1.1/admin?reboot=1", "file:///C:/x.png", "ftp://example.com/x.png"] {
            assert!(fetch_image(address).await.is_err(), "{address}");
        }
    }
}
