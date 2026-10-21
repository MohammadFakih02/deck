//! Ping to game servers, measured the way the games do it.
//!
//! - Valve games (CS2, Dota 2, Deadlock, TF2) connect through Valve's relays
//!   (Steam Datagram Relay). Each city has several relays that can differ by
//!   5-10 ms, so every relay is pinged (ICMP) and the best one counts, like
//!   Steam's own route choice.
//! - Fortnite, Rocket League and Dead by Daylight run on Amazon (AWS). Amazon
//!   has UDP ping servers made for games (`gamelift-ping.<region>.api.aws`,
//!   port 7770, echoes the packet). TCP handshakes are no good: some routers
//!   and ISPs answer those themselves, which showed 5 ms to Tokyo.
//!   Results are grouped into each game's own matchmaking regions.
//!
//! Games whose servers can't be measured from outside (Riot...) aren't listed.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

enum Servers {
    Valve,
    /// (region name, AWS region codes)
    Aws(&'static [(&'static str, &'static [&'static str])]),
}

struct Game {
    id: &'static str,
    name: &'static str,
    servers: Servers,
    steam: &'static [&'static str],
    epic: &'static [&'static str],
}

/// Fortnite's matchmaking regions and their data centres (as Fortnite logs
/// them: EU = FR, GB, DE; ME = IL, MUM...). NA-Central isn't on AWS.
const FORTNITE: &[(&str, &[&str])] = &[
    ("Europe", &["eu-west-3", "eu-west-2", "eu-central-1"]),
    ("NA-East", &["us-east-1", "us-east-2"]),
    ("NA-West", &["us-west-1", "us-west-2"]),
    ("Middle East", &["il-central-1", "ap-south-1"]),
    ("Brazil", &["sa-east-1"]),
    ("Asia", &["ap-northeast-1"]),
    ("Oceania", &["ap-southeast-2"]),
];

/// Rocket League regions (from the servers its region pinger contacts).
const ROCKET_LEAGUE: &[(&str, &[&str])] = &[
    ("Europe", &["eu-west-3", "eu-west-2", "eu-central-1", "eu-west-1", "eu-north-1"]),
    ("US-East", &["us-east-1", "us-east-2"]),
    ("US-West", &["us-west-1", "us-west-2"]),
    ("India", &["ap-south-1"]),
    ("South Africa", &["af-south-1"]),
    ("South America", &["sa-east-1"]),
    ("Asia-East", &["ap-east-1"]),
    ("Asia-SE", &["ap-southeast-1"]),
    ("Japan", &["ap-northeast-1"]),
    ("Oceania", &["ap-southeast-2"]),
];

/// Dead by Daylight uses Amazon GameLift; each data centre is its own choice.
const DBD: &[(&str, &[&str])] = &[
    ("Frankfurt", &["eu-central-1"]),
    ("London", &["eu-west-2"]),
    ("Dublin", &["eu-west-1"]),
    ("Virginia", &["us-east-1"]),
    ("Ohio", &["us-east-2"]),
    ("California", &["us-west-1"]),
    ("Oregon", &["us-west-2"]),
    ("Montreal", &["ca-central-1"]),
    ("São Paulo", &["sa-east-1"]),
    ("Mumbai", &["ap-south-1"]),
    ("Hong Kong", &["ap-east-1"]),
    ("Tokyo", &["ap-northeast-1"]),
    ("Seoul", &["ap-northeast-2"]),
    ("Singapore", &["ap-southeast-1"]),
    ("Sydney", &["ap-southeast-2"]),
];

const GAMES: &[Game] = &[
    Game { id: "cs2", name: "Counter-Strike 2", servers: Servers::Valve, steam: &["730"], epic: &[] },
    Game { id: "fortnite", name: "Fortnite", servers: Servers::Aws(FORTNITE), steam: &[], epic: &["Fortnite"] },
    Game { id: "rocketleague", name: "Rocket League", servers: Servers::Aws(ROCKET_LEAGUE), steam: &["252950"], epic: &["Sugar"] },
    Game { id: "dota2", name: "Dota 2", servers: Servers::Valve, steam: &["570"], epic: &[] },
    Game { id: "dbd", name: "Dead by Daylight", servers: Servers::Aws(DBD), steam: &["381210"], epic: &[] },
    Game { id: "deadlock", name: "Deadlock", servers: Servers::Valve, steam: &["1422450"], epic: &[] },
    Game { id: "tf2", name: "Team Fortress 2", servers: Servers::Valve, steam: &["440"], epic: &[] },
];

/// AWS region -> city.
fn aws_city(code: &str) -> &'static str {
    match code {
        "us-east-1" => "Virginia",
        "us-east-2" => "Ohio",
        "us-west-1" => "California",
        "us-west-2" => "Oregon",
        "ca-central-1" => "Montreal",
        "sa-east-1" => "São Paulo",
        "eu-central-1" => "Frankfurt",
        "eu-west-1" => "Dublin",
        "eu-west-2" => "London",
        "eu-west-3" => "Paris",
        "eu-north-1" => "Stockholm",
        "il-central-1" => "Tel Aviv",
        "af-south-1" => "Cape Town",
        "ap-south-1" => "Mumbai",
        "ap-east-1" => "Hong Kong",
        "ap-northeast-1" => "Tokyo",
        "ap-northeast-2" => "Seoul",
        "ap-southeast-1" => "Singapore",
        "ap-southeast-2" => "Sydney",
        _ => "",
    }
}

/// Valve relay cities: (city, country, relay IPs). Cached for 6 hours.
static VALVE: Mutex<Option<(Instant, Vec<(String, String, Vec<String>)>)>> = Mutex::new(None);

/// Every game, installed ones first: [{id, name, installed, kind}]
pub fn games() -> Vec<Value> {
    let have = crate::games::installed();
    let has = |store: &str, id: &str| have.iter().any(|g| g.id == format!("{store}:{id}"));
    let mut out: Vec<Value> = GAMES
        .iter()
        .map(|g| {
            let installed = g.steam.iter().any(|id| has("steam", id)) || g.epic.iter().any(|id| has("epic", id));
            json!({ "id": g.id, "name": g.name, "installed": installed, "kind": match g.servers { Servers::Valve => "valve", Servers::Aws(_) => "aws" } })
        })
        .collect();
    out.sort_by_key(|g| !g["installed"].as_bool().unwrap_or(false));
    out
}

/// "Stockholm - Kista (Sweden)" -> ("Stockholm - Kista", "Sweden"),
/// "Datapacket Frankfurt (Germany)" -> ("Frankfurt", "Germany · Datapacket")
fn split_desc(desc: &str) -> (String, String) {
    let (mut city, mut country) = match desc.rsplit_once(" (") {
        Some((city, rest)) => (city.trim().to_string(), rest.trim_end_matches(')').to_string()),
        None => (desc.trim().to_string(), String::new()),
    };
    for host in ["Alibaba Cloud", "Tencent", "Datapacket", "Hetzner"] {
        if let Some(rest) = city.strip_prefix(host) {
            city = rest.trim().to_string();
            country = if country.is_empty() { host.to_string() } else { format!("{country} · {host}") };
        }
    }
    (city, country)
}

fn valve_relays() -> Result<Vec<(String, String, Vec<String>)>, String> {
    if let Some((t, v)) = VALVE.lock().as_ref() {
        if t.elapsed() < Duration::from_secs(6 * 3600) {
            return Ok(v.clone());
        }
    }
    let agent = crate::log::http(ureq::AgentBuilder::new()).timeout(Duration::from_secs(8)).build();
    let v: Value = agent
        .get("https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid=730")
        .call()
        .map_err(|e| format!("Couldn't get Valve's server list: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (_, pop) in v["pops"].as_object().ok_or("Valve's server list was empty")? {
        let ips: Vec<String> = pop["relays"]
            .as_array()
            .map(|r| r.iter().filter_map(|x| x["ipv4"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        if ips.is_empty() {
            continue;
        }
        let (city, country) = split_desc(pop["desc"].as_str().unwrap_or(""));
        out.push((city, country, ips));
    }
    *VALVE.lock() = Some((Instant::now(), out.clone()));
    Ok(out)
}

/// Median of the replies and the share that were lost.
fn stats(samples: &[Option<f64>]) -> (Option<f64>, f64) {
    let mut got: Vec<f64> = samples.iter().flatten().cloned().collect();
    let loss = if samples.is_empty() { 1.0 } else { 1.0 - got.len() as f64 / samples.len() as f64 };
    if got.is_empty() {
        return (None, loss);
    }
    got.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let m = got.len() / 2;
    let median = if got.len() % 2 == 1 { got[m] } else { (got[m - 1] + got[m]) / 2.0 };
    (Some(median), loss)
}

const TRIES: usize = 5;
const GAP: Duration = Duration::from_millis(120);

/// Best relay of a Valve city: its relays (up to 6) are pinged side by side.
fn valve_city(ips: &[String]) -> (Option<f64>, f64) {
    let jobs: Vec<Box<dyn FnOnce() -> (Option<f64>, f64) + Send>> = ips
        .iter()
        .take(6)
        .cloned()
        .map(|ip| {
            Box::new(move || {
                let mut s = Vec::new();
                for i in 0..TRIES {
                    if i > 0 {
                        std::thread::sleep(GAP);
                    }
                    s.push(crate::net::ping(&ip, 800).map(|ms| ms as f64));
                }
                stats(&s)
            }) as Box<dyn FnOnce() -> _ + Send>
        })
        .collect();
    run_parallel(jobs)
        .into_iter()
        .filter(|r| r.0.is_some())
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .unwrap_or((None, 1.0))
}

/// UDP round trips to an Amazon GameLift ping server (it echoes the packet).
fn aws_dc(code: &str) -> (Option<f64>, f64) {
    let Some(addr) = format!("gamelift-ping.{code}.api.aws:7770").to_socket_addrs().ok().and_then(|mut a| a.find(SocketAddr::is_ipv4)) else {
        return (None, 1.0);
    };
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else { return (None, 1.0) };
    if sock.connect(addr).is_err() {
        return (None, 1.0);
    }
    let _ = sock.set_read_timeout(Some(Duration::from_millis(1000)));
    let mut buf = [0u8; 64];
    // The first packet sets up the route (NAT, ARP) and is often slow or lost.
    let _ = sock.send(b"DECK-warmup");
    let _ = sock.recv(&mut buf);
    let mut s = Vec::new();
    for i in 0..TRIES {
        std::thread::sleep(GAP);
        let msg = format!("DECK-{i}");
        let t = Instant::now();
        if sock.send(msg.as_bytes()).is_err() {
            s.push(None);
            continue;
        }
        // Skip late replies to earlier packets.
        let mut got = None;
        while t.elapsed() < Duration::from_millis(1000) {
            match sock.recv(&mut buf) {
                Ok(n) if &buf[..n] == msg.as_bytes() || !buf[..n].starts_with(b"DECK-") => {
                    got = Some(t.elapsed().as_secs_f64() * 1000.0);
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        s.push(got);
    }
    stats(&s)
}

fn run_parallel<T: Send + 'static>(jobs: Vec<Box<dyn FnOnce() -> T + Send>>) -> Vec<T> {
    let handles: Vec<_> = jobs.into_iter().map(std::thread::spawn).collect();
    handles.into_iter().filter_map(|h| h.join().ok()).collect()
}

fn sort_rows(rows: &mut [Value]) {
    rows.sort_by(|a, b| {
        let ms = |v: &Value| v["ms"].as_f64().unwrap_or(f64::MAX);
        ms(a).partial_cmp(&ms(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Ping one game's servers, fastest first: [{name, sub, ms, loss}]
pub fn ping(id: &str) -> Result<Vec<Value>, String> {
    let game = GAMES.iter().find(|g| g.id == id).ok_or("unknown game")?;
    let mut rows = match game.servers {
        Servers::Valve => {
            let jobs: Vec<Box<dyn FnOnce() -> Value + Send>> = valve_relays()?
                .into_iter()
                .map(|(city, country, ips)| {
                    Box::new(move || {
                        let (ms, loss) = valve_city(&ips);
                        json!({ "name": city, "sub": country, "ms": ms, "loss": loss })
                    }) as Box<dyn FnOnce() -> Value + Send>
                })
                .collect();
            run_parallel(jobs)
        }
        Servers::Aws(regions) => {
            let mut codes: Vec<&'static str> = regions.iter().flat_map(|r| r.1.iter().copied()).collect();
            codes.sort();
            codes.dedup();
            let jobs: Vec<Box<dyn FnOnce() -> (&'static str, (Option<f64>, f64)) + Send>> =
                codes.into_iter().map(|c| Box::new(move || (c, aws_dc(c))) as Box<dyn FnOnce() -> _ + Send>).collect();
            let dcs = run_parallel(jobs);
            regions
                .iter()
                .map(|(name, codes)| {
                    let mut cities: Vec<(&str, Option<f64>, f64)> =
                        codes.iter().filter_map(|c| dcs.iter().find(|d| d.0 == *c).map(|d| (aws_city(c), d.1 .0, d.1 .1))).collect();
                    cities.sort_by(|a, b| a.1.unwrap_or(f64::MAX).partial_cmp(&b.1.unwrap_or(f64::MAX)).unwrap());
                    // Matchmaking picks the best data centre of the region.
                    let best = cities.first().cloned();
                    let sub = if codes.len() > 1 || aws_city(codes[0]) != *name {
                        cities
                            .iter()
                            .map(|(c, ms, _)| match ms {
                                Some(ms) => format!("{c} {}", ms.round()),
                                None => format!("{c} –"),
                            })
                            .collect::<Vec<_>>()
                            .join(" · ")
                    } else {
                        String::new()
                    };
                    json!({ "name": name, "sub": sub, "ms": best.and_then(|b| b.1), "loss": best.map(|b| b.2).unwrap_or(1.0) })
                })
                .collect()
        }
    };
    sort_rows(&mut rows);
    Ok(rows)
}
