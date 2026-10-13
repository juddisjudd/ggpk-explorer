//! Which patch is live, asked of the game's own patch server the way the client asks.

use crate::settings::Game;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

/// The protocol the clients spoke when this was written; their logs say "Send patching protocol version 7".
const PROTOCOL: u8 = 7;
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REPLY: usize = 16 * 1024;

/// What a patch server says is live.
#[derive(Debug, Clone, PartialEq)]
pub struct LivePatch {
    pub version: String,
    /// Where the patch's files are served, as the client would fetch them.
    pub web_root: String,
}

/// The hosts each client names for its own realm; the Korean ones run their own patches.
fn servers(game: Game) -> &'static [&'static str] {
    match game {
        Game::Poe1 => &["patch.pathofexile.com:12995", "patch2.pathofexile.com:12995", "patch3.pathofexile.com:12995", "patch4.pathofexile.com:12995"],
        Game::Poe2 => &["us.patch.pathofexile2.com:13060", "patch.pathofexile2.com:13060"],
    }
}

/// The protocol number the installed client last sent, so a client update carries this tool along.
pub fn logged_protocol(install_root: &Path) -> Option<u8> {
    const MARKER: &str = "Send patching protocol version ";
    ["logs/LatestClient.txt", "logs/Client.txt"].iter().find_map(|name| {
        let log = crate::data_export::read_tail(&install_root.join(name), 512 * 1024).ok()?;
        let after = &log[log.rfind(MARKER)? + MARKER.len()..];
        after[..after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len())].parse().ok()
    })
}

/// Asks the game's patch servers in turn. `logged` is the protocol number from the install's log, tried first.
pub fn live(game: Game, logged: Option<u8>) -> Result<LivePatch, String> {
    let mut protocols: Vec<u8> = Vec::new();
    for protocol in logged.into_iter().chain([PROTOCOL, PROTOCOL + 1, PROTOCOL + 2, PROTOCOL - 1]) {
        if !protocols.contains(&protocol) {
            protocols.push(protocol);
        }
    }
    let mut failure = format!("no patch server is known for {}", game.label());
    for server in servers(game) {
        let mut reached = false;
        for protocol in &protocols {
            let stream = match connect(server) {
                Ok(stream) => stream,
                Err(why) => {
                    failure = format!("{}: {}", server, why);
                    break;
                }
            };
            reached = true;
            match ask(stream, *protocol) {
                Ok(patch) => return Ok(patch),
                Err(why) => failure = format!("{} (protocol {}): {}", server, protocol, why),
            }
        }
        // A game's servers run one service, so one that answers and names no patch speaks for the rest.
        if reached {
            break;
        }
    }
    Err(failure)
}

fn connect(server: &str) -> Result<TcpStream, String> {
    let address = server.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("the host has no address")?;
    let stream = TcpStream::connect_timeout(&address, TIMEOUT).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    Ok(stream)
}

fn ask(mut stream: TcpStream, protocol: u8) -> Result<LivePatch, String> {
    stream.write_all(&[1, protocol]).map_err(|e| e.to_string())?;

    let mut reply = Vec::new();
    let mut chunk = [0u8; 2048];
    while reply.len() < MAX_REPLY {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => reply.extend_from_slice(&chunk[..n]),
        }
        if let Some(patch) = parse(&reply) {
            return Ok(patch);
        }
    }
    Err(format!("the {}-byte reply names no patch", reply.len()))
}

/// The first web root in a reply that ends in a patch number, found by scanning so a longer header does not break it.
fn parse(reply: &[u8]) -> Option<LivePatch> {
    let scheme: Vec<u8> = "https://".encode_utf16().flat_map(u16::to_le_bytes).collect();
    (0..reply.len().saturating_sub(scheme.len())).filter(|at| reply[*at..].starts_with(&scheme)).find_map(|at| {
        let units = reply[at..].as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair));
        let web_root: String = units.take_while(|unit| (0x21..0x7F).contains(unit)).map(|unit| unit as u8 as char).collect();
        // A web root ends in a slash, so one cut short by a partial read is never taken for a whole one.
        let version = web_root.strip_suffix('/')?.rsplit('/').next()?;
        let numbered = version.split('.').count() >= 3 && version.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
        numbered.then(|| LivePatch { version: version.to_string(), web_root: web_root.clone() })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A reply as the servers send it: 0x02, 32 bytes, then two strings each behind a big-endian character count.
    fn reply(header: usize, roots: [&str; 2]) -> Vec<u8> {
        let mut out = vec![2u8];
        out.resize(header, 0);
        for root in roots {
            out.extend_from_slice(&(root.encode_utf16().count() as u16).to_be_bytes());
            out.extend_from_slice(&wide(root));
        }
        out
    }

    #[test]
    fn a_reply_names_the_live_patch_and_its_web_root() {
        let root = "https://patch-poe2.poecdn.com/0.5.5.4.2/";
        let patch = parse(&reply(33, [root, root])).unwrap();
        assert_eq!(patch, LivePatch { version: "0.5.5.4.2".to_string(), web_root: root.to_string() });
        assert_eq!(parse(&reply(33, ["https://patch.poecdn.com/3.29.3.3/"; 2])).unwrap().version, "3.29.3.3");
    }

    #[test]
    fn a_longer_header_or_another_host_still_reads() {
        let patch = parse(&reply(48, ["https://cdn.example.net/poe/1.2.3/"; 2])).unwrap();
        assert_eq!((patch.version.as_str(), patch.web_root.as_str()), ("1.2.3", "https://cdn.example.net/poe/1.2.3/"));
    }

    #[test]
    fn a_reply_cut_short_names_nothing() {
        let whole = reply(33, ["https://patch.poecdn.com/3.29.3.3/"; 2]);
        let first_ends = 35 + wide("https://patch.poecdn.com/3.29.3.3/").len();
        for cut in 0..first_ends {
            assert_eq!(parse(&whole[..cut]), None, "{} bytes of the reply should not be enough", cut);
        }
        assert!(parse(&whole[..first_ends]).is_some());
        assert_eq!(parse(b"\x02 not a patch server"), None);
    }

    #[test]
    fn the_install_log_says_which_protocol_the_client_speaks() {
        let dir = std::env::temp_dir().join(format!("ggpk-patch-server-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        std::fs::write(dir.join("logs/LatestClient.txt"), "x [INFO Client 1] Send patching protocol version 6\ny [INFO Client 1] Send patching protocol version 8\nz Web root: https://patch.poecdn.com/3.29.3.3/\n").unwrap();
        assert_eq!(logged_protocol(&dir), Some(8));
        std::fs::write(dir.join("logs/LatestClient.txt"), "nothing about patching").unwrap();
        assert_eq!(logged_protocol(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cargo test --release patch_servers_answer -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn patch_servers_answer() {
        for game in [Game::Poe1, Game::Poe2] {
            let patch = live(game, None).unwrap();
            println!("{}: {} at {}", game.label(), patch.version, patch.web_root);
            for server in servers(game) {
                assert_eq!(connect(server).and_then(|stream| ask(stream, PROTOCOL)).as_ref(), Ok(&patch), "{} disagrees", server);
            }
            assert!(patch.web_root.contains(crate::bundles::cdn::host(game)), "the CDN host this app fetches from is not the one the server names");
            assert_eq!(patch.version.starts_with("3."), game == Game::Poe1);
        }
    }
}
